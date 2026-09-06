# User-configured MCP servers (local stdio + remote HTTP) — design

Date: 2026-09-05
Status: pending review
Related: `PRD.md:117` ("3rd-party MCP servers" architecture diagram entry this closes the gap on)

## Problem

`PRD.md:117` lists "3rd-party MCP servers" as a tool source, but there is no way for a user to add
one. `ToolEndpoint` (`crates/savvagent-host/src/config.rs:27`) has exactly one variant,
`Stdio { command, args }` — a remote server can't be expressed in the type system at all. The server
set is hardcoded as `ToolBins { fs, bash, grep, lsp, web }` (`crates/savvagent/src/main.rs:118`),
each resolved by `locate_bundled_bin` and appended via `HostConfig::with_tool` — adding a server means
adding a struct field and rebuilding. `~/.savvagent/config.toml` (`crates/savvagent/src/config_file.rs`)
has no server list. There is no `/mcp` slash command. The only remote-MCP path today is the
*provider* side (`SAVVAGENT_PROVIDER_URL`, rmcp Streamable HTTP) — that transport can't host tools.

## Premise corrections

The issue's "Work involved" section states the registry's "spawn/reap path currently assumes a child
process and will need a per-endpoint connection abstraction." This is only half true: `ToolServer`
(`crates/savvagent-host/src/tools.rs:301-304`) stores `service: RunningService<RoleClient,
ResourceCapturingHandler>` — the transport is erased once `.serve(transport)` returns. `connect()`
can build that same `RunningService` from either a `TokioChildProcess` or a
`StreamableHttpClientTransport` and push the result into the existing `eager_servers: Vec<ToolServer>`
— **no new connection-kind enum or trait object is needed for storage**, only a second match arm in
`connect()`'s construction logic. This materially shrinks the "Work involved" estimate for the
`ToolRegistry` side of this change.

`rmcp`'s `transport-streamable-http-client`/`-reqwest` features are already enabled
(`Cargo.toml:93-105`, confirmed in use by `savvagent-host/src/provider.rs` and every provider crate's
integration tests) — no new base dependency, only the `auth` feature (deferred, see Scope) would be
new.

## Scope

**In (v1):**

- `ToolEndpoint::Http { url, auth }` variant (`auth`: `None` | `Bearer` — a static token resolved
  from the OS keyring, never inline in config).
- `ToolRegistry::connect` gains an `Http` construction arm building a `StreamableHttpClientTransport`
  and pushing the resulting `ToolServer` into the same `eager_servers` vec used today.
- `[[mcp_servers]]` TOML array in `~/.savvagent/config.toml` (`ConfigFile`), covering both `stdio` and
  `http` transports, `env` values markable `"keyring"` for stdio, `auth = "bearer"` for http.
- Startup wiring: configured servers are registered **in addition to** `ToolBins`'s bundled set.
- Keyring: `mcp:<server name>` account namespace under service `savvagent`; add `creds::delete`;
  reuse `creds::save`/`load` shape.
- A `/mcp` slash command + screen (mirroring `/connect`'s plugin shape) listing configured servers
  with connected/failed status, and add/remove flows that write `config.toml` + the keyring.
- Third-party tools land on the `ask` permission default (no special-casing beyond what
  `ArgPattern::for_call`'s existing fallthrough already does).
- Failure handling: an unreachable/misconfigured server degrades (log + omit its tool surface),
  never blocks startup.
- Docs: README "Extending" section, `PRD.md` tool-server table, `CLAUDE.md`'s "/connect is the only
  keyring writer" invariant (now two writers: `/connect` and `/mcp`).

**Out (v1 — explicit deferrals, not silent cuts):**

- **OAuth 2.1 + PKCE + dynamic client registration.** The issue's own text flags this as the
  hardest, least-certain part ("Decide whether a browser-less environment falls back to paste",
  "Scope of v1" left as an open question) and it is a distinct, large feature (loopback HTTP
  listener, browser launch, token refresh, multi-field keyring blob) deserving its own design cycle.
  v1 ships static bearer tokens only. A server configured with `auth = "oauth"` is rejected at load
  with a clear note ("oauth is not yet supported; use auth = \"bearer\" or omit auth"), never silently
  ignored or half-registered. **A follow-up issue should be filed for OAuth support** once this ships.
- **SSE transport.** Only Streamable HTTP, per the issue's own "is Streamable HTTP alone sufficient"
  question — SSE is not implemented; a server with an unrecognized `transport` value is rejected the
  same way as unsupported `auth`.
- **Per-project config** (`.savvagent/config.toml` in the project root). v1 is home-config-only,
  matching every other section of `ConfigFile` today. Per-project merge is a natural follow-up but
  changes `ConfigFile`'s loading contract and is out of scope here.
- **Live add/remove/reconnect without restart.** `Host`'s provider pool
  (`add_provider`/`remove_provider`/`set_active_provider`) has no tool-side equivalent — `ToolRegistry`
  is built once from `HostConfig::tools` at `Host::start` and has no runtime mutation API today.
  Building one is a real architecture addition (mirroring the provider pool's `RwLock<HashMap<...>>`
  shape) that deserves its own review, not a rider on this change. v1's `/mcp` screen lists status,
  and add/remove edit `config.toml` + the keyring but require an app restart to take effect — the
  screen says so explicitly ("Restart savvagent to apply changes"). `Host` gaining a tool-pool API
  analogous to the provider pool is the natural v2.
- **Multi-secret servers.** Per the issue's own note ("pick one before the `/mcp` screen is
  written; the flat-account form matches the existing code better"), v1 uses one flat `mcp:<server>`
  keyring account per server, matching `creds.rs`'s existing `Entry::new(SERVICE, account)` shape. A
  stdio server whose `env` map marks more than one value `"keyring"` is rejected at load with a note
  ("only one keyring-backed env value is supported per server in this version") rather than silently
  using just one. Widening to `mcp:<server>:<var>` or a JSON blob is deferred to whenever OAuth (which
  needs a blob anyway for its token set) is built.

## Approach

### 1. `ToolEndpoint::Http` (`crates/savvagent-host/src/config.rs`)

```rust
pub enum ToolEndpoint {
    Stdio { command: PathBuf, args: Vec<String> },
    Http { url: String, auth: HttpAuth },
}

pub enum HttpAuth {
    None,
    Bearer { token: String },
}
```

`token` is always a resolved secret by the time it reaches `HostConfig` — `savvagent-host` never
reads a keyring or a config file; the embedder (TUI) resolves secrets before constructing
`ToolEndpoint` values, exactly as it already does for provider API keys. This keeps
`savvagent-host` free of a `keyring`/`toml` dependency, preserving the crate-boundary invariant
(`savvagent-host` is a library with no OS-credential-store awareness).

### 2. `ToolRegistry::connect` (`crates/savvagent-host/src/tools.rs`)

Add an arm alongside the existing `ToolEndpoint::Stdio` handling in `connect()` (near line 399):

```rust
ToolEndpoint::Http { url, auth } => {
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(url.clone()),
    );
    // `auth` becomes an Authorization header via the transport's client builder
    // when `HttpAuth::Bearer` — `None` builds the transport with no auth header.
    let service = handler.serve(transport).await?;
    // list_all_tools, register routes, push ToolServer { label, service } —
    // identical to the stdio arm from here on.
}
```

No sandbox wrapping applies to HTTP endpoints (`SandboxConfig` governs process spawns; a remote HTTP
call has no local process to sandbox). This is a scope decision worth a one-line callout in the
`/mcp` screen's help text ("remote servers are not sandboxed; only add ones you trust").

**Tool-name collisions.** `routes: HashMap<String, usize>` is a flat namespace. Bundled `ToolBins`
entries are pushed into `HostConfig::tools` before configured `mcp_servers` entries (established by
`ToolBins::apply` running first in the bootstrap sequence — see §3). `connect()` iterates
`self.tools` in order and inserts into `routes`; change the insert to skip (with a logged warning,
surfaced as an `app.push_note`) rather than overwrite when a tool name already has a route. This
gives bundled tools priority deterministically, matching the issue's requirement for "a defined
resolution."

### 3. `McpServersSection` (`crates/savvagent/src/config_file.rs`)

```toml
[[mcp_servers]]
name = "github"
transport = "stdio"
command = "/usr/local/bin/github-mcp-server"
args = ["--read-only"]
env = { GITHUB_TOKEN = "keyring" }

[[mcp_servers]]
name = "sentry"
transport = "http"
url = "https://mcp.sentry.dev/mcp"
auth = "bearer"
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigFile {
    pub startup: StartupSection,
    pub migration: MigrationSection,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "lowercase")]
pub enum McpServerEntry {
    Stdio {
        name: String,
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>, // value "keyring" is a sentinel, resolved at bootstrap
    },
    Http {
        name: String,
        url: String,
        #[serde(default)]
        auth: McpAuthMode, // "none" (default) | "bearer" | "oauth" (rejected at load, see Scope)
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpAuthMode {
    #[default]
    None,
    Bearer,
    Oauth,
}
```

**Validation** (new `McpServerEntry::validate(&self, seen_names: &HashSet<String>) -> Result<(),
String>`, called during bootstrap before any tool is registered): name non-empty, unique across
`mcp_servers` (case-sensitive — bundled tool names `fs`/`bash`/`grep`/`lsp`/`web` are a different
namespace, see §2's collision handling at the tool-name level, not the server-name level), no `:`
character (reserved for the keyring account separator), `auth != Oauth`, `transport` not `sse` or
anything unrecognized (serde already rejects unknown `transport` values as a deserialize error — that
error is caught and turned into a per-entry skip-with-note rather than aborting `ConfigFile::load`
for the whole file), and at most one `env` value equal to `"keyring"` for `Stdio`.

### 4. Bootstrap wiring (`crates/savvagent/src/main.rs`)

Near `bootstrap_app_and_host` / `ToolBins::apply` (main.rs:137-154): after the bundled `ToolBins` are
applied, iterate `config_file.mcp_servers`, validate each, resolve secrets (`creds::mcp_load(name)`
for `Bearer`/keyring-env), and append the resulting `ToolEndpoint` via the same `.with_tool(...)`
builder. A server that fails validation or whose keyring secret is missing/unreadable is skipped
with an `app.push_note` (mirroring `/connect`'s `notes.keyring-store-failed` pattern) — never a
startup abort, matching the issue's "Failure mode" requirement and this repo's error-handling
invariant (no `unwrap()`, no silent fallback, actionable message).

### 5. Keyring (`crates/savvagent/src/creds.rs`)

```rust
/// Reserved account prefix for MCP-server secrets, namespaced against
/// provider accounts of the same name (`/connect` writes bare `<provider
/// id>`; `/mcp` always writes `mcp:<server name>`).
const MCP_PREFIX: &str = "mcp:";

pub fn mcp_save(server_name: &str, secret: &str) -> Result<(), keyring::Error> {
    save(&format!("{MCP_PREFIX}{server_name}"), secret)
}

pub fn mcp_load(server_name: &str) -> Result<Option<String>, keyring::Error> {
    load(&format!("{MCP_PREFIX}{server_name}"))
}

pub fn delete(account: &str) -> Result<(), keyring::Error> {
    let entry = match Entry::new(SERVICE, account) {
        Ok(e) => e,
        Err(keyring::Error::NoStorageAccess(_)) => return Ok(()),
        Err(e) => return Err(e),
    };
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(keyring::Error::NoStorageAccess(_)) => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn mcp_delete(server_name: &str) -> Result<(), keyring::Error> {
    delete(&format!("{MCP_PREFIX}{server_name}"))
}
```

`CLAUDE.md`'s "`/connect` is the only writer" line is updated to name both `/connect` and `/mcp` as
the two writers, and to note the `mcp:` account-prefix reservation.

### 6. `/mcp` slash command + screen (`crates/savvagent/src/plugin/builtin/mcp/`)

New built-in plugin, structured like `connect/` (`mod.rs` + `screen.rs`):

- `mod.rs`: registers slash command `"mcp"`, screen id `"mcp.manager"`. With no args, opens the
  manager screen. `HostEvent`/status is sourced from a new read-only accessor
  `Host::tool_server_statuses() -> Vec<ToolServerStatus>` (`name`, `transport` kind,
  `Connected`/`Failed { reason }`) — a small addition to `Host`/`ToolRegistry` alongside the existing
  `tool_defs()` accessor, not a new mutation surface.
- `screen.rs`: lists configured servers with status; `a` opens an add form (name, transport,
  command/args or url, optional secret — masked input identical to `/connect`'s API-key prompt);
  `d`/`Delete` removes a server (calls `creds::mcp_delete`, rewrites `config.toml` without the entry,
  and pushes a note "Restart savvagent to apply changes"); `r` is a no-op in v1 beyond re-reading
  status (no live reconnect — see Scope).
- Failure surfaces inline per-row ("failed: connection refused") rather than a global note, so
  multiple failing servers are all visible at once.

### 7. Permissions

No policy-engine change. `ArgPattern::for_call` already falls through to `Any` for tool names it
doesn't special-case (`permissions.rs:133-165`), and `PermissionPolicy::evaluate`'s built-in-defaults
tier (`permissions.rs:402-429`) governs anything without an explicit rule — this is already the `ask`
default the issue asks for. No source/server identity flows into the policy key today; this spec
does not add one (a `Rule` keyed by server-not-just-tool-name is worth a follow-up if false-positive
collisions across servers exposing same-named tools become a real complaint, but is speculative today
and out of scope).

## Public-interface changes

- **Additive:** `ToolEndpoint::Http` variant (new enum arm on a `#[non_exhaustive]`-eligible type —
  confirm `ToolEndpoint` is already `#[non_exhaustive]` or add it now so this and future variants
  don't break downstream matches; if it is not already marked, adding it is itself listed as a task
  below since match-exhaustiveness on a public host-config enum is exactly the kind of interface
  surface Non-Negotiable Rule 6 cares about).
- **Additive:** `[[mcp_servers]]` TOML section — an unrecognized/absent section deserializes to the
  default empty vec (`#[serde(default)]`), so existing `config.toml` files keep working unchanged.
- **Additive:** `/mcp` slash command, `creds::delete`/`mcp_save`/`mcp_load`/`mcp_delete` functions.
- **Documented behavior change, not a breaking wire/ABI change:** `CLAUDE.md`'s "`/connect` is the
  only writer" invariant becomes "two writers, namespaced" — this is a documentation update, not a
  format change; existing provider keyring entries (bare `<provider id>` accounts) are untouched.

## Assumptions

- OAuth is deferred to a follow-up issue rather than attempted in this change — the issue text itself
  treats OAuth's scope as an open question, and PKCE + dynamic client registration + a loopback
  callback listener is large enough to warrant independent spec/plan/review.
- SSE transport is not implemented — Streamable HTTP is sufficient for the servers named in the
  issue's own example (Sentry, Linear, GitHub's remote endpoint all speak Streamable HTTP or bearer
  stdio).
- Per-project `mcp_servers` config is deferred — v1 is home-config-only like every other
  `ConfigFile` section today.
- Live add/remove/reconnect without an app restart is deferred — `Host` has no tool-pool mutation API
  today (unlike its provider pool) and building one is substantial enough to warrant its own review.
  `/mcp`'s add/remove writes config + keyring and asks for a restart.
- One keyring secret per server (flat `mcp:<server>` account) — matches the issue's own preferred
  fallback and `creds.rs`'s existing shape; multi-secret stdio servers are rejected at load with a
  clear note rather than silently dropping all-but-one secret.
- Remote HTTP tool servers are not sandboxed (no local process exists to sandbox) — flagged in the
  `/mcp` screen's help text as a trust boundary the user should understand before adding a server.
- Bundled tool names win tool-name collisions against configured servers; the losing tool is skipped
  with a logged note, not silently shadowed.

## Error Handling & Edge Cases

- Config file has `mcp_servers` entries with duplicate names → both/all duplicates after the first
  are skipped with a note; the first-seen entry registers.
- `auth = "oauth"` or `transport = "sse"`/unrecognized → entry skipped with an explicit
  not-yet-supported note, never silently coerced to `none`/`stdio`.
- Keyring secret configured but unreadable (`NoStorageAccess`, no Secret Service on a headless Linux
  box) → server skipped with a note; app still starts.
- HTTP server unreachable at startup (connection refused, DNS failure, TLS error) → same
  skip-with-note treatment; the app does not block or retry indefinitely at startup.
- `env` map with more than one `"keyring"` value → entry skipped with a note naming the offending
  server and both variable names.
- Removing a server via `/mcp` whose keyring entry is already gone → `creds::mcp_delete` treats
  `NoEntry` as success (mirrors `load`'s existing `NoEntry` handling), so remove is idempotent.

## Risks & Open Questions

- Should `Host` eventually grow a tool-pool API mirroring `add_provider`/`remove_provider` so `/mcp`
  can reconnect live? Flagged above as the natural v2; not attempted here.
- Should the collision-resolution note distinguish "bundled tool wins" from "first-configured-server
  wins" more visibly in the UI (today: a note, not a persistent screen indicator)? Low risk, cheap to
  improve later.
- File a follow-up issue for OAuth support once this ships, referencing this spec's Scope section for
  what to build on top of.

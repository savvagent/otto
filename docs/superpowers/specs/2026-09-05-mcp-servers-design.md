# User-configured MCP servers (local stdio + remote HTTP) — design

Date: 2026-09-05
Status: pending review
Source: savvagent/savvagent-cli#36
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
#[non_exhaustive]
pub enum ToolEndpoint {
    Stdio {
        /// Stable identity for this endpoint — `"fs"`/`"bash"`/`"grep"`/`"lsp"`/`"web"`
        /// for bundled `ToolBins` entries, or the configured `[[mcp_servers]].name`
        /// for user-configured servers. This is what `ToolServerStatus::name` (§2)
        /// and `/mcp`'s listing (§6) key on — never derived from `command`/`url`,
        /// which are ambiguous when two entries share a binary or endpoint.
        name: String,
        command: PathBuf,
        args: Vec<String>,
        /// Resolved (never `"keyring"`-sentinel) environment variables to
        /// set on the spawned child, merged over the process's inherited
        /// environment. Empty for the bundled `ToolBins` endpoints today.
        env: HashMap<String, String>,
    },
    Http {
        /// Same role as `Stdio::name` above — the configured server's stable
        /// identity, independent of `url`.
        name: String,
        url: String,
        auth: HttpAuth,
    },
}

pub enum HttpAuth {
    None,
    Bearer { token: String },
}
```

`token`/`env` values are always resolved secrets by the time they reach `HostConfig` —
`savvagent-host` never reads a keyring or a config file; the embedder (TUI) resolves secrets before
constructing `ToolEndpoint` values, exactly as it already does for provider API keys. This keeps
`savvagent-host` free of a `keyring`/`toml` dependency, preserving the crate-boundary invariant
(`savvagent-host` is a library with no OS-credential-store awareness).

`ToolRegistry`'s stdio spawn path (`crates/savvagent-host/src/tools.rs`, the `tokio::process::Command`
construction shared by both the eager and lazy-bash arms) is extended to call `.envs(&env)` on the
built command before wrapping it in `TokioChildProcess::new`, merging over (not replacing) whatever
environment the process already inherits — the existing sandbox environment handling is untouched,
`env` is applied in addition to it. `ToolServer.label` (`tools.rs:301-304`) is populated from
`name`, not derived from `command`, closing the identity-ambiguity gap flagged in spec review.

### 2. `ToolRegistry::connect` (`crates/savvagent-host/src/tools.rs`)

**Per-endpoint failure isolation.** `connect()` today propagates the first endpoint failure via `?`,
which aborts `Host::start` entirely — acceptable when every endpoint is a trusted bundled binary, but
wrong once a user-configured server can be unreachable. `connect()` changes shape: each
`ToolEndpoint` in `self.tools` is attempted independently inside a `match` arm that captures
`Result<ToolServer, ConnectError>` instead of using `?` at the top level; a failure is recorded (see
status record below) and iteration continues to the next endpoint. Only a failure that indicates a
genuine programming/config error the embedder must know about before anything runs (there is none
identified for this change) would still abort — endpoint-reachability failures never do. This is a
behavior change to the *bundled*-tool path too (a missing bundled binary today already degrades
gracefully per `ToolBins`'s `Option<PathBuf>` — this makes the underlying registry consistent with
that, not looser).

**Status record.** Add:

```rust
pub struct ToolServerStatus {
    pub name: String,             // ToolEndpoint::name — see §1
    pub transport: TransportKind, // Stdio | Http
    pub state: ConnectState,      // Connected | Failed { reason: String }
}
```

`ToolRegistry` gains a `statuses: Vec<ToolServerStatus>` field populated during `connect()` (both
success and failure cases), and a `fn statuses(&self) -> &[ToolServerStatus]` accessor. `Host` exposes
this as `Host::tool_server_statuses()` (§6) — this is the authoritative data source the `/mcp` screen
reads; it is not derived from `eager_servers` alone (which only holds successes) or from an
approximation on the embedder side.

**Two-stage timeout so `service.cancel()` is always reachable (fix from round-3 review).** A timeout
scoped to only the transport handshake leaves `list_all_tools()` (and, for stdio, the child-process
spawn + MCP initialization before the handshake even starts) unbounded — a server that connects but
never answers `tools/list` could still hang `Host::start`. An earlier draft wrapped `serve()` *and*
`list_all_tools()` inside a single `async` block whose only externally-visible outcome was a
`Result` — on a post-`serve` failure or timeout, the already-created `RunningService` was moved into
that block and never escaped it, so there was nothing to call `.cancel()` on (a round-3 review
finding). The corrected shape binds `service` in the *outer* scope as soon as `serve()` returns, then
applies a second, independent `tokio::time::timeout_at` to `list_all_tools()` against the same overall
deadline (both stages share one deadline derived from `HostConfig::connect_timeout_ms`, rather than
inventing a second timeout constant), so every failure path after `serve()` succeeds has `service`
available to cancel:

```rust
ToolEndpoint::Http { name, url, auth } => {
    let deadline = Instant::now() + connect_timeout;
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(url.clone()),
    );
    // HttpAuth::Bearer sets an Authorization: Bearer <token> header via
    // rmcp's StreamableHttpClientTransportConfig::auth_header (rmcp 1.6,
    // transport/streamable_http_client.rs) at transport-construction time;
    // HttpAuth::None builds the transport with no auth header. The token
    // is never logged — only passed to the header builder.
    let serve_result = tokio::time::timeout_at(deadline, handler.serve(transport)).await;
    let service = match serve_result {
        Ok(Ok(service)) => service,
        // `serve()` itself timed out or failed: nothing was ever bound to
        // `service`, so there is nothing to cancel — record and move on.
        Ok(Err(e)) => { /* record ConnectState::Failed { reason: e.to_string() }; continue */ }
        Err(_) => { /* record ConnectState::Failed { reason: "connect timed out" }; continue */ }
    };
    match tokio::time::timeout_at(deadline, service.list_all_tools()).await {
        Ok(Ok(tools)) => { /* register routes per-tool (see collision handling
                               below), push ToolServer { label: name, service },
                               record ConnectState::Connected */ }
        Ok(Err(e)) => {
            service.cancel().await;
            /* record ConnectState::Failed { reason: e.to_string() } */
        }
        Err(_) => {
            service.cancel().await;
            /* record ConnectState::Failed { reason: "connect timed out" } */
        }
    }
}
```

The `Stdio` arm follows the identical two-stage shape: bind the spawned/served `service` before
attempting `list_all_tools()`, and call `.cancel().await` on it for any failure discovered after that
point. A hung `initialize` handshake that never returns from `serve()` is covered by the first
`timeout_at` — because nothing was bound yet in that case, there is nothing to cancel explicitly;
`rmcp`'s `TokioChildProcess` kills its child on `Drop`, so dropping the abandoned `serve()` future
(which owns the child process handle) is the backstop for that specific case. The implementation
task must add a regression test using a stub MCP server that completes `initialize` successfully but
never replies to `tools/list`, asserting (a) `connect()` returns within `connect_timeout_ms` rather
than hanging, and (b) the server's child process is no longer running (or, for the HTTP case, its
connection is closed) once `connect()` returns.

No sandbox wrapping applies to HTTP endpoints (`SandboxConfig` governs process spawns; a remote HTTP
call has no local process to sandbox). This is a scope decision worth a one-line callout in the
`/mcp` screen's help text ("remote servers are not sandboxed; only add ones you trust").

**Tool-name collisions are per-tool, not per-endpoint (clarified per round-2 review).** `routes:
HashMap<String, usize>` is a flat namespace, but one endpoint can expose several tools. Bundled
`ToolBins` entries are pushed into `HostConfig::tools` before configured `mcp_servers` entries
(established by `ToolBins::apply` running first in the bootstrap sequence — see §4). After a
successful `list_all_tools()`, `connect()` registers each returned tool name into `routes`
individually; a name already present is skipped (not overwritten) and logged as a `app.push_note`
warning naming both the tool and the endpoint that lost the race, but this **does not** mark the
whole endpoint `Failed` — an endpoint with 5 tools where 1 collides is still `ConnectState::Connected`
overall (the other 4 tools are registered and usable), with the collision reported as a separate
per-tool note, not folded into the endpoint's own status reason. `ConnectState::Failed` is reserved
for "the endpoint itself never came up" (timeout, transport/handshake error, `list_all_tools` error),
not "some of its tools lost a name collision."

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
/// The `ConfigFile` struct itself is unchanged in shape at the type level;
/// `mcp_servers` decoding is tolerant at the loader level (see below), not
/// via a raw `Vec<toml::Value>` field — downstream code still sees
/// `Vec<McpServerEntry>`. `startup`/`migration` keep the `#[serde(default)]`
/// they already carry today (round-2 review flagged their omission here as
/// a drafting slip, not an intended change) — a `config.toml` containing
/// only `[[mcp_servers]]` entries, or only `[startup]`, or only
/// `[migration]`, all still load correctly.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigFile {
    #[serde(default)]
    pub startup: StartupSection,
    #[serde(default)]
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

**Tolerant per-entry loading (blocking fix from spec review).** Today `ConfigFile::load_or_default`
does one whole-document `toml::from_str::<ConfigFile>(&contents)` and falls back entirely to
`ConfigFile::default()` on any error — acceptable when the only failure mode is "the whole file is
garbage," wrong once one bad `[[mcp_servers]]` entry (an unknown `transport`/`auth` tag, a missing
required field) must not also discard valid `startup`/`migration` settings and every *other* valid
`mcp_servers` entry. `load_or_default` changes internally (its signature also changes — see below) to:

1. Parse the file contents into a `toml::Value` first (`contents.parse::<toml::Value>()`). A
   syntactically broken file (unparsable TOML at all) still falls back to `ConfigFile::default()`
   with zero diagnostics — this failure mode is unchanged and out of scope for this fix, which
   targets *semantically* invalid individual `mcp_servers` rows in an otherwise well-formed file.
2. Deserialize `startup`/`migration` from that `Value` as today (a top-level `ConfigFile`-shaped
   deserialize with `mcp_servers` temporarily treated as an opaque `Vec<toml::Value>` via a small
   `#[serde(skip)]`-adjacent internal type, or simply extracting the `mcp_servers` key from the
   parsed table before doing the strongly-typed remainder — implementation is free to pick either
   as long as a bad `mcp_servers` row cannot fail this step).
3. Extract the raw `mcp_servers` array (default empty if the key is absent or not an array) and
   attempt `McpServerEntry::deserialize` on each element independently. A per-element failure
   produces a diagnostic string (`"mcp_servers[<index>] (name = \"<name-if-present>\"): <serde
   error>"`) and is skipped; a success is appended to the returned `Vec<McpServerEntry>`.
4. Return both the assembled `ConfigFile` and the diagnostics:

```rust
pub struct LoadedConfig {
    pub config: ConfigFile,
    /// One line per skipped mcp_servers entry, ready to hand to app.push_note.
    pub mcp_server_diagnostics: Vec<String>,
}

impl ConfigFile {
    pub fn load_or_default(path: &Path) -> LoadedConfig { /* ... */ }
}
```

`bootstrap_app_and_host` (§4) surfaces each `mcp_server_diagnostics` entry via `app.push_note`,
exactly like the resolution-time skip notes from validation/keyring-lookup failures — both are the
same "here's a server I couldn't register and why" UX, just triggered at different phases.

**Write-side contract: `/mcp`'s add/remove must never silently delete a row it couldn't parse
(blocking fix from round-2 review; mechanism corrected per round-3 review).** If `/mcp` rewrote
`config.toml` by reserializing `LoadedConfig::config.mcp_servers` (the typed, successfully-parsed
entries only), any row that failed step 3's tolerant decode — a real row the user wrote, just with
e.g. a typo'd `transport` value — would be silently dropped the next time the user added or removed
an unrelated server, defeating the entire point of tolerant loading. An earlier draft proposed
parsing to `toml::Value` and serializing the whole document back — round-3 review correctly flagged
that `toml::Value` does not retain lexical formatting (comments, blank lines, key order, or even a
malformed table's original text), so "serialize the whole modified `toml::Value` back to disk" cannot
actually deliver byte-for-byte preservation; it can only guarantee semantic preservation of whatever
serde successfully modeled, which is exactly the data tolerant loading already excludes.

`/mcp`'s write path therefore uses `toml_edit::DocumentMut` (a new dependency; not the existing
`toml` crate, which does not preserve document structure) instead of `toml::Value`: it re-reads
`config.toml` fresh into a `DocumentMut`, locates the `mcp_servers` array-of-tables node, and mutates
only that node — appending a new `[[mcp_servers]]` table for "add" (built with `toml_edit::Table`/
`Item` construction, not string formatting), or removing the specific table whose `name` key matches
for "remove" via `ArrayOfTables::remove(index)` — while every other node in the document (valid or
malformed, including comments and formatting) is left untouched because `DocumentMut` only rewrites
the nodes it was asked to mutate. The document is then written back with `DocumentMut::to_string()`.
This replaces step 3's `toml::Value`-based raw-table representation for the *write* path specifically
— step 3's *read*-side tolerant decode (`toml::Value` → per-element `McpServerEntry::deserialize`)
is unaffected and continues to use `toml::Value`/`toml::from_str`, since round-tripping fidelity is
only required on write, not on read. `Cargo.toml` gains `toml_edit` as a new workspace dependency
(`crates/savvagent`-only; `savvagent-host` does not gain a `toml_edit` dependency, consistent with
the crate-boundary invariant in §1). The plan's implementation task for this includes a regression
test: a `config.toml` with one valid `[[mcp_servers]]` row, one intentionally-malformed row (e.g. a
row with an unrecognized `transport` value, plus a hand-written comment immediately above it), add a
third server via the write path, then reload the raw file text and confirm the malformed row's
original text *and* its preceding comment are byte-for-byte unchanged.

**Validation** (new `McpServerEntry::validate(&self, seen_names: &HashSet<String>) -> Result<(),
String>`, called during bootstrap on every successfully-*parsed* entry, before any tool is
registered — this runs after the tolerant-decode step above, on the entries that step 3 already
turned into typed `McpServerEntry` values): name non-empty, unique across `mcp_servers`
(case-sensitive — bundled tool names `fs`/`bash`/`grep`/`lsp`/`web` are a different namespace, see
§2's collision handling at the tool-name level, not the server-name level), no `:` character
(reserved for the keyring account separator), `auth != Oauth`, and at most one `env` value equal to
`"keyring"` for `Stdio`. (`transport = "sse"`/anything unrecognized is already rejected one step
earlier, during the tolerant per-entry deserialize in step 3 above — an unknown `#[serde(tag =
"transport")]` value is a deserialize error for that element, producing a diagnostic there; it never
reaches `McpServerEntry::validate` as a typed value at all.)

### 4. Bootstrap wiring (`crates/savvagent/src/main.rs`)

Near `bootstrap_app_and_host` / `ToolBins::apply` (main.rs:137-154): `ConfigFile::load_or_default`
now returns `LoadedConfig { config, mcp_server_diagnostics }` (§3) — bootstrap first pushes a note for
each `mcp_server_diagnostics` entry (bad TOML rows caught at parse time), then, after the bundled
`ToolBins` are applied, iterates `config.mcp_servers` (already-typed, already-parsed entries),
validates each, resolves secrets (`creds::mcp_load(name)` for `Bearer`/keyring-env), and appends the
resulting `ToolEndpoint` via the same `.with_tool(...)` builder. A server that fails validation or
whose keyring secret is missing/unreadable is skipped with an `app.push_note` (mirroring `/connect`'s
`notes.keyring-store-failed` pattern) — never a startup abort, matching the issue's "Failure mode"
requirement and this repo's error-handling invariant (no `unwrap()`, no silent fallback, actionable
message). Actual connection failures (unreachable HTTP server, timeout, tool-name collision) surface
later, at `Host::start`, via `Host::tool_server_statuses()` (§2/§6) — bootstrap-time notes and
startup-time statuses are deliberately two different signals for two different failure phases (config
malformed vs. config valid but the server itself unreachable).

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
  command/args or url, optional secret — masked input identical to `/connect`'s API-key prompt) and
  appends the new entry via §3's `toml_edit`-based write path; `d`/`Delete` removes a server —
  **config-then-credential ordering (non-blocking fix from round-3 review):** it first removes the
  server's raw table via that write path (preserving every other row — including any that failed to
  parse — untouched) and pushes a note "Restart savvagent to apply changes," and only *then* calls
  `creds::mcp_delete`. Removing the config row first means that if the config write itself fails
  (e.g. a permissions error on `config.toml`), the server is untouched end-to-end — nothing is left
  half-removed with a missing secret. If the config write succeeds but the subsequent
  `creds::mcp_delete` fails, the server is already gone from `config.toml` (so it will not be
  reloaded on restart) and the screen reports "server removed; stale credential could not be deleted"
  rather than silently leaving an orphaned keyring entry unmentioned; a leftover `mcp:<server>`
  keyring entry in that state is inert (nothing references it) and can be cleaned up by re-adding and
  re-removing the same server name, or manually via the OS credential manager. `r` is a no-op in v1
  beyond re-reading status (no live reconnect — see Scope).
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

- **Breaking (per Non-Negotiable Rule 6 — requires a MINOR version bump under this repo's pre-1.0
  convention, not a PATCH):** `ToolEndpoint` is not `#[non_exhaustive]` today
  (`crates/savvagent-host/src/config.rs`), so this change both (a) adds a new `Http` variant and (b)
  adds a new `env` field to the existing `Stdio` variant — either alone breaks any exhaustive
  `match`/struct-literal construction outside this workspace. This spec marks `ToolEndpoint` (and,
  for consistency, `HttpAuth`) `#[non_exhaustive]` **as part of this same breaking release**, so this
  is the last time adding a variant to `ToolEndpoint` requires a MINOR bump — this is a deliberate,
  documented breaking change, not an incidental refactor side effect, called out explicitly here,
  in the plan's release-line note, and in `CHANGELOG.md`. Every in-workspace construction site
  (`ToolBins::apply`, tests) is updated in the same PR to use the new `Stdio { command, args, env }`
  shape.
- **Behavior change, not a type-level break:** `ToolRegistry::connect`'s error-handling contract
  changes from "any endpoint failure aborts `Host::start`" to "endpoint failures are isolated and
  recorded; `Host::start` continues." This is additive in the sense that it makes startup *more*
  resilient, but any embedder code relying on `Host::start` failing when a single tool endpoint is
  bad (none identified in this workspace) would need to switch to checking
  `Host::tool_server_statuses()` instead. Called out explicitly since it changes an existing method's
  observable behavior, even though its signature is unchanged.
- **Additive:** `[[mcp_servers]]` TOML section — an unrecognized/absent section deserializes to the
  default empty vec (`#[serde(default)]`), so existing `config.toml` files keep working unchanged.
  `ConfigFile::load_or_default`'s return type changing to `LoadedConfig` (§3) is an internal-only
  signature change (this function has no callers outside `crates/savvagent`, which is the TUI binary
  crate, not a published/embeddable library per `CLAUDE.md`'s workspace map) — not governed by Rule 6,
  which concerns the SPP wire format, tool MCP schemas, the plugin ABI, slash commands, env vars, and
  on-disk formats specifically. The on-disk `config.toml` format itself is unchanged for existing
  users; only malformed-`mcp_servers`-row recovery is new.
- **Additive:** `/mcp` slash command, `creds::delete`/`mcp_save`/`mcp_load`/`mcp_delete` functions,
  `Host::tool_server_statuses()`.
- **New dependency (round-3 review):** `toml_edit` is added to root `Cargo.toml` as a new workspace
  dependency, used only by `crates/savvagent` (§3's `/mcp` write path). The existing `toml` crate is
  retained unchanged for read-side parsing/serialization everywhere else; `toml_edit` is not a
  replacement for it. `savvagent-host` gains no new dependency.
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
- HTTP connect handshake exceeds `connect_timeout_ms` → treated identically to a connection refusal:
  `ConnectState::Failed { reason: "connect timed out" }`, never a hang.
- Bearer token construction must never log the token. The plan's implementation task for §2's `Http`
  arm includes a test asserting the resolved token reaches the transport's `Authorization: Bearer
  <token>` header (via `StreamableHttpClientTransportConfig`'s auth-header builder, rmcp 1.6) without
  appearing in any `tracing`/log output produced during `connect()`.

## Risks & Open Questions

- Should `Host` eventually grow a tool-pool API mirroring `add_provider`/`remove_provider` so `/mcp`
  can reconnect live? Flagged above as the natural v2; not attempted here.
- Should the collision-resolution note distinguish "bundled tool wins" from "first-configured-server
  wins" more visibly in the UI (today: a note, not a persistent screen indicator)? Low risk, cheap to
  improve later.
- File a follow-up issue for OAuth support once this ships, referencing this spec's Scope section for
  what to build on top of.

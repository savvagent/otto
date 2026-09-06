# mcp-servers Implementation Plan

**Goal:** Let a user add third-party MCP tool servers — local stdio processes or remote Streamable
HTTP endpoints — via `~/.savvagent/config.toml` and a new `/mcp` slash command/screen, without
rebuilding the binary. Configured servers register alongside the bundled `ToolBins` set
(`fs`/`bash`/`grep`/`lsp`/`web`), degrade gracefully on individual failure, and never block
`Host::start`.

**Architecture:** `ToolEndpoint` (`crates/savvagent-host/src/config.rs`) gains an `Http` variant and
a `name`/`env` field, becoming `#[non_exhaustive]`. `ToolRegistry::connect`
(`crates/savvagent-host/src/tools.rs`) is reshaped to isolate per-endpoint failures behind a
two-stage `tokio::time::timeout_at` (spawn/serve, then `list_all_tools`), explicitly
`service.cancel().await`ing any endpoint that fails after `serve()` succeeds, and records a
`ToolServerStatus` per endpoint (surfaced via `Host::tool_server_statuses()`). `ConfigFile`
(`crates/savvagent/src/config_file.rs`) gains a tolerantly-decoded `mcp_servers: Vec<McpServerEntry>`
field — a single malformed row never drops `startup`/`migration` or other valid rows.
`crates/savvagent/src/creds.rs` gains `delete`/`mcp_save`/`mcp_load`/`mcp_delete` under a new
`mcp:<server>` keyring namespace. `crates/savvagent/src/main.rs`'s `bootstrap_pool_host` resolves
configured servers (validating + reading secrets) into `ToolEndpoint`s appended after `ToolBins`.
A new built-in plugin, `crates/savvagent/src/plugin/builtin/mcp/` (mirroring `connect/`), backs the
`/mcp` slash command and `mcp.manager` screen; its add/remove flows edit `config.toml` through a new
`toml_edit::DocumentMut`-based writer (`crates/savvagent/src/mcp_config_writer.rs`) that mutates only
the `mcp_servers` array-of-tables, leaving every other node — including malformed neighboring rows —
untouched, and push a "restart to apply" note (v1 has no live reconnect).

**Tech Stack:** Rust 2024, `rmcp` 1.6 (`transport-streamable-http-client`/`-reqwest` features already
enabled; no new rmcp feature), `toml` 0.8 (read-side, unchanged), `toml_edit` 0.25 (new dependency,
`crates/savvagent`-only, write-side only), `keyring`, `savvagent-plugin`'s `Screen`/`Effect` ABI.

**Spec:** `docs/superpowers/specs/2026-09-05-mcp-servers-design.md` — read it first, including the
"Premise corrections" section (the issue's own complexity estimate for `ToolRegistry` is inflated —
`ToolServer` already erases transport type) and the round-3-review-driven two-stage timeout and
`toml_edit` write-path designs in §2/§3. This plan implements it exactly.

**Release line:** the next **MINOR** version after whatever is latest-released at merge time — this
is a breaking `ToolEndpoint` change (new variant + new `Stdio` field on a type that is not
`#[non_exhaustive]` today), which the spec adds `#[non_exhaustive]` for as part of this same release.
Do not hardcode a version number here; confirm the exact next MINOR against `CHANGELOG.md`'s most
recent released heading when the release PR (Task 9's final note) is actually opened.

**Branch:** `host/mcp-servers` (already created for this work).

**File Map:**

- Modified: `crates/savvagent-host/src/config.rs` — `ToolEndpoint` gains `name`/`env` on `Stdio` and
  a new `Http { name, url, auth }` variant; both `ToolEndpoint` and a new `HttpAuth` enum are marked
  `#[non_exhaustive]`.
- Modified: `crates/savvagent-host/src/tools.rs` — `ToolRegistry::connect`'s per-endpoint loop is
  reshaped for failure isolation, two-stage timeout + `cancel()`, per-tool (not per-endpoint)
  collision handling, and a new `Http` construction arm; `ToolServerStatus`/`ConnectState`/
  `TransportKind` types added; `ToolRegistry` gains a `statuses` field + `statuses()` accessor;
  `ToolServer.label` now comes from `ToolEndpoint::name`, not `command.display()`.
- Modified: `crates/savvagent-host/src/session.rs` — `Host` gains `tool_server_statuses()` forwarding
  to `ToolRegistry::statuses()`.
- Modified: `crates/savvagent/src/config_file.rs` — `ConfigFile` gains `mcp_servers:
  Vec<McpServerEntry>`; new `McpServerEntry`/`McpAuthMode` types; `load_or_default` is rewritten to
  return `LoadedConfig { config, mcp_server_diagnostics }` via tolerant per-entry decode.
- Created: `crates/savvagent/src/mcp_config_writer.rs` — `toml_edit::DocumentMut`-based
  add/remove-by-name for the `mcp_servers` array-of-tables; used only by the `/mcp` plugin.
- Modified: `crates/savvagent/src/creds.rs` — add `delete`, `mcp_save`, `mcp_load`, `mcp_delete`.
- Modified: `crates/savvagent/src/main.rs` — `ToolBins::apply`'s caller in `bootstrap_pool_host`
  gains a step that validates `config_file.mcp_servers`, resolves secrets via `creds::mcp_load`, and
  appends resulting `ToolEndpoint`s; `load_or_default`'s new return shape is threaded through
  `bootstrap_app_and_host`.
- Created: `crates/savvagent/src/plugin/builtin/mcp/mod.rs`,
  `crates/savvagent/src/plugin/builtin/mcp/screen.rs` — the `/mcp` slash command + `mcp.manager`
  screen, mirroring `connect/`.
- Modified: `crates/savvagent/src/plugin/builtin/mod.rs` — add `pub mod mcp;` with a doc comment.
- Modified: `crates/savvagent/src/plugin/mod.rs` — register `McpPlugin` in `register_builtins`; update
  any test asserting the builtin plugin count/id list.
- Modified: `crates/savvagent/Cargo.toml` — add `toml_edit.workspace = true`.
- Modified: root `Cargo.toml` — add `toml_edit = "0.25"` under `[workspace.dependencies]`.
- Modified: `README.md` — "Extending" section gains a "New MCP server (user-configured)" recipe;
  document `/mcp`, the `[[mcp_servers]]` config format, and the keyring namespace.
- Modified: `PRD.md` — update the "3rd-party MCP servers" architecture-diagram entry (`PRD.md:117`)
  to reflect it's now implemented, with a pointer to the spec.
- Modified: `CLAUDE.md` — update the "`/connect` is the only writer" keyring invariant to "two
  writers, namespaced" (`/connect` and `/mcp`).
- Modified: `CHANGELOG.md` — add entries under `[Unreleased]` for the new `Http` variant/breaking
  change, `/mcp`, and the new `toml_edit` dependency.

## Task 1: `ToolEndpoint::Http` + `name`/`env` on `Stdio` (`crates/savvagent-host/src/config.rs`)

**Files:**
- Modify: `crates/savvagent-host/src/config.rs`

- [x] Baseline: `cargo test --workspace --no-fail-fast` — confirm green before touching anything;
      record the total test count as the pre-change baseline for this plan's final comparison.
- [x] Add `name: String` and `env: HashMap<String, String>` fields to `ToolEndpoint::Stdio`; add a
      new `Http { name: String, url: String, auth: HttpAuth }` variant; define
      `pub enum HttpAuth { None, Bearer { token: String } }`. Mark both `ToolEndpoint` and `HttpAuth`
      `#[non_exhaustive]`. Add `use std::collections::HashMap;`.
- [x] Update every in-workspace `ToolEndpoint::Stdio { command, args }` construction site to the new
      shape (`crates/savvagent/src/main.rs`'s `ToolBins::apply`, passing `name` as the bundled tool's
      canonical name — `"fs"`/`"bash"`/`"grep"`/`"lsp"`/`"web"` — and `env: HashMap::new()`; any
      test-only constructions in `crates/savvagent-host/src/session.rs`/`tools.rs` tests). Do not
      touch `ToolRegistry::connect`'s body yet (Task 2) beyond what's needed to keep it compiling
      against the new field shape (pattern match arms will need `name`/`env` bound, even if unused
      until Task 2).
- [x] Add/extend unit tests in `config.rs`: `ToolEndpoint::Http` constructs with each `HttpAuth`
      variant; a `#[non_exhaustive]`-enforcement compile-time check is unnecessary (rustc enforces it
      automatically for out-of-crate code) — instead add a doc-comment-level note only.
- [x] Run `cargo check -p savvagent-host -p savvagent --all-targets` — fix any compile errors from
      the field/variant addition (expected in `tools.rs`'s match arm and `main.rs`'s `ToolBins::apply`
      and any test fixtures).
- [x] Run `cargo test -p savvagent-host` — confirm green.
- [x] Public-interface check: this is the **breaking `ToolEndpoint` change** identified in the spec.
      Record in the PR body: `ToolEndpoint::Stdio` gained `name`/`env` fields (breaking for any
      external struct-literal construction), a new `Http` variant was added, and `ToolEndpoint`/
      `HttpAuth` are now `#[non_exhaustive]` (breaking for any external exhaustive `match`). Note the
      required MINOR version bump per this repo's pre-1.0 convention.
- [x] Commit: `feat(host): add ToolEndpoint::Http and name/env fields on Stdio`.

## Task 2: `ToolRegistry::connect` rework — failure isolation, status record, two-stage timeout, `Http` arm

**Files:**
- Modify: `crates/savvagent-host/src/tools.rs`
- Modify: `crates/savvagent-host/src/session.rs`

- [x] Add `pub enum TransportKind { Stdio, Http }`, `pub enum ConnectState { Connected, Failed {
      reason: String } }`, `pub struct ToolServerStatus { pub name: String, pub transport:
      TransportKind, pub state: ConnectState }` to `tools.rs`.
- [x] Add a `statuses: Vec<ToolServerStatus>` field to `ToolRegistry`; populate it during `connect()`
      for every endpoint (success and failure); add `pub fn statuses(&self) -> &[ToolServerStatus]`.
- [x] **Timeout plumbing (blocking fix from plan-review round 1):** `ToolRegistry::connect`'s current
      signature (`endpoints: &[ToolEndpoint], project_root: &Path, sandbox: &SandboxConfig,
      bash_net_resolver: BashNetResolverHandle, resource_tx: mpsc::Sender<ResourceEvent>`) has no
      timeout parameter. Add `connect_timeout: std::time::Duration` as a new parameter. Update all
      three call sites in `crates/savvagent-host/src/session.rs` (`Host::start` ~line 577,
      `Host::with_components` ~line 682, and the test-only construction ~line 3548) to pass
      `Duration::from_millis(config.connect_timeout_ms)` (the field already exists on `HostConfig`,
      currently used only for provider auto-connect — this reuses it for tool connect too, per spec
      §2, rather than inventing a second timeout constant). The test call site (~3548) should pass
      whatever duration that specific test needs (short, if it's exercising timeout behavior; the
      existing default otherwise).
- [x] Rework `connect()`'s per-endpoint loop: replace the `?`-propagating `anyhow::bail!`/`with_context`
      chain for the non-bash `Stdio` arm with the two-stage `tokio::time::timeout_at` shape from the
      spec's §2 (bind `service` in an outer scope as soon as `serve()` returns; timeout/error before
      that point records `ConnectState::Failed` with nothing to cancel; timeout/error on
      `list_all_tools()` after that point calls `service.cancel().await` before recording `Failed`).
      **Apply `env` to the spawned command (blocking fix from plan-review round 2 — Task 1 added the
      `env: HashMap<String, String>` field to `ToolEndpoint::Stdio`, but connect()'s non-bash arm
      never reads it today):** after building `cmd` via `tokio::process::Command::new(command)` and
      `cmd.args(args)`, call `cmd.envs(&env)` — merging over (not replacing) the three
      `SAVVAGENT_TOOL_*_ROOT` vars and the process's inherited environment, exactly as the spec's §1
      describes. Add a regression test asserting a configured `env` entry (e.g. a literal
      `MY_VAR=value`) actually reaches a spawned stdio server (a minimal stub server that echoes its
      environment back as a tool result, or reads `std::env::var` and asserts inside the test process
      if the stub is in-process — match whatever stub-server pattern Task 2's other new tests already
      establish).
      Apply the identical two-stage shape to the bash-probe `Stdio` arm (still probe-spawn-and-cancel
      on success, but now also two-stage-timeout-bounded and status-recorded). **The bash arm's `env`
      must also reach every later lazy respawn, not just the probe (blocking fix from plan-review
      round 2):** `BashSpawnConfig` (used by `LazyBash` to respawn `tool-bash` per-call) currently
      persists only `command`/`args`/`project_root`/`sandbox_template` — add an `env:
      HashMap<String, String>` field, populate it from the `Stdio` endpoint's `env` at `connect()`
      time (alongside the other `BashSpawnConfig` fields already captured there), and thread it
      through `build_bash_command`'s signature (`crates/savvagent-host/src/tools.rs:883`) with a
      `cmd.envs(&env)` call matching the non-bash arm's treatment — both the probe spawn and every
      later lazy respawn (`call_with_bash_net_override`'s respawn path) share this one code path, so a
      single change to `build_bash_command` covers both. Add a regression test asserting a configured
      `env` entry reaches a lazy-respawned bash child (not just the probe), since the probe and the
      real spawn are separate `build_bash_command` invocations.
      A genuine per-endpoint failure no longer aborts the whole `connect()` call — record the status
      and `continue` to the next endpoint. Endpoint-registration failures that indicate a
      caller-must-know-before-anything-runs condition are not introduced by this change (none
      identified in the spec) — the only remaining hard error path is truly unexpected internal state
      (e.g. an unreachable branch), not endpoint reachability.
- [x] Add the `ToolEndpoint::Http` construction arm exactly per spec §2: build
      `StreamableHttpClientTransport::from_config(StreamableHttpClientTransportConfig::with_uri(url))`
      with `.auth_header(token)` applied when `auth` is `HttpAuth::Bearer { token }` (confirmed API:
      `StreamableHttpClientTransportConfig::auth_header<T: Into<String>>(self, value: T) -> Self`,
      rmcp 1.6, takes the token without a `"Bearer "` prefix — rmcp adds the scheme). No sandbox
      wrapping applies. Same two-stage timeout/cancel shape as the stdio arm.
- [x] Per-tool (not per-endpoint) collision handling: after a successful `list_all_tools()` on any
      arm, register each tool name into `routes` individually; a name already present is skipped
      (not overwritten), logged via `tracing::warn!`, and does **not** mark the endpoint `Failed` —
      it stays `Connected` with the other tools registered. `ConnectState::Failed` is reserved for
      "the endpoint itself never came up."
- [x] `ToolServer.label` is now populated from the endpoint's `name` field (not
      `command.display()`/`url.clone()`).
- [x] Add `pub fn tool_server_statuses(&self) -> Vec<ToolServerStatus>` to `Host`
      (`session.rs`), cloning from the registry (acquire the `tools: Mutex<Option<Arc<ToolRegistry>>>`
      lock briefly, clone `ToolServerStatus` — it's a small `Clone`-able struct — release before
      returning).
- [x] Write regression tests in `tools.rs`'s test module: (a) an endpoint whose command doesn't exist
      records `ConnectState::Failed` and does not abort `connect()` for the remaining endpoints
      (construct with ≥2 endpoints, one valid one bogus, assert both `Connected`/`Failed` co-exist in
      `statuses()`); (b) a tool-name collision between two endpoints leaves both `Connected` with a
      warning logged, not one marked `Failed`; (c) a stub MCP server that completes `initialize` but
      never replies to `tools/list` — `connect()` returns within the configured timeout and the
      server's child process is confirmed no longer running afterward (use a short
      `connect_timeout_ms` in the test's `HostConfig`/direct `ToolRegistry::connect` call, and a
      minimal test-only stub binary or an in-process fake transport, whichever the existing test
      infrastructure in this file already supports — check for existing stub-server test helpers in
      `tools.rs`/`session.rs` before writing a new one from scratch).
- [x] Bearer-token non-logging test: assert that constructing and using an `HttpAuth::Bearer` token
      through `connect()` never causes the literal token value to appear in any `tracing` output
      captured during the call (use a `tracing` test subscriber or `tracing_test`/`tracing-subscriber`
      capture pattern already used elsewhere in this workspace, if any — check first).
- [x] Run `cargo test -p savvagent-host` — confirm green, including the new tests.
- [x] Public-interface check: `ToolRegistry::connect`'s behavior contract changes ("any endpoint
      failure aborts `Host::start`" → "isolated and recorded"); `Host` gains
      `tool_server_statuses()`. Record both in the PR body per spec's Public-interface-changes
      section.
- [x] Commit: `feat(host): isolate per-endpoint tool connect failures, add HTTP transport and status tracking`.

## Task 3: Tolerant `mcp_servers` config loading (`crates/savvagent/src/config_file.rs`)

**Files:**
- Modify: `crates/savvagent/src/config_file.rs`

- [x] Add `McpServerEntry` (tagged enum, `#[serde(tag = "transport", rename_all = "lowercase")]`,
      `Stdio { name, command, args, env }` / `Http { name, url, auth }`) and `McpAuthMode` (`None`
      default / `Bearer` / `Oauth`) exactly per spec §3. Add `mcp_servers: Vec<McpServerEntry>`
      (`#[serde(default)]`) to `ConfigFile`, keeping the existing `#[serde(default)]` on
      `startup`/`migration`.
- [x] Add `pub struct LoadedConfig { pub config: ConfigFile, pub mcp_server_diagnostics: Vec<String>
      }`.
- [x] Rewrite `ConfigFile::load_or_default(path: &Path) -> LoadedConfig`: parse file contents to
      `toml::Value` first (fall back to `ConfigFile::default()` + empty diagnostics on a syntactically
      broken file, exactly as today's whole-file fallback does); deserialize `startup`/`migration`
      from that `Value` (with `mcp_servers` treated as opaque during this step — do not fail the whole
      document if `mcp_servers` entries are malformed); extract the raw `mcp_servers` array as
      `Vec<toml::Value>` and attempt `McpServerEntry::deserialize` on each element independently,
      collecting a diagnostic string (including which row, e.g. by index or `name` if present) for
      each failure and skipping it, appending successes to `config.mcp_servers`.
- [x] Add `McpServerEntry::validate(&self, seen_names: &HashSet<String>) -> Result<(), String>` per
      spec's Validation subsection: name non-empty, unique (case-sensitive) across `mcp_servers`, no
      `:` character in `name` (keyring namespace separator), `auth != Oauth` (not yet supported,
      rejected with a clear message), at most one `env` value equal to the literal string `"keyring"`
      for `Stdio` entries (naming both variables in the error if more than one).
- [x] Add unit tests: (a) a `config.toml` with one valid and one malformed `[[mcp_servers]]` row still
      loads `startup`/`migration` correctly and returns one diagnostic + one successfully-parsed
      entry; (b) `auth = "oauth"` is accepted at the tolerant-decode step (it's valid TOML shape) but
      rejected by `validate()` with a clear "not yet supported" message; (c) duplicate `name`s across
      two entries — the second is caught by `validate()`; (d) an entry with two `"keyring"`-marked
      env vars is rejected by `validate()` naming both; (e) existing `round_trip_preserves_fields`/
      `invalid_policy_string_falls_back_to_default`/`missing_file_returns_default` tests updated for
      the new `LoadedConfig` return shape (should still pass conceptually unchanged, just accessed via
      `.config`).
- [x] **Update every caller of `ConfigFile::load_or_default` (blocking fix from plan-review round 1
      — the full caller list, not just `main.rs`):**
      `crates/savvagent/src/main.rs:260` (`bootstrap_app_and_host`) — destructure `LoadedConfig`,
      threading `mcp_server_diagnostics` into the startup-notes path (this is Task 6's job; here just
      make it compile by taking `.config` and stashing diagnostics in a local the Task 6 change will
      consume).
      `crates/savvagent/src/egui_app/mod.rs:597` — same treatment as `main.rs` (destructure, forward
      diagnostics wherever that path surfaces startup notes to the GUI).
      `crates/savvagent/src/plugin/builtin/migration_picker/mod.rs:53,91,206` — three call sites; this
      plugin both reads and rewrites `config.toml` for provider migration. Decide explicitly: (a) each
      read site takes `.config` and ignores `mcp_server_diagnostics` (acceptable since migration_picker
      only touches `startup`/`migration`, never `mcp_servers`), and (b) any site that calls
      `cfg.save(path)` afterward must confirm `ConfigFile::save`'s reserialization does not corrupt or
      drop `mcp_servers` entries that came from the tolerant decode — since `save` still round-trips
      through the typed `Vec<McpServerEntry>` (only `/mcp`'s dedicated writer in Task 4 avoids that),
      a migration_picker write will silently drop any row that failed tolerant parsing. Document this
      as an accepted limitation in the PR body (migration_picker's writes are rare, one-time, opt-in
      first-launch flows, not the routine add/remove path Task 4's regression test protects) — do not
      attempt to route migration_picker's writes through Task 4's writer, since that would entangle two
      unrelated features; simply confirm in a test that migration_picker's save path still preserves
      all typed fields it's actually responsible for.
      This is compile-error-driven, so run `cargo check -p savvagent --all-targets` after the signature
      change and confirm no other call sites were missed.
- [x] Run `cargo test -p savvagent`. Confirm green.
- [x] Public-interface check: `ConfigFile::load_or_default`'s return-type change is internal-only per
      spec (no external callers; `crates/savvagent` is the TUI binary crate) — note this explicitly in
      the PR body so reviewers don't flag it as a Rule-6 violation needing a version bump.
- [x] Commit: `feat(config): add tolerant mcp_servers loading to ConfigFile`.

## Task 4: `toml_edit`-based write path (`crates/savvagent/src/mcp_config_writer.rs`)

**Files:**
- Create: `crates/savvagent/src/mcp_config_writer.rs`
- Modify: `crates/savvagent/Cargo.toml`, root `Cargo.toml`
- Modify: `crates/savvagent/src/main.rs` (module registration only, `mod mcp_config_writer;`)

- [x] Add `toml_edit = "0.25"` to root `Cargo.toml`'s `[workspace.dependencies]`; add
      `toml_edit.workspace = true` to `crates/savvagent/Cargo.toml`. Run `cargo check -p savvagent` to
      confirm the dependency resolves.
- [x] Implement `mcp_config_writer.rs` with two public functions operating on a `config.toml` path:
      `pub fn add_server(path: &Path, entry_toml: toml_edit::Table) -> std::io::Result<()>` and
      `pub fn remove_server(path: &Path, name: &str) -> std::io::Result<bool>` (returns whether a
      matching row was found and removed). Both: read the file fresh via `std::fs::read_to_string`
      (create an empty document if the file doesn't exist yet, matching `ConfigFile::save`'s
      `create_dir_all` behavior for the parent dir), parse into a `toml_edit::DocumentMut`, locate (or
      create, for `add_server`, if absent) the top-level `mcp_servers` array-of-tables via
      `as_array_of_tables_mut`, mutate only that node (`push` for add; find-by-`name`-key-and-remove
      for remove — do not touch any other node), write back via `document.to_string()`.
- [x] Add a helper to build a `toml_edit::Table` from a validated `McpServerEntry`-shaped input (the
      `/mcp` add form's fields), setting `transport`/`name`/`command`/`args`/`env` or
      `transport`/`name`/`url`/`auth` keys directly as `toml_edit::Item`s (not via serde — this module
      never round-trips through `McpServerEntry`, exactly per spec).
- [x] Regression test (the one explicitly required by spec §3): write a `config.toml` with one valid
      `[[mcp_servers]]` row, one malformed row (unrecognized `transport` value) preceded by a
      hand-written comment, call `add_server` to append a third row, then read the raw file text back
      and assert the malformed row's original text *and* its preceding comment are byte-for-byte
      unchanged, and the new row is present. A second test does the same for `remove_server`,
      removing the valid row by name and confirming the malformed row + comment survive untouched.
- [x] Test: `remove_server` on a name that doesn't exist returns `Ok(false)` without modifying the
      file (idempotent no-op, matching `creds::mcp_delete`'s idempotency).
- [x] Run `cargo test -p savvagent mcp_config_writer`. Confirm green.
- [x] Commit: `feat(config): add toml_edit-based mcp_servers write path`.

## Task 5: Keyring additions (`crates/savvagent/src/creds.rs`)

**Files:**
- Modify: `crates/savvagent/src/creds.rs`

- [x] Add `const MCP_PREFIX: &str = "mcp:";`, `pub fn delete(account: &str) -> Result<(),
      keyring::Error>` (idempotent: `NoEntry`/`NoStorageAccess` treated as success, mirroring `load`'s
      existing error-mapping style), `pub fn mcp_save`, `pub fn mcp_load`, `pub fn mcp_delete` exactly
      per spec §5 (thin wrappers namespacing the account as `format!("{MCP_PREFIX}{server_name}")`).
- [x] Add unit tests: `delete` on a nonexistent account returns `Ok(())`; `mcp_save`/`mcp_load`
      round-trip a secret under the namespaced account (skip/ignore gracefully in CI environments with
      no keyring backend, matching however existing `creds.rs`/`save`/`load` tests already handle
      backend-unavailable environments — check for a `#[cfg(...)]` guard or graceful-skip pattern used
      today before writing new tests from scratch).
- [x] Run `cargo test -p savvagent creds`. Confirm green.
- [x] Commit: `feat(creds): add delete and mcp_* keyring helpers`.

## Task 6: Bootstrap wiring (`crates/savvagent/src/main.rs`)

**Files:**
- Modify: `crates/savvagent/src/main.rs`

- [x] In `bootstrap_app_and_host`, destructure the new `LoadedConfig` from `ConfigFile::load_or_default`
      and push a startup note for each `mcp_server_diagnostics` entry (via whatever the existing
      startup-notes plumbing is — `deferred_notes`/`HostBoot::startup_notes`, per the code already
      read in `bootstrap_pool_host`).
- [x] Add `pub(crate) struct McpManagerSeed { pub configured: Vec<McpServerSummary>, pub skip_notes:
      Vec<(String, String)> }` and `pub(crate) struct McpServerSummary { pub name: String, pub
      transport: &'static str }` (plain, `Clone + Send` — no secrets, just enough for the `/mcp`
      screen's initial listing before Task 7 merges in live connect status). `skip_notes` is `(name,
      reason)` pairs for entries that failed `validate()` or had an unreadable keyring secret.
- [x] In `bootstrap_pool_host`, after `tool_bins.apply(...)` builds `config` and before
      `Host::start(config).await`, add a step that: iterates `config_file.mcp_servers`, calls
      `McpServerEntry::validate` against a running `seen_names` set, resolves secrets via
      `creds::mcp_load` for entries needing one (`Stdio` with an `env` value marked `"keyring"`, or
      `Http` with `auth = Bearer`), and appends the resulting `ToolEndpoint::Stdio`/`Http` to
      `config.tools` via repeated `config = config.with_tool(...)` (or direct `config.tools.push(...)`
      — check which is more idiomatic given `HostConfig::with_tool` takes `self` by value and `config`
      is already `mut` at this point). A server that fails validation or whose keyring secret is
      missing/unreadable is skipped with both a `deferred_notes.push(...)` note (not fatal) and a
      `skip_notes` entry on a `McpManagerSeed` being built alongside `config`. Every entry — validated
      and skipped alike — contributes a `McpServerSummary` to `McpManagerSeed::configured`.
- [x] Add `mcp_manager_seed: McpManagerSeed` to `HostBoot` (`main.rs:217`), populated in
      `bootstrap_pool_host` right after `Host::start(config).await` succeeds (same function, same
      scope — no cross-boundary plumbing needed since `bootstrap_pool_host` already has both
      `config_file` and the started `host` in hand at that point).
- [x] Run `cargo check -p savvagent --all-targets`, fix any remaining call-site fallout from the
      `LoadedConfig`/`HostBoot` shape changes (e.g. `start_host_remote`'s legacy path, if it also
      calls `load_or_default` or receives `config_file` — confirm from the read code whether it needs
      the same treatment; note it currently returns a bare `Arc<Host>`, not a `HostBoot`, so it may
      need no `McpManagerSeed` at all if the legacy remote-provider debug path is out of scope for
      `mcp_servers` — confirm against the spec, which doesn't call this out either way, and default to
      an empty `McpManagerSeed` for that path if so).
- [x] Add an integration-style test (in `main.rs`'s test module or a new
      `tests/mcp_bootstrap.rs`, whichever matches this crate's existing test-location convention —
      check first) that builds a `ConfigFile` with one valid `[[mcp_servers]]` stdio entry pointing at
      a trivial test MCP server binary/script, runs the bootstrap path, and asserts the resulting
      `HostConfig.tools`/`Host::tool_server_statuses()`/`HostBoot::mcp_manager_seed` all include it.
      If spinning up a real trivial MCP server for this test is impractical without infra not already
      present, scope this down to a unit test of just the validate-and-resolve step (not the full
      `Host::start`), and note in the PR body why the fuller test was scoped down.
- [x] Run `cargo test -p savvagent`. Confirm green.
- [ ] Commit: `feat: wire configured mcp_servers into host bootstrap`.

## Task 7: `/mcp` slash command + screen (`crates/savvagent/src/plugin/builtin/mcp/`)

**Files:**
- Create: `crates/savvagent/src/plugin/builtin/mcp/mod.rs`,
  `crates/savvagent/src/plugin/builtin/mcp/screen.rs`
- Modify: `crates/savvagent/src/plugin/builtin/mod.rs`, `crates/savvagent/src/plugin/mod.rs`,
  `crates/savvagent/src/plugin/external.rs` (`register_builtins_with_external`'s parameter list),
  `crates/savvagent/src/main.rs` (`build_app_with_host`'s call into `register_builtins_with_external`)

- [x] Read `crates/savvagent/src/plugin/builtin/connect/mod.rs` and `screen.rs` in full immediately
      before starting this task (already read once during spec drafting — re-read for exact ABI
      shape: `Manifest`/`Contributions`/`SlashSpec`/`ScreenSpec`/`ScreenLayout`/`Effect::OpenScreen`/
      `create_screen`).
- [x] **Plugin-ABI status bridge (blocking fix from plan-review round 2 — replaces the round-1 draft's
      `HostEvent`-based design, which round 2 correctly flagged as (a) requiring matching changes to
      `crates/savvagent-plugin-wit/wit/shared.wit`'s `hook-kind` variant, both directions of
      `crates/savvagent-plugin-wasm/src/convert.rs`'s exhaustive `HookKind` conversions, and both
      `HostEvent`-projection match arms in `crates/savvagent-plugin-wasm/src/adapter/{interactive,
      static_}.rs` — a much larger surface than this feature needs — and (b) not actually
      dispatchable from `bootstrap_pool_host`, which runs before `App`/the plugin registry exist and
      before `app.install_plugin_runtime` (`main.rs:391`) — there is no `dispatch_host_event` call
      possible at the point the round-1 draft named):**
      Since `McpPlugin` only needs a **one-shot initial seed** (v1 has no live reconnect — see Scope),
      and `register_builtins`/`register_builtins_with_external` are already called *after*
      `Host::start` has completed (confirmed: `build_app_with_host`, `main.rs:317`, calls
      `register_builtins_with_external` well after `bootstrap_pool_host` returned a `HostBoot` with an
      already-started `host`), the simplest correct bridge is **constructor-argument seeding**,
      exactly like `UserSlashCommandsPlugin::new(trust_levels)` already does — no new `HostEvent`,
      no `HookKind` variant, no WIT/WASM-adapter changes, and no wire-format/plugin-ABI surface change
      at all (this bridge is entirely internal to `crates/savvagent`, never crossing into
      `savvagent-plugin`'s portable ABI):
      1. Task 6 already adds `McpManagerSeed { configured: Vec<McpServerSummary>, skip_notes:
         Vec<(String, String)> }` to `HostBoot`, populated in `bootstrap_pool_host` right after
         `Host::start` succeeds (same function scope has both `config_file` and the started `host`).
      2. In `build_app_with_host` (`main.rs:269`), read `initial.as_ref().map(|b|
         b.mcp_manager_seed.clone()).unwrap_or_default()` (mirroring how `header_model`/
         `startup_notes` are already destructured from `initial` at the top of the function) and pass
         it as a new parameter to `register_builtins_with_external` →
         `register_builtins` (`crates/savvagent/src/plugin/external.rs:70`,
         `crates/savvagent/src/plugin/mod.rs:86`), which constructs `McpPlugin::new(seed)` instead of
         `McpPlugin::new()`.
      3. Additionally read live connect status once at construction: `McpPlugin::new` (or a
         `with_statuses` builder called right before construction in `build_app_with_host`, using
         `current_host(&host_slot).await` — already available at that point since `host_slot` is
         built earlier in the same function) takes `Vec<ToolServerStatus>` (the `savvagent-host`-side
         type from Task 2 — reachable here because `crates/savvagent` **can** depend on
         `savvagent-host`, unlike `savvagent-plugin`) alongside the `McpManagerSeed`, converts each to
         a plain in-plugin display record, and stores both in `McpPlugin`'s fields for
         `create_screen` to read — the same "stash what the screen needs at plugin-construction time"
         shape `ConnectPlugin` uses for `candidates`, just seeded once up front instead of
         accumulated via `on_event`.
      4. **Combine with configured-but-not-yet-connected servers (advisory note from plan-review
         round 1, addressed here):** a server that failed `McpServerEntry::validate` or had an
         unreadable keyring secret in Task 6 never became a `ToolEndpoint` at all, so it will never
         appear in `Host::tool_server_statuses()` — it only appears in `McpManagerSeed::skip_notes`.
         `McpManagerScreen` must render the full configured list from `McpManagerSeed::configured`
         merged with the live status list by name — entries present in `configured` but absent from
         the status list are rendered as a distinct "not started: <reason>" row (using the matching
         `skip_notes` entry), never conflated with `ConnectState::Failed` (which means "we tried to
         connect and it failed," a different condition from "we never tried because
         validation/secret-resolution failed first").
- [x] `mod.rs`: `McpPlugin` registers slash command `"mcp"` (no args → open `mcp.manager` screen,
      matching `connect`'s no-arg behavior) and screen id `"mcp.manager"`. `create_screen` builds
      `McpManagerScreen` from the seeded `McpManagerSeed` + status list, per the bridge design above.
- [x] `screen.rs`: `McpManagerScreen` lists configured servers (name, transport, status) — reuse
      whatever list/table rendering helper `connect/screen.rs`'s `ConnectPickerScreen` or another
      existing picker screen already provides, rather than writing new ratatui rendering from scratch.
      Key handling: `a` opens an add sub-flow (name / transport / command+args or url / optional
      secret via a masked input matching `/connect`'s `InputMode::EnteringApiKey` pattern in
      `main.rs`), calling `mcp_config_writer::add_server` + `creds::mcp_save` (secret) on confirm.
      `d`/`Delete` removes the selected server: **config-then-credential ordering** — call
      `mcp_config_writer::remove_server` first, then `creds::mcp_delete` only if that succeeds; push a
      "restart savvagent to apply changes" note either way once the config write succeeds, and report
      "server removed; stale credential could not be deleted" if the keyring delete step fails after a
      successful config removal. `r` re-reads status only (no live reconnect).
- [x] Register `McpPlugin` in `crates/savvagent/src/plugin/builtin/mod.rs` (`pub mod mcp;` with a doc
      comment) and in `crates/savvagent/src/plugin/mod.rs`'s `register_builtins` plugin vec; update
      `register_builtins_pr8_complete` (and any other test asserting the builtin id list/count) to
      include `"internal:mcp"` (or whatever id convention this plugin uses — match `connect`'s
      `"internal:connect"` pattern only if `/mcp` is meant to be a Core, non-disableable plugin;
      otherwise follow the Optional-plugin registration pattern used by e.g. `lsp_installer` — decide
      based on whether the spec implies core-vs-optional, defaulting to Core since `/connect` is
      Core and `/mcp` is its direct analog).
- [x] Add plugin-level unit tests mirroring `connect/mod.rs`'s test module shape: manifest exposes the
      `"mcp"` slash + `"mcp.manager"` screen; `handle_slash` with no args returns
      `Effect::OpenScreen`; screen key handling for add/remove (mock/stub the config-writer and
      keyring calls behind whatever seam is idiomatic here — check how other builtins with
      side-effecting slash handlers are tested, e.g. `save`/`route`, for the mocking convention before
      inventing a new one).
- [x] Run `cargo test -p savvagent plugin::builtin::mcp`. Confirm green.
- [ ] Commit: `feat: add /mcp slash command and manager screen`.

## Task 8: Docs (`README.md`, `PRD.md`, `CLAUDE.md`, `CHANGELOG.md`)

**Files:**
- Modify: `README.md`, `PRD.md`, `CLAUDE.md`, `CHANGELOG.md`

- [ ] `README.md`: add a "New MCP server (user-configured)" recipe next to the existing "New
      provider"/"New tool" recipes in the "Extending" section, documenting the `[[mcp_servers]]` TOML
      shape, the `/mcp` command, and the `mcp:<server>` keyring namespace. Add `/mcp` to whatever
      slash-command reference table/list already documents `/connect` etc.
- [ ] `PRD.md`: update the "3rd-party MCP servers" architecture-diagram entry (`PRD.md:117`) to note
      it's implemented as of this release, pointing at the spec file for the exact scope (stdio +
      Streamable HTTP only, no OAuth/SSE — link the spec's Scope section).
- [ ] `CLAUDE.md`: update the "`/connect` is the only writer" keyring invariant sentence under
      Persistence to "two writers, namespaced (`/connect` for provider keys, `/mcp` for
      `mcp:<server>` secrets)".
- [ ] `CHANGELOG.md`: add `### Added` (Http tool transport, `/mcp`, `mcp_servers` config,
      `toml_edit` dependency) and `### Changed`/`### Breaking` (per this repo's existing changelog
      section conventions — check the most recent `[Unreleased]` or prior release heading for the
      exact section names used) entries under `[Unreleased]`.
- [ ] Validate: run `cargo test --doc -p savvagent-host -p savvagent` (catches broken doc-comment
      code fences/links touched by this task, since `config.rs`/`session.rs` doc comments referenced
      by README/PRD prose were touched in earlier tasks) and grep the modified docs for any now-stale
      cross-references (e.g. confirm no remaining "`/connect` is the only writer" phrasing survives
      outside the intended `CLAUDE.md` edit). This is the doc-equivalent of every other task's
      pre-commit test run — there is no repo-specific markdown linter to invoke, so this substitutes
      a targeted check for the kind of breakage doc edits can actually cause here (dead links, stale
      invariant text, broken doc-tests).
- [ ] Commit: `docs: document user-configured MCP servers`.

## Task 9: Final verification, PR, release

- [ ] Run `cargo test --workspace --no-fail-fast` and `cargo clippy --workspace --all-targets
      -- -D warnings` (or whatever clippy invocation `bacon clippy-all` maps to) — confirm zero
      failures/warnings and compare the total test count against Task 1's baseline (expect strictly
      more, never fewer, given only additive tests were written).
- [ ] Manual smoke check (per this repo's convention of a quick manual pass beyond automated tests):
      add a `[[mcp_servers]]` stdio entry pointing at a trivial local script/binary that speaks MCP
      (or reuse one of the bundled `tool-*` binaries pointed at via `mcp_servers` instead of
      `ToolBins`, as a stand-in) to a scratch `~/.savvagent/config.toml`-equivalent test path, run
      `cargo run -p savvagent`, confirm `/mcp` lists it as Connected, and that its tools are callable.
- [ ] Open the PR referencing `savvagent/savvagent-cli#36`, following this repo's PR-description
      conventions (summary, spec/plan links, breaking-change callout, test-plan section). Run the
      mandatory review trio (Rust-expert + architecture via `general-purpose`, security via
      `security-review`) and the review-response subagent loop per the skill's Phase 4 rules. Merge
      once reviews pass.
- [ ] **Release cut (explicit committed step, not a bare post-merge action):** in a separate release
      worktree/branch per `RELEASING.md`'s documented procedure, bump the version to the next
      **MINOR** (confirming the exact number against `CHANGELOG.md`'s most recent released heading at
      this point in time, not hardcoded earlier in this plan), move the `[Unreleased]` CHANGELOG
      entries under the new version heading, run whatever verification `RELEASING.md` prescribes
      (build/test/artifact checks), commit the version bump + CHANGELOG move, tag, and open/merge the
      release PR per that document's process.
- [ ] File a follow-up issue for OAuth 2.1 + PKCE + dynamic client registration support, referencing
      this spec's Scope section for what v1 explicitly deferred.

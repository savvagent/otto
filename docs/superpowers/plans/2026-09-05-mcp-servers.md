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

- [ ] Baseline: `cargo test --workspace --no-fail-fast` — confirm green before touching anything;
      record the total test count as the pre-change baseline for this plan's final comparison.
- [ ] Add `name: String` and `env: HashMap<String, String>` fields to `ToolEndpoint::Stdio`; add a
      new `Http { name: String, url: String, auth: HttpAuth }` variant; define
      `pub enum HttpAuth { None, Bearer { token: String } }`. Mark both `ToolEndpoint` and `HttpAuth`
      `#[non_exhaustive]`. Add `use std::collections::HashMap;`.
- [ ] Update every in-workspace `ToolEndpoint::Stdio { command, args }` construction site to the new
      shape (`crates/savvagent/src/main.rs`'s `ToolBins::apply`, passing `name` as the bundled tool's
      canonical name — `"fs"`/`"bash"`/`"grep"`/`"lsp"`/`"web"` — and `env: HashMap::new()`; any
      test-only constructions in `crates/savvagent-host/src/session.rs`/`tools.rs` tests). Do not
      touch `ToolRegistry::connect`'s body yet (Task 2) beyond what's needed to keep it compiling
      against the new field shape (pattern match arms will need `name`/`env` bound, even if unused
      until Task 2).
- [ ] Add/extend unit tests in `config.rs`: `ToolEndpoint::Http` constructs with each `HttpAuth`
      variant; a `#[non_exhaustive]`-enforcement compile-time check is unnecessary (rustc enforces it
      automatically for out-of-crate code) — instead add a doc-comment-level note only.
- [ ] Run `cargo check -p savvagent-host -p savvagent --all-targets` — fix any compile errors from
      the field/variant addition (expected in `tools.rs`'s match arm and `main.rs`'s `ToolBins::apply`
      and any test fixtures).
- [ ] Run `cargo test -p savvagent-host` — confirm green.
- [ ] Public-interface check: this is the **breaking `ToolEndpoint` change** identified in the spec.
      Record in the PR body: `ToolEndpoint::Stdio` gained `name`/`env` fields (breaking for any
      external struct-literal construction), a new `Http` variant was added, and `ToolEndpoint`/
      `HttpAuth` are now `#[non_exhaustive]` (breaking for any external exhaustive `match`). Note the
      required MINOR version bump per this repo's pre-1.0 convention.
- [ ] Commit: `feat(host): add ToolEndpoint::Http and name/env fields on Stdio`.

## Task 2: `ToolRegistry::connect` rework — failure isolation, status record, two-stage timeout, `Http` arm

**Files:**
- Modify: `crates/savvagent-host/src/tools.rs`
- Modify: `crates/savvagent-host/src/session.rs`

- [ ] Add `pub enum TransportKind { Stdio, Http }`, `pub enum ConnectState { Connected, Failed {
      reason: String } }`, `pub struct ToolServerStatus { pub name: String, pub transport:
      TransportKind, pub state: ConnectState }` to `tools.rs`.
- [ ] Add a `statuses: Vec<ToolServerStatus>` field to `ToolRegistry`; populate it during `connect()`
      for every endpoint (success and failure); add `pub fn statuses(&self) -> &[ToolServerStatus]`.
- [ ] **Timeout plumbing (blocking fix from plan-review round 1):** `ToolRegistry::connect`'s current
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
- [ ] Rework `connect()`'s per-endpoint loop: replace the `?`-propagating `anyhow::bail!`/`with_context`
      chain for the non-bash `Stdio` arm with the two-stage `tokio::time::timeout_at` shape from the
      spec's §2 (bind `service` in an outer scope as soon as `serve()` returns; timeout/error before
      that point records `ConnectState::Failed` with nothing to cancel; timeout/error on
      `list_all_tools()` after that point calls `service.cancel().await` before recording `Failed`).
      Apply the identical two-stage shape to the bash-probe `Stdio` arm (still probe-spawn-and-cancel
      on success, but now also two-stage-timeout-bounded and status-recorded).
      A genuine per-endpoint failure no longer aborts the whole `connect()` call — record the status
      and `continue` to the next endpoint. Endpoint-registration failures that indicate a
      caller-must-know-before-anything-runs condition are not introduced by this change (none
      identified in the spec) — the only remaining hard error path is truly unexpected internal state
      (e.g. an unreachable branch), not endpoint reachability.
- [ ] Add the `ToolEndpoint::Http` construction arm exactly per spec §2: build
      `StreamableHttpClientTransport::from_config(StreamableHttpClientTransportConfig::with_uri(url))`
      with `.auth_header(token)` applied when `auth` is `HttpAuth::Bearer { token }` (confirmed API:
      `StreamableHttpClientTransportConfig::auth_header<T: Into<String>>(self, value: T) -> Self`,
      rmcp 1.6, takes the token without a `"Bearer "` prefix — rmcp adds the scheme). No sandbox
      wrapping applies. Same two-stage timeout/cancel shape as the stdio arm.
- [ ] Per-tool (not per-endpoint) collision handling: after a successful `list_all_tools()` on any
      arm, register each tool name into `routes` individually; a name already present is skipped
      (not overwritten), logged via `tracing::warn!`, and does **not** mark the endpoint `Failed` —
      it stays `Connected` with the other tools registered. `ConnectState::Failed` is reserved for
      "the endpoint itself never came up."
- [ ] `ToolServer.label` is now populated from the endpoint's `name` field (not
      `command.display()`/`url.clone()`).
- [ ] Add `pub fn tool_server_statuses(&self) -> Vec<ToolServerStatus>` to `Host`
      (`session.rs`), cloning from the registry (acquire the `tools: Mutex<Option<Arc<ToolRegistry>>>`
      lock briefly, clone `ToolServerStatus` — it's a small `Clone`-able struct — release before
      returning).
- [ ] Write regression tests in `tools.rs`'s test module: (a) an endpoint whose command doesn't exist
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
- [ ] Bearer-token non-logging test: assert that constructing and using an `HttpAuth::Bearer` token
      through `connect()` never causes the literal token value to appear in any `tracing` output
      captured during the call (use a `tracing` test subscriber or `tracing_test`/`tracing-subscriber`
      capture pattern already used elsewhere in this workspace, if any — check first).
- [ ] Run `cargo test -p savvagent-host` — confirm green, including the new tests.
- [ ] Public-interface check: `ToolRegistry::connect`'s behavior contract changes ("any endpoint
      failure aborts `Host::start`" → "isolated and recorded"); `Host` gains
      `tool_server_statuses()`. Record both in the PR body per spec's Public-interface-changes
      section.
- [ ] Commit: `feat(host): isolate per-endpoint tool connect failures, add HTTP transport and status tracking`.

## Task 3: Tolerant `mcp_servers` config loading (`crates/savvagent/src/config_file.rs`)

**Files:**
- Modify: `crates/savvagent/src/config_file.rs`

- [ ] Add `McpServerEntry` (tagged enum, `#[serde(tag = "transport", rename_all = "lowercase")]`,
      `Stdio { name, command, args, env }` / `Http { name, url, auth }`) and `McpAuthMode` (`None`
      default / `Bearer` / `Oauth`) exactly per spec §3. Add `mcp_servers: Vec<McpServerEntry>`
      (`#[serde(default)]`) to `ConfigFile`, keeping the existing `#[serde(default)]` on
      `startup`/`migration`.
- [ ] Add `pub struct LoadedConfig { pub config: ConfigFile, pub mcp_server_diagnostics: Vec<String>
      }`.
- [ ] Rewrite `ConfigFile::load_or_default(path: &Path) -> LoadedConfig`: parse file contents to
      `toml::Value` first (fall back to `ConfigFile::default()` + empty diagnostics on a syntactically
      broken file, exactly as today's whole-file fallback does); deserialize `startup`/`migration`
      from that `Value` (with `mcp_servers` treated as opaque during this step — do not fail the whole
      document if `mcp_servers` entries are malformed); extract the raw `mcp_servers` array as
      `Vec<toml::Value>` and attempt `McpServerEntry::deserialize` on each element independently,
      collecting a diagnostic string (including which row, e.g. by index or `name` if present) for
      each failure and skipping it, appending successes to `config.mcp_servers`.
- [ ] Add `McpServerEntry::validate(&self, seen_names: &HashSet<String>) -> Result<(), String>` per
      spec's Validation subsection: name non-empty, unique (case-sensitive) across `mcp_servers`, no
      `:` character in `name` (keyring namespace separator), `auth != Oauth` (not yet supported,
      rejected with a clear message), at most one `env` value equal to the literal string `"keyring"`
      for `Stdio` entries (naming both variables in the error if more than one).
- [ ] Add unit tests: (a) a `config.toml` with one valid and one malformed `[[mcp_servers]]` row still
      loads `startup`/`migration` correctly and returns one diagnostic + one successfully-parsed
      entry; (b) `auth = "oauth"` is accepted at the tolerant-decode step (it's valid TOML shape) but
      rejected by `validate()` with a clear "not yet supported" message; (c) duplicate `name`s across
      two entries — the second is caught by `validate()`; (d) an entry with two `"keyring"`-marked
      env vars is rejected by `validate()` naming both; (e) existing `round_trip_preserves_fields`/
      `invalid_policy_string_falls_back_to_default`/`missing_file_returns_default` tests updated for
      the new `LoadedConfig` return shape (should still pass conceptually unchanged, just accessed via
      `.config`).
- [ ] **Update every caller of `ConfigFile::load_or_default` (blocking fix from plan-review round 1
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
- [ ] Run `cargo test -p savvagent`. Confirm green.
- [ ] Public-interface check: `ConfigFile::load_or_default`'s return-type change is internal-only per
      spec (no external callers; `crates/savvagent` is the TUI binary crate) — note this explicitly in
      the PR body so reviewers don't flag it as a Rule-6 violation needing a version bump.
- [ ] Commit: `feat(config): add tolerant mcp_servers loading to ConfigFile`.

## Task 4: `toml_edit`-based write path (`crates/savvagent/src/mcp_config_writer.rs`)

**Files:**
- Create: `crates/savvagent/src/mcp_config_writer.rs`
- Modify: `crates/savvagent/Cargo.toml`, root `Cargo.toml`
- Modify: `crates/savvagent/src/main.rs` (module registration only, `mod mcp_config_writer;`)

- [ ] Add `toml_edit = "0.25"` to root `Cargo.toml`'s `[workspace.dependencies]`; add
      `toml_edit.workspace = true` to `crates/savvagent/Cargo.toml`. Run `cargo check -p savvagent` to
      confirm the dependency resolves.
- [ ] Implement `mcp_config_writer.rs` with two public functions operating on a `config.toml` path:
      `pub fn add_server(path: &Path, entry_toml: toml_edit::Table) -> std::io::Result<()>` and
      `pub fn remove_server(path: &Path, name: &str) -> std::io::Result<bool>` (returns whether a
      matching row was found and removed). Both: read the file fresh via `std::fs::read_to_string`
      (create an empty document if the file doesn't exist yet, matching `ConfigFile::save`'s
      `create_dir_all` behavior for the parent dir), parse into a `toml_edit::DocumentMut`, locate (or
      create, for `add_server`, if absent) the top-level `mcp_servers` array-of-tables via
      `as_array_of_tables_mut`, mutate only that node (`push` for add; find-by-`name`-key-and-remove
      for remove — do not touch any other node), write back via `document.to_string()`.
- [ ] Add a helper to build a `toml_edit::Table` from a validated `McpServerEntry`-shaped input (the
      `/mcp` add form's fields), setting `transport`/`name`/`command`/`args`/`env` or
      `transport`/`name`/`url`/`auth` keys directly as `toml_edit::Item`s (not via serde — this module
      never round-trips through `McpServerEntry`, exactly per spec).
- [ ] Regression test (the one explicitly required by spec §3): write a `config.toml` with one valid
      `[[mcp_servers]]` row, one malformed row (unrecognized `transport` value) preceded by a
      hand-written comment, call `add_server` to append a third row, then read the raw file text back
      and assert the malformed row's original text *and* its preceding comment are byte-for-byte
      unchanged, and the new row is present. A second test does the same for `remove_server`,
      removing the valid row by name and confirming the malformed row + comment survive untouched.
- [ ] Test: `remove_server` on a name that doesn't exist returns `Ok(false)` without modifying the
      file (idempotent no-op, matching `creds::mcp_delete`'s idempotency).
- [ ] Run `cargo test -p savvagent mcp_config_writer`. Confirm green.
- [ ] Commit: `feat(config): add toml_edit-based mcp_servers write path`.

## Task 5: Keyring additions (`crates/savvagent/src/creds.rs`)

**Files:**
- Modify: `crates/savvagent/src/creds.rs`

- [ ] Add `const MCP_PREFIX: &str = "mcp:";`, `pub fn delete(account: &str) -> Result<(),
      keyring::Error>` (idempotent: `NoEntry`/`NoStorageAccess` treated as success, mirroring `load`'s
      existing error-mapping style), `pub fn mcp_save`, `pub fn mcp_load`, `pub fn mcp_delete` exactly
      per spec §5 (thin wrappers namespacing the account as `format!("{MCP_PREFIX}{server_name}")`).
- [ ] Add unit tests: `delete` on a nonexistent account returns `Ok(())`; `mcp_save`/`mcp_load`
      round-trip a secret under the namespaced account (skip/ignore gracefully in CI environments with
      no keyring backend, matching however existing `creds.rs`/`save`/`load` tests already handle
      backend-unavailable environments — check for a `#[cfg(...)]` guard or graceful-skip pattern used
      today before writing new tests from scratch).
- [ ] Run `cargo test -p savvagent creds`. Confirm green.
- [ ] Commit: `feat(creds): add delete and mcp_* keyring helpers`.

## Task 6: Bootstrap wiring (`crates/savvagent/src/main.rs`)

**Files:**
- Modify: `crates/savvagent/src/main.rs`

- [ ] In `bootstrap_app_and_host`, destructure the new `LoadedConfig` from `ConfigFile::load_or_default`
      and push a startup note for each `mcp_server_diagnostics` entry (via whatever the existing
      startup-notes plumbing is — `deferred_notes`/`HostBoot::startup_notes`, per the code already
      read in `bootstrap_pool_host`).
- [ ] In `bootstrap_pool_host`, after `tool_bins.apply(...)` builds `config` and before
      `Host::start(config).await`, add a step that: iterates `config_file.mcp_servers`, calls
      `McpServerEntry::validate` against a running `seen_names` set, resolves secrets via
      `creds::mcp_load` for entries needing one (`Stdio` with an `env` value marked `"keyring"`, or
      `Http` with `auth = Bearer`), and appends the resulting `ToolEndpoint::Stdio`/`Http` to
      `config.tools` via repeated `config = config.with_tool(...)` (or direct `config.tools.push(...)`
      — check which is more idiomatic given `HostConfig::with_tool` takes `self` by value and `config`
      is already `mut` at this point). A server that fails validation or whose keyring secret is
      missing/unreadable is skipped with a `deferred_notes.push(...)` note (not fatal).
- [ ] Run `cargo check -p savvagent --all-targets`, fix any remaining call-site fallout from the
      `LoadedConfig` shape change (e.g. `start_host_remote`'s legacy path, if it also calls
      `load_or_default` or receives `config_file` — confirm from the read code whether it needs the
      same treatment).
- [ ] Add an integration-style test (in `main.rs`'s test module or a new
      `tests/mcp_bootstrap.rs`, whichever matches this crate's existing test-location convention —
      check first) that builds a `ConfigFile` with one valid `[[mcp_servers]]` stdio entry pointing at
      a trivial test MCP server binary/script, runs the bootstrap path, and asserts the resulting
      `HostConfig.tools`/`Host::tool_server_statuses()` includes it. If spinning up a real trivial MCP
      server for this test is impractical without infra not already present, scope this down to a
      unit test of just the validate-and-resolve step (not the full `Host::start`), and note in the PR
      body why the fuller test was scoped down.
- [ ] Run `cargo test -p savvagent`. Confirm green.
- [ ] Commit: `feat: wire configured mcp_servers into host bootstrap`.

## Task 7: `/mcp` slash command + screen (`crates/savvagent/src/plugin/builtin/mcp/`)

**Files:**
- Create: `crates/savvagent/src/plugin/builtin/mcp/mod.rs`,
  `crates/savvagent/src/plugin/builtin/mcp/screen.rs`
- Modify: `crates/savvagent/src/plugin/builtin/mod.rs`, `crates/savvagent/src/plugin/mod.rs`
- Modify: `crates/savvagent-plugin/src/event.rs` — new `HostEvent::ToolServersReady` variant and a
  new plain DTO type (plugin-ABI-visible; confirmed `savvagent-plugin` has zero dependency on
  `savvagent-host` per its `Cargo.toml`'s "Intentionally NO ratatui, crossterm, tokio runtime,
  anyhow" comment and WIT-portability rule — this DTO must not reference `savvagent-host` types).
- Modify: `crates/savvagent/src/main.rs` — emit `HostEvent::ToolServersReady` once after
  `Host::start` succeeds in bootstrap (mirroring the existing `HostEvent::ProviderRegistered`/
  `Connect` dispatch calls in `perform_connect`, e.g. around `main.rs:2500`).

- [ ] Read `crates/savvagent/src/plugin/builtin/connect/mod.rs` and `screen.rs` in full immediately
      before starting this task (already read once during spec drafting — re-read for exact ABI
      shape: `Manifest`/`Contributions`/`SlashSpec`/`ScreenSpec`/`ScreenLayout`/`Effect::OpenScreen`/
      `create_screen`, and specifically the `candidates`-cache-via-`on_event` pattern the bridge
      design below reuses).
- [ ] **Plugin-ABI status bridge (blocking fix from plan-review round 1 — concrete design, not
      deferred to implementation time):** `Plugin`/`Screen` trait methods have no direct `&Host`
      access (confirmed: `Plugin::create_screen` takes only `(&self, id: &str, args: ScreenArgs)`,
      and `savvagent-plugin` cannot depend on `savvagent-host`'s `ToolServerStatus`/`ConnectState`/
      `TransportKind` types directly — that would violate the crate-boundary/WIT-portability rule).
      The bridge is the same pattern `ConnectPlugin` already uses for `HostEvent::ProviderRegistered`
      (`crates/savvagent/src/plugin/builtin/connect/mod.rs`'s `candidates: Vec<(ProviderId, String)>`
      field, updated in `on_event` and read by `create_screen`):
      1. Add plain (non-`savvagent-host`-dependent) types to `crates/savvagent-plugin/src/event.rs`:
         `pub enum ToolTransportKind { Stdio, Http }`, `pub enum ToolConnectState { Connected, Failed
         { reason: String } }`, `pub struct ToolServerStatusInfo { pub name: String, pub transport:
         ToolTransportKind, pub state: ToolConnectState }` — all three deriving `Debug, Clone,
         PartialEq, Eq` to match `HostEvent`'s own derive list — and a new
         `HostEvent::ToolServersReady { statuses: Vec<ToolServerStatusInfo> }` variant (with its
         `HookKind` mapping entry, mirroring every other `HostEvent` variant's treatment in
         `event.rs`). **Public-interface check:** confirmed `HostEvent` (`crates/savvagent-plugin/
         src/event.rs`) is **not** `#[non_exhaustive]` today, so adding this variant **is a breaking
         change** for any external exhaustive `match HostEvent { ... }` (e.g. a WASM plugin compiled
         against the old ABI) — this must be called out in the PR body per Rule 6 with the same
         treatment as `ToolEndpoint` in Task 1, and folded into this feature's single MINOR version
         bump (not a second separate bump). Do not mark `HostEvent` `#[non_exhaustive]` as part of
         this change unless the spec is updated first — that's a separate, its own scope decision
         and out of bounds for this plan; simply document the breakage.
      2. In `crates/savvagent/src/main.rs`, after `Host::start(config).await` succeeds in
         `bootstrap_pool_host` (and the equivalent GUI bootstrap path if one exists — check
         `egui_app`), convert `host.tool_server_statuses()` (the `savvagent-host`-side type from
         Task 2) into `Vec<ToolServerStatusInfo>` and dispatch `HostEvent::ToolServersReady` via
         `crate::plugin::effects::dispatch_host_event` exactly like the existing
         `ProviderRegistered`/`Connect` dispatches. Since v1 has no live reconnect, this fires exactly
         once per app launch — no ongoing subscription/polling needed.
      3. `McpPlugin` (this task) subscribes to `HookKind::ToolServersReady` in its `Manifest`
         (mirroring `ConnectPlugin`'s `HookKind::ProviderRegistered` subscription), caches the
         `Vec<ToolServerStatusInfo>` in a plugin-owned field on `on_event`, and passes a clone of that
         cache to `McpManagerScreen` in `create_screen` — exactly the same shape as
         `ConnectPlugin::candidates`/`ConnectPickerScreen::with_candidates`.
      4. **Combine with configured-but-not-yet-connected servers (advisory note from plan-review
         round 1, addressed here):** a server that failed `McpServerEntry::validate` or had an
         unreadable keyring secret in Task 6 never became a `ToolEndpoint` at all, so it will never
         appear in `tool_server_statuses()`. `McpManagerScreen` must show the full configured list
         from `config_file.mcp_servers` (passed in alongside the status cache, sourced the same way
         Task 6 already reads it) merged with the live status cache by name — entries present in
         config but absent from the status cache are rendered as a distinct "not started: <reason>"
         row (reusing whatever diagnostic string Task 6's skip-with-note path already produced),
         never conflated with `ConnectState::Failed` (which means "we tried to connect and it
         failed", a different condition from "we never tried because validation/secret-resolution
         failed first").
- [ ] `mod.rs`: `McpPlugin` registers slash command `"mcp"` (no args → open `mcp.manager` screen,
      matching `connect`'s no-arg behavior) and screen id `"mcp.manager"`. `create_screen` builds
      `McpManagerScreen` from the cached `Vec<ToolServerStatusInfo>` plus the configured-server list,
      per the bridge design above.
- [ ] `screen.rs`: `McpManagerScreen` lists configured servers (name, transport, status) — reuse
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
- [ ] Register `McpPlugin` in `crates/savvagent/src/plugin/builtin/mod.rs` (`pub mod mcp;` with a doc
      comment) and in `crates/savvagent/src/plugin/mod.rs`'s `register_builtins` plugin vec; update
      `register_builtins_pr8_complete` (and any other test asserting the builtin id list/count) to
      include `"internal:mcp"` (or whatever id convention this plugin uses — match `connect`'s
      `"internal:connect"` pattern only if `/mcp` is meant to be a Core, non-disableable plugin;
      otherwise follow the Optional-plugin registration pattern used by e.g. `lsp_installer` — decide
      based on whether the spec implies core-vs-optional, defaulting to Core since `/connect` is
      Core and `/mcp` is its direct analog).
- [ ] Add plugin-level unit tests mirroring `connect/mod.rs`'s test module shape: manifest exposes the
      `"mcp"` slash + `"mcp.manager"` screen; `handle_slash` with no args returns
      `Effect::OpenScreen`; screen key handling for add/remove (mock/stub the config-writer and
      keyring calls behind whatever seam is idiomatic here — check how other builtins with
      side-effecting slash handlers are tested, e.g. `save`/`route`, for the mocking convention before
      inventing a new one).
- [ ] Run `cargo test -p savvagent plugin::builtin::mcp`. Confirm green.
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

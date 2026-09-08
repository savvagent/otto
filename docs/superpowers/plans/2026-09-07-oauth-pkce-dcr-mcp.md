# oauth-pkce-dcr-mcp Implementation Plan

**Goal:** Implement OAuth 2.1 authorization-code support with PKCE and dynamic client registration
for user-configured remote Streamable HTTP MCP servers, reusing Otto's existing `[[mcp_servers]]`
transport model and keyring namespace so `/mcp` can configure, authorize, and persist remote HTTP
servers that require OAuth instead of a static bearer token.

**Architecture:** The feature is split across the existing transport boundary. `crates/otto` owns
all config parsing, keyring persistence, loopback callback UX, and OAuth flow orchestration via a
new `mcp_oauth` module that wraps `rmcp`'s auth primitives (`AuthorizationManager`,
`AuthorizationSession`, `AuthClient`, custom credential/state stores). `crates/otto-host` remains
credential-store agnostic: it receives either `HttpAuth::None`, `HttpAuth::Bearer`, or a
preconfigured `HttpAuth::OAuth { client }` and connects the remote MCP server through the same
`ToolRegistry` HTTP path it already owns. `/mcp` grows HTTP auth-mode selection plus authorize/check
actions, but remains restart-to-apply; no runtime tool-pool mutation or host-swap behavior changes
are introduced.

**Tech Stack:** Rust 2024, existing workspace crates only, `rmcp` 1.6 with the `auth` feature
enabled, `reqwest`, `axum` (loopback callback listener), `keyring`, `toml_edit`, `otto-plugin`
`Effect::OpenUrl`.

**Spec:** `docs/superpowers/specs/2026-09-07-oauth-pkce-dcr-mcp-design.md` — read it first. This
plan implements it exactly.

**Release line:** `v0.26.0` (MINOR — additive feature: remote HTTP MCP OAuth support)

**Branch:** `mcp/oauth-pkce-dcr`

## File Map

**New files**
- `crates/otto/src/mcp_oauth.rs` — OAuth storage, discovery/bootstrap helpers, loopback callback
  listener, pending-session state, and `/mcp` auth orchestration.
- `docs/superpowers/specs/2026-09-07-oauth-pkce-dcr-mcp-design.md` — committed design spec.
- `docs/superpowers/plans/2026-09-07-oauth-pkce-dcr-mcp.md` — this plan.

**Modified files**
- `Cargo.toml` — enable `rmcp`'s `auth` feature in the workspace dependency.
- `crates/otto-host/src/config.rs` — add `HttpAuth::OAuth` and redact it in `Debug`.
- `crates/otto-host/src/tools.rs` — construct HTTP transports with either the default reqwest
  client or an OAuth `AuthClient`, and improve auth-failure status text.
- `crates/otto/Cargo.toml` — add `axum` (already a workspace dependency) so the otto crate can host
  the loopback OAuth callback listener.
- `crates/otto/src/creds.rs` — typed load/save helpers for structured OAuth blobs while preserving
  raw bearer-token compatibility.
- `crates/otto/src/config_file.rs` — accept `auth = "oauth"` for HTTP MCP servers.
- `crates/otto/src/main.rs` — make configured-MCP resolution async, resolve OAuth-configured MCP
  servers into `HttpAuth::OAuth`, surface startup skip notes, and thread the new module into host
  bootstrap.
- `crates/otto/src/plugin/builtin/mcp/mod.rs` — extend `McpManagerOps` with OAuth begin/poll/clear.
- `crates/otto/src/plugin/builtin/mcp/screen.rs` — add HTTP auth-mode selection and authorize/check
  actions in the `/mcp` manager screen.
- `README.md` — document `auth = "oauth"`, keyring behavior, and the `/mcp` OAuth flow.

## Task 1: Enable OAuth-capable transport/config primitives

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/otto-host/src/config.rs`
- Modify: `crates/otto-host/src/tools.rs`

- [x] Add the `auth` feature to the workspace `rmcp` dependency in `Cargo.toml`; do not add any new
      third-party crates.
- [x] Add `HttpAuth::OAuth { client: rmcp::transport::auth::AuthClient<reqwest::Client> }` to
      `crates/otto-host/src/config.rs`, keeping the manual `Debug` redaction behavior intact.
- [x] Update `crates/otto-host/src/tools.rs`'s HTTP connect arm so `HttpAuth::OAuth` uses
      `StreamableHttpClientTransport::with_client(client.clone(), config)` while `None`/`Bearer`
      keep the existing config-only path.
- [x] Improve HTTP auth failure formatting so `ToolServerStatus` reasons differentiate missing
      authorization / reauthorization-required errors from generic transport failures.
- [x] Add/extend focused `otto-host` tests covering: OAuth transport construction, no-secret
      logging/redaction for the OAuth auth variant, and auth-failure status text.
- [x] Run `cargo test -p otto-host -- tools` and `cargo build -p otto-host --all-targets`; expect
      both to pass.
- [x] Public-interface check: record that `HttpAuth` gains an additive `OAuth` variant; no SPP,
      tool-schema, plugin-ABI, or on-disk format changes occur in this task.
- [x] Host-swap `RwLock` check: not applicable — no `app.rs`/`tui.rs` changes.
- [x] Commit: `otto-host: add oauth http auth transport support`.

## Task 2: Add typed OAuth secret persistence and bootstrap resolution

**Files:**
- Create: `crates/otto/src/mcp_oauth.rs`
- Modify: `crates/otto/Cargo.toml`
- Modify: `crates/otto/src/creds.rs`
- Modify: `crates/otto/src/config_file.rs`
- Modify: `crates/otto/src/main.rs`

- [x] Add `axum = { workspace = true }` to `crates/otto/Cargo.toml` so the otto crate can host the
      loopback callback listener without introducing a new dependency source.
- [x] In `crates/otto/src/creds.rs`, add typed helpers for serializing/deserializing structured OAuth
      blobs under `mcp:<server name>` while preserving raw-string bearer-token helpers for existing
      HTTP servers.
- [x] In `crates/otto/src/config_file.rs`, stop rejecting `McpAuthMode::Oauth`; add/update tests so
      tolerant config loading accepts OAuth rows and still rejects unrelated malformed entries.
- [x] Create `crates/otto/src/mcp_oauth.rs` with the storage/bootstrap types from the spec:
      `StoredMcpOAuthSecret`, `KeyringOAuthCredentialStore`, callback payload/state structs, startup
      auth-client builder, and helper functions for validating issuer binding and strict PKCE
      metadata requirements.
- [x] Refactor `crates/otto/src/main.rs` so configured MCP-server resolution is async end-to-end
      (`resolve_configured_mcp_servers`, `resolve_mcp_http_auth`, and the bootstrap callers) before
      wiring in metadata rediscovery; the sync shape used today is not sufficient for OAuth.
- [x] Update `crates/otto/src/main.rs::resolve_mcp_http_auth` so OAuth rows load the structured blob,
      rediscover metadata at startup, validate issuer binding, configure an `AuthorizationManager`,
      and emit `HttpAuth::OAuth { client }`; missing/malformed OAuth state must degrade gracefully to
      skip notes instead of aborting host startup.
- [x] Add focused tests covering structured-secret round trips, raw-bearer backward compatibility,
      OAuth config validation, and startup resolution/skip-note behavior.
- [x] Run `cargo test -p otto -- creds` and `cargo test -p otto -- config_file`; then run a focused
      bootstrap test selector for the new OAuth resolver cases. Expect all to pass.
- [x] Public-interface check: `auth = "oauth"` becomes an additive supported `[[mcp_servers]]`
      config value, and the `mcp:<server name>` keyring payload gains an OAuth JSON form without
      breaking legacy bearer-token entries.
- [x] Host-swap `RwLock` check: not applicable — this task touches no host-slot locking code.
- [x] Commit: `otto: add oauth mcp credential storage and bootstrap resolution`.

## Task 3: Implement `/mcp` OAuth flow orchestration

**Files:**
- Modify: `crates/otto/src/mcp_oauth.rs`
- Modify: `crates/otto/src/plugin/builtin/mcp/mod.rs`

- [x] In `crates/otto/src/mcp_oauth.rs`, implement OAuth begin/poll/cancel orchestration: metadata
      discovery, **Otto-owned reqwest DCR request construction** for a native/public client
      (`application_type = "native"`), loopback listener startup, browser authorization URL
      creation, callback capture, RFC 9207 `iss` validation, and token exchange via PKCE.
- [x] Reuse stored client registrations only when issuer matches and the stored redirect URI can be
      rebound exactly; otherwise perform fresh DCR and overwrite the stored registration on success.
- [x] Ensure the loopback listener binds only to `127.0.0.1`, accepts one callback, shuts down on
      completion/cancel, and never logs codes/tokens.
- [x] Extend `McpManagerOps` in `crates/otto/src/plugin/builtin/mcp/mod.rs` with OAuth begin/poll
      methods and an explicit pending-session cleanup path for delete/retry/close behavior; wire the
      real implementation through `RealMcpManagerOps` to the new module.
- [x] Add focused tests for DCR request shape, callback validation (`state`, `iss`, OAuth error
      callbacks), success-path token persistence, and refresh-token persistence via the credential
      store.
- [x] Run `cargo test -p otto -- mcp_oauth` and `cargo test -p otto -- plugin::builtin::mcp::`; fix
      any failures until green.
- [x] Public-interface check: `/mcp` gains additive OAuth actions/notes; no tool schema or wire
      protocol shape changes.
- [x] Host-swap `RwLock` check: not applicable — this task stays within `/mcp` plugin ops and the new
      OAuth module.
- [x] Commit: `otto: implement mcp oauth authorization flow`.

## Task 4: Extend `/mcp` screen UX and startup status integration

**Files:**
- Modify: `crates/otto/src/plugin/builtin/mcp/screen.rs`
- Modify: `crates/otto/src/plugin/builtin/mcp/mod.rs`
- Modify: `crates/otto/src/main.rs`

- [x] Add an HTTP auth-mode field to the add-server draft in `screen.rs`, supporting `none`,
      `bearer`, and `oauth`, with the secret field shown only for `bearer`.
- [x] Add OAuth row actions (`authorize`, `check callback`, plus cleanup messaging as needed) and
      update tips/status text so OAuth-specific states are understandable from the modal alone.
- [x] Ensure startup skip notes/status rows for OAuth servers distinguish `authorization required`,
      `issuer mismatch`, `refresh failed`, and generic connect failures where possible.
- [x] Extend screen/plugin tests to cover HTTP auth-mode entry creation, authorize/check effects, and
      the rendered status text for pending/authorized/error states.
- [x] Run `cargo test -p otto -- plugin::builtin::mcp::screen` and `cargo test -p otto -- plugin::builtin::mcp::`; expect green.
- [x] Public-interface check: `/mcp` UX expands additively; restart-to-apply behavior remains the
      same and must still be documented in notes/tips.
- [x] Host-swap `RwLock` check: not applicable — no `app.rs`/`tui.rs` edits.
- [x] Commit: `otto: extend mcp manager for oauth servers`.

## Task 5: Documentation, final validation, and close-out updates

**Files:**
- Modify: `README.md`
- Modify: `docs/superpowers/specs/2026-09-07-oauth-pkce-dcr-mcp-design.md`
- Modify: `docs/superpowers/plans/2026-09-07-oauth-pkce-dcr-mcp.md`

- [x] Update `README.md`'s `[[mcp_servers]]` and `/mcp` documentation to describe `auth = "oauth"`,
      keyring-backed OAuth storage, the authorize/check flow, and restart-to-apply behavior.
- [x] Run `cargo fmt --all` and then the required full validation set:
      `cargo fmt --all -- --check`, `cargo build --workspace`, `cargo test --workspace`,
      `cargo clippy --workspace --all-targets`.
- [x] Fix any failures until all four commands pass cleanly.
- [x] Update this plan in place: tick every completed checkbox and add brief implementation notes if
      task ordering or exact file placement changed.
- [x] Update the spec in place: flip `Status:` to `IMPLEMENTED` and add an `Implementation notes`
      section documenting any material deviations (for example, if runtime insufficient-scope retry
      lands as manual reauthorization instead of automatic step-up).
- [x] Public-interface check: confirm the shipped surface matches the spec (config + `/mcp` +
      keyring payload only; no hidden public-interface drift).
- [x] Final task note: after this PR merges, the separate release process can cut the next MINOR
      release; this plan does not open the release PR itself.
- [x] Commit: `docs: finalize oauth mcp spec plan and readme`.

## Implementation notes

- Task 3 and Task 4 landed together in commit `887e020` because the `/mcp` operation surface and
  the screen UX changed in one buildable slice.
- Final hardening extended beyond the original task breakdown: Otto now preserves disconnected
  `/mcp` seed/skip-note state without triggering OAuth network discovery during GUI/TUI fallback,
  constrains discovery URLs before fetching them (including DNS checks for cross-origin auth
  hosts), and uses per-flow registration instance IDs so stale concurrent OAuth sessions fail
  loudly instead of silently overwriting or dropping keyring state.
- The implementation keeps runtime insufficient-scope handling as a manual reauthorization path
  surfaced through startup notes and `/mcp`; automatic scope step-up/retry was intentionally left
  out of scope per the spec.
- CI follow-up: the `macos-latest` CI job flaked on
  `mcp_oauth::tests::begin_and_complete_authorization_persists_tokens_when_keyring_is_available`
  with `Platform secure storage failure: A default keychain could not be found` — the test's
  keyring-availability probe only runs once at the start, but the platform secret store became
  unreachable partway through the async OAuth flow on that runner. Fixed by extending
  `creds::is_backend_unavailable` coverage (new `is_backend_unavailable_message` helper) to the
  later poll/persist step too, so the test skips on backend-unavailable errors there the same way
  it already does at the initial probe, instead of panicking.

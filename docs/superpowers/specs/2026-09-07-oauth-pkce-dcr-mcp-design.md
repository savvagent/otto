# Support OAuth 2.1 + PKCE + dynamic client registration for remote HTTP MCP servers — design

Date: 2026-09-07
Status: IMPLEMENTED
Related: `savvagent/otto#49`
Related: `docs/superpowers/specs/2026-09-05-mcp-servers-design.md`

## Brief

Add end-to-end OAuth 2.1 authorization-code support for user-configured remote Streamable HTTP MCP
servers (`[[mcp_servers]]` with `transport = "http"`) using PKCE and dynamic client registration,
with credentials persisted in the existing OS keyring namespace (`otto` service,
`mcp:<server name>` account family), automatic access-token refresh, and `/mcp` manager UX for
starting and completing the authorization flow and surfacing authorization failures.

## Assumptions

- Otto will implement the issue's requested **dynamic client registration** path and will **not**
  attempt Client ID Metadata Documents in this change. Rationale: the issue explicitly scopes in DCR,
  while Otto does not currently host an HTTPS metadata document and has no existing public web origin
  to use as a `client_id` URL.
- Otto will treat the MCP client as a **public native client** and request
  `token_endpoint_auth_method = "none"` with `application_type = "native"` during dynamic client
  registration. Rationale: the TUI is a local desktop/CLI app and must not ship or persist a shared
  client secret.
- OAuth authorization will use a **loopback redirect URI** on `127.0.0.1` with a stable stored
  redirect URI once a client registration exists; the first authorization attempt may choose an
  ephemeral port, and later re-authorization attempts will try to re-bind that exact stored URI
  before falling back to a fresh DCR registration if the port is unavailable. Otto will also show a
  manual copy-open fallback note if launching the browser fails. Rationale: this preserves exact
  redirect-URI matching for authorization servers that validate registered loopback URIs strictly
  while still allowing recovery when the old port can no longer be reused.
- The `/mcp` manager will remain **restart-to-apply** after successful authorization, matching the
  existing add/remove MCP-server UX shipped in #41. Rationale: runtime tool-registry mutation is
  still out of scope for this issue.
- Otto will continue to support existing `auth = "bearer"` HTTP servers whose keyring value is a raw
  token string. Rationale: OAuth must be additive and must not break the already-shipped static-auth
  path.
- When a configured OAuth server advertises no dynamic-registration endpoint and Otto has no stored
  client registration for that server/issuer pair, the flow will fail with an actionable note rather
  than prompting for manual client credentials. Rationale: manual pre-registration UI is not part of
  the issue scope.
- For local tests and loopback development, Otto may accept `http://127.0.0.1/...` or
  `http://localhost/...` authorization server metadata/redirects in test fixtures, while production
  behavior still prefers HTTPS and loopback-only redirects. Rationale: the repo's integration tests
  run against local mock servers.

## Goal & Success Criteria

Remote HTTP MCP servers configured with `auth = "oauth"` should be first-class peers of today's
`auth = "bearer"` servers: a user can add the server in `/mcp`, authorize it via a browser-based
PKCE flow, persist the resulting registration + token set in the OS keyring, restart Otto, and have
future sessions connect automatically with proactive refresh and clear recovery notes when
re-authorization is needed.

Success criteria:

- `[[mcp_servers]]` accepts `auth = "oauth"` for HTTP entries, and `/mcp` can create those entries.
- `/mcp` can start an OAuth authorization flow, launch the browser, receive the loopback callback,
  exchange the code with PKCE, persist credentials, and report success/failure without exposing
  secrets in logs, notes, or transcripts.
- Otto can boot an OAuth-configured remote MCP server from stored credentials, automatically refresh
  the access token before expiry, and persist the refreshed token set back to the keyring.
- OAuth failures are actionable: missing authorization, issuer mismatch, failed dynamic
  registration, token refresh failure, or malformed callbacks surface a clear `/mcp`-oriented note.
- Existing `auth = "bearer"`, stdio MCP servers, bundled tools, and host/provider behavior remain
  unchanged.

## Scope

### In scope

- HTTP `[[mcp_servers]]` entries with `auth = "oauth"`.
- OAuth 2.1 authorization code flow with PKCE (`S256`) for remote Streamable HTTP MCP servers.
- Discovery via MCP authorization rules: `WWW-Authenticate` parsing when available and fallback to
  `.well-known/oauth-protected-resource`, then authorization-server metadata discovery.
- Dynamic client registration (RFC 7591) for native/public clients when no stored registration is
  already bound to the discovered authorization-server issuer.
- Keyring-backed persistence for OAuth registration + token state under the existing
  `mcp:<server name>` namespace.
- Automatic access-token refresh and persistence of refreshed credentials.
- `/mcp` manager UX for configuring OAuth entries, initiating authorization, completing/polling the
  callback, and surfacing auth-specific status/errors.
- README updates for new config/UX.

### Out of scope

- Local stdio MCP servers.
- Replacing or changing the existing static `auth = "bearer"` path.
- Client ID Metadata Documents, manual pre-registered OAuth client credentials, or device-code flow.
- Live hot-reload/reconnect of tool servers after auth completion (restart remains required).
- Provider-side OAuth (this issue applies to remote **tool** servers configured under
  `[[mcp_servers]]`, not `OTTO_PROVIDER_URL`).
- Any changes to the SPP wire format, plugin ABI, or provider pool model.

## Premise corrections

- The current repository already ships the base user-configured MCP-server feature from #41: HTTP
  endpoints, `auth = "bearer"`, the `/mcp` manager plugin, and keyring namespace `mcp:<server>` all
  exist. This issue layers OAuth onto that design; it is not a greenfield MCP-server feature.
- `otto-host` already has the right transport seam for remote HTTP tools. The missing piece is not a
  brand-new registry abstraction; it is richer HTTP auth wiring plus TUI-side OAuth state handling.
- The workspace's `rmcp` dependency already contains a transport-auth subsystem implementing core
  OAuth discovery, PKCE helpers, refresh-token handling, and credential/state storage traits. Otto
  should reuse that infrastructure for token/state management and transport auth, but **not** blindly
  for DCR request construction where MCP-specific fields (notably `application_type = "native"`)
  need stricter control than `rmcp` 1.6 currently exposes.

## Public-interface changes

This change is **additive** to Otto's user-facing surfaces:

- `[[mcp_servers]]` gains supported `auth = "oauth"` for `transport = "http"`.
- `/mcp` gains OAuth-oriented actions/notes for HTTP servers.
- The keyring payload format for `mcp:<server name>` expands to support a structured OAuth JSON blob
  **when** the config entry uses `auth = "oauth"`; existing bearer-token entries remain raw strings
  and continue to load unchanged.

This change does **not** alter the SPP wire format, any MCP tool schema, the plugin ABI, or the
provider pool APIs.

## Approach

### 1. Add OAuth-aware persisted state in `crates/otto`

Create a new TUI-side module (e.g. `crates/otto/src/mcp_oauth.rs`) that owns:

- `StoredMcpOAuthSecret`: serializable keyring payload containing:
  - `issuer` (authorization-server issuer this registration is bound to)
  - `client_id`
  - optional `client_secret` (accepted from DCR responses but expected to be absent for Otto's
    public/native registration)
  - `redirect_uri`
  - `granted_scopes`
  - `token_received_at`
  - the OAuth token response (access token, refresh token, expiry, extra fields)
- `KeyringOAuthCredentialStore`: implements `rmcp::transport::auth::CredentialStore` by reading and
  writing the `StoredMcpOAuthSecret` JSON blob through `crate::creds`, preserving the existing raw
  bearer-token helpers for `auth = "bearer"`.
- `MemoryOAuthStateStore`: thin wrapper for in-flight PKCE state and callback correlation.
- `PendingOAuthSession`: in-memory flow state for one `/mcp` authorization attempt, including the
  discovered metadata snapshot, expected issuer, whether `iss` is required, the loopback redirect
  URI, and a oneshot/receiver used by the local callback listener.
- `DynamicRegistrationClient`: a tiny Otto-owned helper that POSTs the DCR JSON body directly with
  `reqwest` so Otto can include `application_type = "native"` and any other required fields while
  still handing the resulting client registration back to `AuthorizationManager` for the rest of the
  flow.

`creds.rs` remains the single OS-keyring boundary, but it grows typed helpers for loading/saving the
structured OAuth blob in addition to the existing raw `mcp_save`/`mcp_load` bearer-token path.

### 2. Extend `otto-host` HTTP auth to support dynamic OAuth clients at runtime

`crates/otto-host/src/config.rs`'s `HttpAuth` gains a new additive variant:

```rust
pub enum HttpAuth {
    None,
    Bearer { token: String },
    OAuth {
        client: rmcp::transport::auth::AuthClient<reqwest::Client>,
    },
}
```

This keeps `otto-host` free of keyring/config-file dependencies while allowing the embedder to hand
it a preconfigured OAuth-capable HTTP client. The manual `Debug` impl continues redacting secrets.

`crates/otto-host/src/tools.rs`'s HTTP connect arm switches from a single
`StreamableHttpClientTransport::from_config(...)` path to:

- `None` / `Bearer` => current config-based transport construction.
- `OAuth` => `StreamableHttpClientTransport::with_client(auth_client.clone(), config)`.

The existing two-stage timeout/cancel structure remains intact. Error formatting is tightened so
authorization-required / token-refresh / insufficient-scope failures surface an action-oriented
reason suitable for `/mcp` status rows.

### 3. Resolve `auth = "oauth"` entries during host bootstrap

`crates/otto/src/config_file.rs` stops rejecting `McpAuthMode::Oauth` in `validate()`. Because OAuth
metadata rediscovery and auth-client construction are async, `crates/otto/src/main.rs` must also
reshape `resolve_configured_mcp_servers`/`resolve_mcp_http_auth` from a synchronous helper into an
async bootstrap step. The resolver becomes a three-way async path:

- `None` => `HttpAuth::None`
- `Bearer` => current raw-token load from `mcp:<server name>`
- `Oauth` => load a structured OAuth secret, discover current authorization metadata, ensure the
  discovered issuer still matches the stored issuer, configure an `AuthorizationManager` with the
  stored client registration, install the keyring-backed credential store, and build
  `HttpAuth::OAuth { client }`

If no OAuth credentials exist yet, or the stored blob is malformed/bound to the wrong issuer, the
server is skipped from startup with a note captured in `McpManagerSeed.skip_notes` and the startup
notes list (e.g. "oauth authorization required; open /mcp").

This preserves the crate boundary: `otto` does keyring/config work and constructs the auth client;
`otto-host` only consumes an already-built transport/auth value.

### 4. Implement OAuth flow orchestration for `/mcp`

Extend `crates/otto/src/plugin/builtin/mcp/mod.rs`'s `McpManagerOps` with OAuth methods such as:

- `begin_oauth(entry: &McpServerEntry) -> Result<OAuthBeginResult, String>`
- `poll_oauth(name: &str) -> Result<OAuthPollResult, String>`
- `clear_oauth(name: &str) -> Result<(), String>` (used by delete/retry cleanup)

The real implementation (`RealMcpManagerOps`) delegates to the new `mcp_oauth` module.

OAuth begin flow:

1. Validate the selected server is `transport = "http"` + `auth = "oauth"`.
2. Discover protected-resource and authorization-server metadata.
3. Require `code_challenge_methods_supported` to contain `S256`; fail closed if absent.
4. If Otto already has a stored registration for the same issuer, try to bind a loopback listener at
   that exact stored `redirect_uri`; otherwise bind `127.0.0.1:0` and derive a new `redirect_uri`.
5. Reuse the stored client registration only when the issuer matches **and** the stored redirect URI
   could be rebound exactly; otherwise perform a fresh DCR registration using the newly bound
   listener URI and replace the stored registration on success.
6. Create an `AuthorizationSession`, derive the authorization URL, extract/store the `state`, and
   remember the expected issuer / issuer-support flag for callback validation.
7. Return effects that both open the authorization URL in the system browser and push a note telling
   the user how to continue if browser launch fails.

Completion/poll flow:

1. Non-blockingly inspect the pending callback receiver.
2. If still pending, report that `/mcp` is waiting for the browser redirect.
3. If a callback arrived with `error`, surface it directly.
4. Validate `state` and `iss` (RFC 9207 rules using the stored expected issuer and the
   `authorization_response_iss_parameter_supported` metadata flag).
5. Exchange the code through `AuthorizationManager::exchange_code_for_token`, which persists the
   resulting token set via the keyring-backed credential store.
6. Persist/update the registration binding blob, clear the in-memory pending session, and tell the
   user to restart Otto to apply the newly authorized server.

The listener returns a tiny success/failure HTML page so the browser tab can close cleanly.

### 5. Update `/mcp` screen state and key handling

`crates/otto/src/plugin/builtin/mcp/screen.rs` grows HTTP-auth awareness:

- Add an `Auth` field to the add-server draft for HTTP servers (`none` / `bearer` / `oauth`).
- Only show the secret input field when HTTP auth mode is `bearer`.
- Add list actions for OAuth rows, e.g. `o authorize` and `c check callback` (exact key names may be
  adjusted to fit the existing screen layout).
- Enrich row/status text so OAuth states read differently from ordinary connect failures:
  `authorization required`, `awaiting browser callback`, `issuer mismatch`, `token refresh failed`,
  etc.

The screen stays modal and restart-to-apply; no host-swap or runtime registry mutation is added.

### 6. Local callback server and browser interaction live in `crates/otto`

The callback listener belongs in the TUI crate, not `otto-host`, because it is a UX concern and must
integrate with `/mcp` and `Effect::OpenUrl`, not the tool-use loop. The implementation should:

- bind only to loopback (`127.0.0.1`)
- accept exactly one callback request per authorization attempt
- capture `code`, `state`, optional `iss`, and OAuth error fields
- shut down once one callback is received or the session is cancelled
- never log authorization codes or tokens

If `Effect::OpenUrl` fails to launch the system browser, Otto still shows the authorization URL so a
user can copy it manually.

## Data flow

1. User adds an HTTP MCP server with `auth = "oauth"` in `/mcp`.
2. Config is written to `~/.otto/config.toml`; no secret is stored yet.
3. User selects the row and triggers authorize.
4. Otto discovers metadata, dynamic-registers a public client if needed, launches the browser, and
   waits on the loopback callback.
5. User returns to `/mcp` and confirms/polls completion; Otto validates `state`/`iss`, exchanges the
   code with PKCE, and writes the OAuth blob to `mcp:<server name>` in the keyring.
6. On next startup, `resolve_mcp_http_auth` loads the blob, constructs an OAuth-capable auth client,
   and hands it to `otto-host` via `HttpAuth::OAuth`.
7. During normal operation, `AuthClient` fetches/refreshes access tokens as needed and persists
   refreshed credentials back through the keyring-backed credential store.

## Error handling & edge cases

- **Missing OAuth keyring blob**: skip startup registration and show `authorization required; open
  /mcp`.
- **Malformed keyring blob**: skip the server, preserve logs without printing secret material, and
  ask the user to re-authorize in `/mcp`.
- **Authorization-server issuer changed**: reject the stored registration/token set, clear the OAuth
  secret only on explicit user action, and require a fresh authorization.
- **DCR unsupported**: fail begin-auth with a message that this server requires pre-registered or
  metadata-document client registration, which Otto does not support in this ticket.
- **PKCE metadata missing or no `S256`**: fail closed before opening the browser.
- **Browser callback never arrives**: keep the pending state in memory and report `awaiting browser
  callback`; allow the user to retry or cancel.
- **Callback `state` mismatch / missing state**: reject the callback and discard the pending session.
- **Callback `iss` mismatch or missing when required**: reject the callback before token exchange.
- **Authorization denied by user**: surface `error` / `error_description` from the callback.
- **Refresh token unavailable or refresh rejected**: runtime requests fail with an authorization
  required message; `/mcp` can re-run the authorize flow.
- **Stored redirect URI port unavailable**: fall back to a fresh listener + fresh DCR registration
  rather than reusing a client whose redirect URI no longer matches.
- **Loopback bind failure**: fail begin-auth with a clear local-listener error.
- **Deleting a configured OAuth server**: remove the config row and delete the keyring secret just as
  the bearer path already does.
- **Legacy bearer secret present for an OAuth row (or vice versa)**: treat as invalid for that auth
  mode and ask the user to re-enter/re-authorize.

## Testing approach

- `crates/otto/src/config_file.rs`: update validation tests so `auth = "oauth"` is accepted.
- `crates/otto/src/creds.rs`: add round-trip tests for structured OAuth secret serialization and
  backward compatibility with raw bearer tokens.
- New unit/integration tests for `mcp_oauth.rs` using local mock HTTP servers to cover:
  - protected-resource metadata discovery via `WWW-Authenticate` and well-known fallback
  - dynamic client registration request shape (`application_type = native`, redirect URI, auth method)
  - authorization callback validation (`state`, `iss`, error callbacks)
  - token exchange + refresh persistence through the keyring-backed credential store
- `crates/otto-host/src/tools.rs`: add/connect tests for `HttpAuth::OAuth` transport wiring and
  auth-failure reason formatting without leaking secrets.
- `crates/otto/src/plugin/builtin/mcp/screen.rs`: extend screen tests for HTTP auth-mode selection,
  authorize/check actions, and note generation.
- Workspace validation before PR: `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets`, `cargo fmt --all -- --check`.

## Implementation notes

- Otto reuses `rmcp`'s `AuthorizationManager`/`AuthClient` stack for discovery, PKCE, token
  exchange, and refresh, but keeps dynamic client registration in Otto-owned `reqwest` code so the
  registration payload can include `application_type = "native"` and other MCP-native-client
  expectations explicitly.
- Startup OAuth resolution is fully async in `crates/otto/src/main.rs`, matching the spec's final
  critique round: Otto rediscoveres authorization metadata before constructing `HttpAuth::OAuth`
  and skips invalid/missing OAuth state with actionable startup notes instead of aborting the whole
  host bootstrap.
- The shipped UX keeps insufficient-scope and refresh failures as clear manual reauthorization paths
  surfaced in `/mcp` and startup notes; automatic scope step-up/retry remains out of scope.

## Risks & open questions

- `rmcp`'s built-in auth layer provides most of the mechanics we need, but Otto still needs to
  enforce a few stricter MCP requirements itself (notably fail-closed PKCE metadata checks,
  callback `iss` validation, and `application_type = "native"` in the DCR request). The
  implementation must not assume `rmcp` already covers those details.
- The current `/mcp` screen model is key-driven and intentionally simple. The final UX must stay
  understandable without introducing a full background task UI.
- `ToolServerStatus` is a startup snapshot, not a live auth-health stream. The implementation should
  keep OAuth runtime status simple: startup/auth-flow state must be reflected clearly in `/mcp`, and
  deeper live-status work can stay out of scope unless required to satisfy the issue.
- Dynamic client registration is deprecated in the MCP authorization docs in favor of client ID
  metadata documents, but the issue explicitly asks for DCR. Otto should keep the implementation
  narrowly scoped and avoid over-investing in abstractions that would only matter for a later CIMD
  feature.
- Runtime insufficient-scope step-up authorization is adjacent to this feature. This ticket should at
  minimum surface insufficient-scope failures clearly and allow re-authorization; if seamless scope
  union/retry proves too invasive for the existing tool transport, document that limitation in the
  final implementation notes.

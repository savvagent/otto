# DeepSeek `list_models` misclassifies HTTP errors as `Network` — design

Date: 2026-09-08
Status: pending review
Related: `savvagent/otto#81`

## Problem

Issue #81 was reopened: a user entered a wrong DeepSeek API key, was never
offered another opportunity to fix it via `/connect deepseek`, and later saw
a "not connected"-shaped failure when trying to actually use DeepSeek. PR 85
(merged for the original #81) added a `--rekey`-pointing hint
(`notes.connect-rejected-keyed`) at the two connect-time rejection sites
(`perform_connect` and `apply_pending_pool_add` in `crates/otto/src/main.rs`)
and a turn-time equivalent, gated on
`kind == otto_protocol::ErrorKind::Authentication && spec.api_key_required`.
That fix is correct and already shipped — but it never fires for DeepSeek,
because DeepSeek's own `ProviderHandler::list_models` never reports
`ErrorKind::Authentication` for a bad key in the first place.

`crates/provider-deepseek/src/lib.rs::list_models` (`lib.rs:130-153`) calls a
private helper, `http_status_error` (`lib.rs:244-266`), for any non-2xx
`/models` response:

```rust
fn http_status_error(label: &str, status: reqwest::StatusCode, body: String) -> ProviderError {
    // ...
    ProviderError {
        kind: ErrorKind::Network,   // <-- always Network, regardless of `status`
        message,
        retry_after_ms: None,
        provider_code: None,
    }
}
```

This hardcodes `ErrorKind::Network` no matter what HTTP status came back —
a 401 (revoked/invalid key), a 403 (permission/quota), and a 503 (DeepSeek
outage) are all indistinguishable from a DNS failure. Notably, the crate's
own test at `lib.rs:511-533` (`list_models_propagates_http_failure`)
currently *asserts* `matches!(err.kind, ErrorKind::Network)` for a mocked
401 response — the bug is encoded as expected behavior in the test suite.

Meanwhile `complete()`'s error path in the very same file
(`parse_error_response`, `lib.rs:272-313`) already does the *correct*
status→kind mapping:

```rust
let kind = match status.as_u16() {
    400 => ErrorKind::InvalidRequest,
    401 => ErrorKind::Authentication,
    403 => ErrorKind::PermissionDenied,
    404 => ErrorKind::ModelNotFound,
    413 => ErrorKind::ContextLengthExceeded,
    429 => ErrorKind::RateLimited,
    500 | 502 | 503 | 504 => ErrorKind::Overloaded,
    _ => ErrorKind::Internal,
};
```

`provider-anthropic` and `provider-gemini` already extracted this exact
match into a shared `status_to_error_kind(status: u16) -> ErrorKind` helper
(`provider-anthropic/src/lib.rs:191`, `provider-gemini/src/lib.rs:210`) and
use it in *both* their `complete()` error path and their `list_models` error
path (`provider-anthropic/src/models.rs:66`,
`provider-gemini/src/models.rs:66`) — each with an explicit
`list_models_401_maps_to_authentication`-style test. `provider-openai`
has the identical `http_status_error`-always-`Network` defect in its own
`list_models` (`provider-openai/src/lib.rs:138-141`) as DeepSeek — same
copy-pasted shape, same bug — but issue #81 only reports DeepSeek, so this
spec fixes DeepSeek only and files a follow-up issue for OpenAI (see Risks).

### Why this produces exactly the two symptoms in #81

`crates/otto/src/plugin/builtin/provider_common.rs::build_dynamic_caps`
treats a `list_models` failure differently depending on `e.kind`
(`provider_common.rs:225-233`, doc comment):

- `ErrorKind::Network` / `ErrorKind::NotImplemented` → treated as
  transient/unsupported: fall back to the provider's static capability list,
  `DynamicCapsOutcome::Ready(static_fallback, Some(note))` — the provider
  **is** registered and added to the host pool.
- Any other kind (including `Authentication`) →
  `DynamicCapsOutcome::Rejected { reason, kind }` — the provider is **not**
  registered.

Because DeepSeek's `list_models` always reports `Network` for a 401, a bad
DeepSeek key takes the *first* branch: `try_build_registration` returns
`ProviderBuildOutcome::Ready(reg, Some(note))`, `perform_connect`/
`apply_pending_pool_add` add it to the host pool as if the key worked (the
static-fallback note reads like a benign "using the built-in model list"
message, not a credential warning), and:

1. **No `--rekey` hint is ever shown**, since `connect_rejected_note_key`
   (`crates/otto/src/main.rs:90-98`) only selects the hinted message on
   `Ok(ProviderBuildOutcome::Rejected { kind: Authentication, .. })` — a
   branch DeepSeek's bad-key case never reaches. The user has no
   discoverable path back into the API-key modal on a plain `/connect
   deepseek` retry (the silent-reconnect fast path,
   `try_connect_from_keyring`, only re-opens the modal for `--rekey` or a
   provider with no keyring entry at all — a provider that "connected"
   successfully is neither).
2. **Every subsequent turn against DeepSeek fails**, because `complete()`'s
   `parse_error_response` correctly reports the same 401 as
   `ErrorKind::Authentication` — but by then the user has already been told
   (via the connect-time fallback note) that DeepSeek is connected, so the
   turn-time failure reads as a broken/"not connected" provider rather than
   a key problem, with the connect flow offering no way back in.

The fix in this spec restores the two-symptom chain to the *already
correct* PR 85 behavior: once DeepSeek's `list_models` classifies a 401 as
`Authentication`, `build_dynamic_caps` correctly rejects the registration,
`connect_rejected_note_key` correctly selects the `--rekey`-hinted message,
and the provider is correctly kept out of the host pool until the user
supplies a working key. No changes to `otto` (the TUI crate), PR 85's
locale keys, or `provider_common.rs` are needed — the defect is entirely
inside `provider-deepseek`.

## Approach

Mirror the `provider-anthropic`/`provider-gemini` pattern inside
`crates/provider-deepseek/src/lib.rs`:

1. Extract the status→kind match already present in `parse_error_response`
   into its own function:

   ```rust
   /// Map a DeepSeek HTTP response status to the SPP `ErrorKind` it
   /// represents. Shared by `parse_error_response` (`complete()`'s error
   /// path) and `list_models`'s error path so both report the same
   /// classification for the same status — previously `list_models`
   /// hardcoded `ErrorKind::Network` for every non-2xx response,
   /// misclassifying a bad API key (401) as a transient network error.
   fn status_to_error_kind(status: u16) -> ErrorKind {
       match status {
           400 => ErrorKind::InvalidRequest,
           401 => ErrorKind::Authentication,
           403 => ErrorKind::PermissionDenied,
           404 => ErrorKind::ModelNotFound,
           413 => ErrorKind::ContextLengthExceeded,
           429 => ErrorKind::RateLimited,
           500 | 502 | 503 | 504 => ErrorKind::Overloaded,
           _ => ErrorKind::Internal,
       }
   }
   ```

2. `parse_error_response` calls `status_to_error_kind(status.as_u16())`
   instead of its inline match (pure refactor, no behavior change there —
   it already did the right thing).

3. `http_status_error` — used only by `list_models` — takes `kind` from
   `status_to_error_kind(status.as_u16())` instead of hardcoding
   `ErrorKind::Network`. Its doc comment ("Build a `Network`-kind
   `ProviderError`...") is corrected to describe the real behavior (maps
   the response status to the matching `ErrorKind`; message still carries
   the label/status/body for display). Signature/call site otherwise
   unchanged — `list_models` still calls
   `http_status_error("DeepSeek /models", status, body)`.

4. Fix the existing test that encodes the bug:
   `list_models_propagates_http_failure` (`lib.rs:511-533`) currently mocks
   a 401 and asserts `ErrorKind::Network`; change the assertion to
   `ErrorKind::Authentication`, matching Anthropic's/Gemini's equivalent
   test and the corrected behavior.

5. Add coverage mirroring Anthropic/Gemini's explicit per-status tests:
   `list_models_401_maps_to_authentication` (also asserts the message
   contains the mocked body's `invalid api key`-style text) and
   `list_models_5xx_maps_to_overloaded`, plus a `status_to_error_kind` unit
   test enumerating the full mapping table (400/401/403/404/413/429/5xx/
   unmapped) so the table itself is directly tested, not just two call
   sites through it.

No changes are needed in `crates/otto/src/plugin/builtin/provider_common.rs`
or `crates/otto/src/main.rs` — PR 85's `build_dynamic_caps`,
`connect_rejected_note_key`, `--rekey`, and the turn-time
`notes.turn-auth-failed-hint` path are all already correct and start working
for DeepSeek the moment `list_models` reports the right `kind`.

## Scope

**In:**
- `crates/provider-deepseek/src/lib.rs` — add `status_to_error_kind`;
  reuse it in `parse_error_response` and `http_status_error`; correct
  `http_status_error`'s doc comment; fix the bug-encoding assertion in
  `list_models_propagates_http_failure`; add
  `list_models_401_maps_to_authentication`,
  `list_models_5xx_maps_to_overloaded`, and a `status_to_error_kind` table
  unit test.

**Out:**
- `provider-openai`'s identical `list_models`/`http_status_error` defect
  (confirmed present, `provider-openai/src/lib.rs:138-141`) — same bug
  shape, but issue #81 is scoped to DeepSeek. Per this workflow's
  Stop-and-Escalate guidance on discovering the same bug pattern elsewhere,
  this is tracked as a new follow-up issue rather than silently widened
  into this PR (see Risks).
- Any change to `crates/otto`'s connect flow, `provider_common.rs`,
  `main.rs`'s rejection-note gating, or the locale files — all already
  correct per PR 85 and untouched by this fix.
- Any change to the silent-reconnect fast path
  (`try_connect_from_keyring`'s "client construction succeeded" heuristic)
  — a separate, real question (does building an HTTP client prove the key
  works?) that is orthogonal to this bug: once `list_models` classifies
  correctly, a bad key is rejected before it ever reaches that fast path
  again, because the keyring's bad key is never treated as "connected" in
  the first place.
- Deleting or auto-correcting a stored bad key in the keyring — unrelated
  to this fix and explicitly out of scope per the prior #81 design
  (`docs/superpowers/specs/2026-09-08-reconnect-after-api-key-error-design.md`).

## Public-interface changes

None. `status_to_error_kind` is a new private (`fn`, not `pub`) helper
inside `provider-deepseek`; `http_status_error` and `parse_error_response`
are already-private functions whose *return values* now classify some
previously-`Network` cases as `Authentication`/`PermissionDenied`/etc. —
this is a bugfix to `ProviderError.kind`'s accuracy, not a shape change to
`ProviderError`, `ProviderHandler`, or the SPP wire format. No tool schema,
plugin ABI, slash command, or on-disk transcript/keyring format is touched.

## Assumptions

- **DeepSeek's `/models` endpoint returns the same status codes as its
  chat-completions endpoint for auth/permission/rate-limit failures**
  (401/403/429), consistent with `complete()`'s existing, already-correct
  mapping for the same API. No DeepSeek-specific `/models`-only status
  quirks are assumed beyond what `parse_error_response` already handles.
- **`build_dynamic_caps`'s `Network`/`NotImplemented`-transient-fallback
  design (provider_common.rs) is correct and stays as-is.** The defect is
  that DeepSeek fed it the wrong `kind`, not that the fallback policy
  itself is wrong — a real DNS/timeout failure (via `map_reqwest_error`,
  unaffected by this change) should still fall back to static capabilities
  rather than reject the whole connection.
- **The OpenAI twin bug is out of scope for this PR and is filed as a
  separate follow-up issue**, per this task's "same bug pattern discovered
  elsewhere → new issue, don't silently widen scope" rule — justified
  because #81 is specifically about DeepSeek and fixing OpenAI here would
  both widen the diff beyond the reported issue and require its own
  independent verification (OpenAI's own test suite, its own PR review).

## Goal & Success Criteria

Restore the already-shipped PR 85 `--rekey` recovery path for DeepSeek by
making DeepSeek's connect-time key validation classify a bad key the same
way its turn-time validation already does.

- A mocked DeepSeek `/models` 401 response yields
  `ProviderError { kind: ErrorKind::Authentication, .. }` from
  `list_models` (not `Network`).
- `list_models_propagates_http_failure` asserts `Authentication`
  (no longer encodes the bug), and new tests cover 401/403/404/413/429/5xx
  status mapping explicitly.
- End-to-end: connecting DeepSeek with an invalid key now reaches
  `apply_pending_pool_add`'s/`perform_connect`'s
  `Ok(ProviderBuildOutcome::Rejected { kind: Authentication, .. })` arm,
  which (unmodified, already shipped) pushes
  `notes.connect-rejected-keyed` (the `--rekey` hint) and does **not** add
  DeepSeek to the host pool — so a subsequent `/connect deepseek --rekey`
  reopens the API-key modal, and DeepSeek is correctly absent from
  `/model`'s catalog and the connect picker's "connected" candidates until
  a working key is supplied.
- `cargo test -p provider-deepseek` and `cargo test --workspace` stay
  green; `cargo clippy --workspace --all-targets` and
  `cargo fmt --all --check` stay clean.

## Error Handling & Edge Cases

- A genuine transient failure (DNS resolution failure, connection refused,
  request timeout) still goes through `map_reqwest_error`
  (`lib.rs:225-234`), untouched by this change, and still reports
  `ErrorKind::Network` — `build_dynamic_caps`'s static-fallback path still
  applies for real connectivity blips.
- A non-JSON or unexpected-shape error body from DeepSeek still falls back
  to using the raw body text as the message (existing behavior in both
  `http_status_error` and `parse_error_response`), unaffected by the `kind`
  fix.
- An unmapped/unusual status code (e.g. 418) falls through to
  `ErrorKind::Internal` in `status_to_error_kind`, matching the existing
  `_ => ErrorKind::Internal` arm already present in `parse_error_response`
  — no new unmapped-status behavior introduced.

## Risks & Open Questions

- **OpenAI has the identical defect** (`provider-openai/src/lib.rs:138-141`
  hardcodes `ErrorKind::Network` in its own `list_models` error path,
  same `http_status_error`-shaped helper). This is a confirmed, separate
  instance of the same bug pattern. Per Stop-and-Escalate guidance, this is
  not silently folded into this PR; a follow-up GitHub issue is filed
  against `savvagent/otto` immediately after this PR merges, so it's
  tracked and not lost.
- Low risk otherwise: the change is a pure reclassification of an
  already-correctly-parsed HTTP status inside one crate, backed by
  existing test patterns already proven out in `provider-anthropic`/
  `provider-gemini`.

# Quiet startup provider auto-connect noise; validate keys and attribute provider errors — design

Date: 2026-09-07
Status: pending review
Related: `savvagent/otto#14`

## Problem

`bootstrap_pool_host` (`crates/otto/src/main.rs:495-661`) tries every built-in
provider plugin (Anthropic → Gemini → OpenAI → Local/Ollama) in turn via the
`try_provider!` macro. Each timeout, build failure, or model-catalog fallback
pushes a localized note into `deferred_notes`, which `run_app` renders into
the transcript once `App` exists. On a machine with only one working provider
and no Ollama installed, this reads as a stream of connector chatter before
the user ever types a prompt — none of it is actionable, and the issue asks
for it to be quiet by default.

Separately, `try_build_registration` in each provider shim (e.g.
`crates/otto/src/plugin/builtin/provider_anthropic/mod.rs:147-224`) calls
`build_dynamic_caps` (`crates/otto/src/plugin/builtin/provider_common.rs:183-216`),
which calls `client.list_models()` to fetch the live model catalog. If that
call fails for **any** reason — including an invalid/revoked key, no billing
credit, or a rate limit — the failure is swallowed into a `list-models-fell-back`
note and the provider is still registered with the static capability
fallback, active and apparently healthy. The header then shows
`▸ anthropic/claude-opus-4-6 — Default` even though the key can't actually
complete a turn. The first real prompt fails with the raw upstream string
passed through `HostError::Provider`'s `Display` impl
(`crates/otto-host/src/session.rs:135-137`: `"provider error: {kind:?}: {message}"`),
with no indication that the message originates from the provider's own API,
not from Otto.

## Approach

### 0. Fix `ErrorKind` misclassification in the `list_models` path (Gemini, OpenAI)

Prerequisite for step 1: `crates/provider-anthropic/src/models.rs` already
maps HTTP status codes to `ErrorKind` correctly for `list_models` via its
existing `pub(crate) fn status_to_error_kind(status: u16) -> ErrorKind`
(`crates/provider-anthropic/src/lib.rs:191-202`), shared with the `complete`
error path. **Gemini and OpenAI do not do this for `list_models`**:

- `crates/provider-gemini/src/models.rs:61-71` hardcodes
  `ErrorKind::Network` for every non-success HTTP status.
- `crates/provider-openai/src/lib.rs:241-259`'s `http_status_error` helper
  (used only by `list_models`) also hardcodes `ErrorKind::Network`, even
  though the same file's `parse_error_response` (used by `complete`,
  lines 204-224 in gemini / an equivalent block in openai) already has the
  correct status→kind match arms inline.

Fix: extract each crate's inline status→kind match block into its own
`pub(crate) fn status_to_error_kind(status: u16) -> ErrorKind` (mirroring
Anthropic's existing helper name/shape exactly), and call it from both the
`complete`-path error builder and the `list_models`-path error builder in
each crate. Without this fix, step 1's `Network`-carve-out would silently
swallow a 401/403/429 from Gemini or OpenAI's model-catalog endpoint as
"transient," defeating the whole point of this change for two of the four
providers. Add/adjust unit tests in each crate's `models.rs`/`lib.rs` so a
mocked 401 response asserts `ErrorKind::Authentication` (matching Anthropic's
existing `list_models_propagates_http_failure`-style coverage), not
`ErrorKind::Network`.

### 1. Validate the key as part of connect-time capability discovery

`list_models()` already returns a structured `otto_protocol::ErrorKind`
(`crates/otto-protocol/src/error.rs`) distinguishing `Authentication`,
`PermissionDenied`, `RateLimited`, `InvalidRequest`, `Overloaded`,
`ContextLengthExceeded`, `ModelNotFound`, `Refusal`, `Internal`, `Network`,
and `NotImplemented`. Today `build_dynamic_caps` treats every `Err` the same
way: log + fall back to the static catalog.

Split that handling:

- `NotImplemented` — provider genuinely has no `list_models` endpoint (e.g. a
  future provider). Unchanged: fall back to the static catalog, register
  normally, no note.
- `Network` — treated as a transient connectivity blip (DNS hiccup, proxy
  timeout, Ollama not running yet) rather than a credential problem.
  Unchanged: fall back to the static catalog, register normally, log at
  `tracing::warn`. This carve-out is only trustworthy once step 0 lands,
  since it now means "genuinely couldn't reach the endpoint," not "got an
  error response we didn't bother to classify."
- Every other kind (`Authentication`, `PermissionDenied`, `RateLimited`,
  `InvalidRequest`, `Overloaded`, `ContextLengthExceeded`, `ModelNotFound`,
  `Refusal`, `Internal`) — treated as "the stored key can't currently
  complete a request." `build_dynamic_caps` returns a new
  `DynamicCapsOutcome::Rejected(String)` variant instead of a capability
  set, carrying a human-readable reason.

This changes `build_dynamic_caps`'s return type from
`(ProviderCapabilities, Option<String>)` to an enum:

```rust
pub(crate) enum DynamicCapsOutcome {
    Ready(ProviderCapabilities, Option<String>), // capabilities + optional fallback note
    Rejected(String),                             // provider unusable; human-readable reason
}
```

`try_build_registration` in each of the four provider shims changes its
return type from `Result<Option<(ProviderRegistration, Option<String>)>, String>`
to `Result<ProviderBuildOutcome, String>`:

```rust
pub(crate) enum ProviderBuildOutcome {
    /// No key in the keyring and none in the environment fallback.
    NoCredentials,
    /// A key was found but the provider rejected it (or a rate limit/quota
    /// error) when validating via `list_models`.
    Rejected(String),
    /// A working client was built and (optionally) the live catalog note.
    Ready(ProviderRegistration, Option<String>),
}
```

This new three-way outcome is necessary because `try_build_registration` is
called from **three** places with different messaging needs — the startup
`try_provider!` macro (`crates/otto/src/main.rs:536-577`), `perform_connect`
(`crates/otto/src/main.rs:2442-2506`), and `apply_pending_pool_add`
(`crates/otto/src/main.rs:1681-1753`) — and the existing two-way
`Ok(Some(...))`/`Ok(None)` shape cannot distinguish "no credentials at all"
(where `perform_connect`'s existing `notes.connect-keyring-not-found`
message is correct — it means the key that was just saved couldn't be read
back) from "credentials present but rejected" (where that message would be
actively misleading, claiming a just-saved key is missing rather than
reporting the real rejection reason).

Call-site handling of the new outcome:

- **Startup (`try_provider!`)**: `NoCredentials` → unchanged, no note (user
  can `/connect` later). `Rejected(reason)` → do **not** push the provider
  into `providers`; `tracing::warn!` the reason; push a
  `notes.startup-build-failed`-shaped note into `deferred_notes` gated by
  `config_file.startup.verbose` (see step 2). `Ready(reg, note)` → unchanged.
- **`perform_connect`** (explicit `/connect`, key just saved): `NoCredentials`
  → unchanged existing `notes.connect-keyring-not-found` message.
  `Rejected(reason)` → push `notes.connect-failed` (existing locale key,
  `"Connect to %{id} failed: %{err}"`) with `err = reason`, unconditionally
  (not gated by `verbose` — this is a direct response to a user-initiated
  `/connect`, not startup chatter). `Ready(reg, note)` → unchanged.
- **`apply_pending_pool_add`** (async pool-add after `Effect::RegisterProvider`):
  same three-way handling as `perform_connect`, reusing the same
  `notes.connect-failed` message for `Rejected`.

This directly answers the issue's "check for a key first ... and validate
it": a present-but-rejected key now behaves like a missing key for pool
membership (never falsely marked active), while still telling the user
*why* in both the startup-verbose and explicit-`/connect` cases.

### 1a. Recognize an environment-variable key, not just a keyring entry

Issue #14 asks to "confirm an API key is available (keyring **or env**)."
Today each cloud shim's `try_build_registration` (e.g.
`provider_anthropic/mod.rs:147-224`) calls `crate::creds::load(PROVIDER_ID)`
and returns `Ok(None)` immediately when the keyring has no entry — even
though the underlying provider builders (`AnthropicProvider::builder()`,
`GeminiProvider::builder()`, `OpenAiProvider::builder()`) already fall back
to `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`/`GOOGLE_API_KEY`, and
`OPENAI_API_KEY` respectively when `.api_key(..)` is never called
(`provider-anthropic/src/lib.rs:96-98`, `provider-gemini/src/lib.rs:99-100`,
`provider-openai/src/lib.rs:100`). Because the shim always short-circuits on
`creds::load` returning `None`, that env-var fallback is currently dead code
from the TUI's perspective — an env-var-only setup never gets past
`NoCredentials`.

Fix (the three cloud shims only; `provider_local` has no API key concept):
when `creds::load(PROVIDER_ID)` returns `Ok(None)`, check
`std::env::var(<VENDOR_ENV_VAR>).is_ok()` (the same var name the builder
itself already reads) before concluding `NoCredentials`. If the env var is
set, proceed to `.build()` **without** calling `.api_key(..)` so the
builder's own existing fallback resolves it — this avoids duplicating the
vendor's env-var-name knowledge in the TUI shim beyond a presence check.
`try_connect_from_keyring` (the synchronous `/connect`-modal path) is
unaffected — it is about *reading a stored key back after saving it*, not
about auto-connect discovery, and is out of scope here.

### 2. Quiet the startup path by default

### 2. Quiet the startup path by default

Per-provider auto-connect notes (`notes.startup-build-failed`,
`notes.startup-timeout`, `notes.list-models-fell-back`,
`notes.list-models-empty`, and the new rejected-key case) stop being pushed
into `deferred_notes` unconditionally. Instead:

- They are always logged via `tracing::warn` (already the case for most of
  these call sites; the rejected-key case above adds one more).
- They are pushed into `deferred_notes` (and thus shown in the transcript)
  only when `config_file.startup.verbose` is `true` — a new `bool` field on
  `StartupSection` (`crates/otto/src/config_file.rs`), defaulting to `false`.
  This is an additive config key (`[startup] verbose = true`), not a
  breaking change to the on-disk config format.
- If, after trying every provider, `providers.is_empty()`, the existing
  `notes.not-connected-startup` message ("Not connected. Type / and pick
  /connect to set up a provider.") is still shown unconditionally — that is
  the one message the issue says a user always needs.

This satisfies "the only startup message a user needs is 'not connected —
run /connect' when nothing is available" while keeping a debugging path
(`verbose = true`) for anyone who wants to see every connector attempt.

### 3. Attribute turn-time provider errors to the provider

`HostError::Provider(ProviderError)` today discards which provider produced
the error by the time it reaches `Display`. Fix this at the source:

- Add a `display_name` accessor to `otto_host::pool::ProviderLease`
  (populated from the owning `PoolEntry::display_name` in `PoolEntry::lease`),
  mirroring the existing `PoolEntry::display_name()` accessor.
- In `Host::run_turn_streaming`'s iteration loop (`crates/otto-host/src/session.rs`,
  around the `resp_result.map_err(HostError::Provider)?` call at line 1288),
  capture the lease's `display_name` into a local `String` *before* the
  existing `drop(lease)` a few lines above, then change the error map to
  `HostError::Provider { name: provider_display_name, error: e }`.
- Change the `HostError::Provider` variant from a tuple variant to a struct
  variant `Provider { name: String, error: ProviderError }` and update its
  `Display` impl to
  `"{name} rejected the request: {message}"` (using `error.message`), so the
  TUI's existing `Entry::Note(format!("Error: {msg}"))` rendering
  (`crates/otto/src/main.rs:3230`) now shows e.g. `Error: Anthropic rejected
  the request: Your credit balance is too low to access the Anthropic API...`
  — the upstream text is still shown verbatim (per the issue: "that ...
  sentence is Anthropic's own API error text passed through verbatim"), but
  it is now unambiguously attributed to the provider, not to Otto.
  `HostError` is `#[non_exhaustive]`, so external `match` arms already
  require a wildcard and are unaffected by the variant shape change; the
  only constructors of `HostError::Provider` live inside `otto-host` itself
  (grep confirms this) and are all updated as part of this task.

## Scope

**In:**
- `crates/provider-gemini/src/models.rs`, `crates/provider-gemini/src/lib.rs`
  — extract `status_to_error_kind`, use it for both `complete` and
  `list_models` error paths.
- `crates/provider-openai/src/lib.rs` — same extraction/reuse for OpenAI.
- `crates/otto/src/plugin/builtin/provider_common.rs` — `build_dynamic_caps`
  return type + kind-based branching (`DynamicCapsOutcome`).
- `crates/otto/src/plugin/builtin/provider_{anthropic,gemini,openai,local}/mod.rs`
  — `try_build_registration` changes to `ProviderBuildOutcome`; the three
  cloud shims additionally gain the env-var-presence check from step 1a.
- `crates/otto/src/main.rs` — `bootstrap_pool_host`'s `try_provider!` macro
  (gate notes behind `config_file.startup.verbose`), `perform_connect`, and
  `apply_pending_pool_add` (both updated to match on `ProviderBuildOutcome`
  and surface `Rejected` via the existing `notes.connect-failed` key).
- `crates/otto/src/config_file.rs` — new `StartupSection::verbose: bool`
  field (default `false`), parsed/serialized like the existing
  `connect_timeout_ms` field.
- `crates/otto-host/src/pool.rs` — `ProviderLease::display_name()` accessor.
- `crates/otto-host/src/session.rs` — `HostError::Provider` variant shape +
  `Display` impl; the one call site that constructs it.
- `crates/otto/locales/{en,es,hi,pt}.toml` — no new keys are required
  (`notes.connect-failed` and the existing startup-note keys are reused
  verbatim, only the conditions under which they're used change); README.md
  gets a one-line mention of the new `[startup] verbose` config key.

**Out:**
- Changing `ErrorKind`'s taxonomy itself — only fixing two providers' failure
  to use the taxonomy correctly for one endpoint (`list_models`).
- A CLI flag or env var for verbose startup (config-file only, per the
  issue's "and/or behind a verbose flag" — the config key is the simpler,
  already-precedented mechanism `StartupSection` uses for other startup
  knobs).
- Retrying a rejected key automatically, or any new `/connect` UX beyond what
  already exists (`/connect <provider> --rekey`).
- Non-startup, non-connect error attribution (e.g. `/model` switch
  failures) — only the turn-time `HostError::Provider` path named in the
  issue, plus the `/connect`/pool-add messaging needed to keep
  `ProviderBuildOutcome::Rejected` from being silently dropped there.
- Changing `try_connect_from_keyring` (the synchronous `/connect`-modal
  read-back path) — env-var recognition is added only to the async
  `try_build_registration` auto-discovery path per step 1a.

## Public-interface changes

- **Additive:** `[startup] verbose` is a new, optional config-file key
  (`~/.otto/config.toml`), defaulting to `false` — no migration needed,
  matches Non-Negotiable Rule 6's additive carve-out.
- **Not a governed public interface, but noted for reviewers:** the
  `otto_host::HostError::Provider` enum variant changes shape from a tuple
  `Provider(ProviderError)` to a struct `Provider { name: String, error:
  ProviderError }`. `HostError` is not one of Rule 6's named surfaces (SPP
  wire format, tool MCP schema, plugin ABI, slash command, env var, on-disk
  format) — it is an internal Rust error type local to `otto-host`, with no
  external plugin-facing contract. All in-tree constructors are updated in
  this same change.
- No change to the SPP wire format, any tool's MCP schema, the plugin ABI, a
  slash command, or the on-disk transcript/keyring format.

## Assumptions

- **`Network`-kind `list_models` failures still register the provider with
  the static fallback catalog** (unchanged from today), rather than being
  treated as a rejected key. Rationale: a transient DNS/proxy blip is not
  evidence the *key* is bad, and the issue's repro and desired-behavior list
  (`bad key, no credit, rate-limited, org disabled`) are all
  authentication/billing/quota conditions, not raw connectivity failures.
  This keeps Ollama's "might simply not be running yet" resilience
  (`provider_local/mod.rs:97-100`) intact for its `list_models` path too.
  This carve-out is only safe once step 0's classification fix lands for
  Gemini/OpenAI, otherwise their 401s were masquerading as `Network`.
- **The verbose toggle is a config-file key, not an env var or CLI flag.**
  `StartupSection` already owns `connect_timeout_ms` and `startup_providers`
  as the home for startup-tuning knobs; adding `verbose` there is the
  smallest, most consistent surface. The issue says "and/or", so a config
  key alone satisfies it.
- **`notes.not-connected-startup` remains unconditional.** It is the single
  message the issue explicitly calls the one thing a user always needs; it
  is not gated behind `verbose`.
- **No new locale keys.** The existing `notes.startup-build-failed`,
  `notes.startup-timeout`, `notes.connect-failed`, `notes.list-models-fell-back`,
  `notes.list-models-empty` keys are reused verbatim; only the condition
  under which they're used changes. This avoids a 4-locale-file churn for
  text that doesn't change.
- **`HostError::Provider`'s struct-variant change is safe** because
  `HostError` is `#[non_exhaustive]` (forcing external `match` arms to carry
  a wildcard already) and the crate has no external plugin-facing
  constructor of this variant — confirmed via `grep -rn "HostError::Provider"`
  returning only the one call site in `session.rs` plus its own tests.
- **Env-var recognition only widens `try_build_registration`'s auto-discovery
  path, not `try_connect_from_keyring`.** The latter is the synchronous path
  used by the `/connect` modal's "try the stored key silently before opening
  the modal" shortcut; it already requires the modal flow (which asks for
  and saves a key) as its fallback, so leaving it keyring-only does not
  regress any existing behavior — only startup/pool-rebuild auto-discovery
  gains env-var recognition, matching the issue's "on launch" framing.

## Goal & Success Criteria

Make provider auto-connect at startup silent by default (except for the
"nothing connected" case), detect a present-but-rejected key at connect time
instead of registering a falsely-healthy provider, and make any turn-time
provider failure name the provider that produced it.

- [ ] Launching with a valid, working key and no other providers configured
      produces no startup notes in the transcript.
- [ ] Launching with an Anthropic key that has no billing credit (or is
      otherwise rejected by `list_models`) does **not** register Anthropic as
      the active provider; the header shows disconnected /
      `not-connected-startup`, and a `tracing::warn` log records the reason.
- [ ] The same rejection, from a Gemini or OpenAI key, is also detected (not
      masked as `ErrorKind::Network`) after the step-0 classification fix.
- [ ] Running `/connect anthropic` (or gemini/openai) with a key that gets
      rejected by `list_models` shows `Connect to anthropic failed: <reason>`
      instead of the misleading "key was saved but couldn't be read back"
      message.
- [ ] Setting `ANTHROPIC_API_KEY` (or `GEMINI_API_KEY`/`GOOGLE_API_KEY`,
      `OPENAI_API_KEY`) with no corresponding keyring entry results in that
      provider auto-connecting at startup.
- [ ] Setting `[startup] verbose = true` in `~/.otto/config.toml` restores
      the previous per-provider chatter in the transcript.
- [ ] A turn-time provider failure renders as
      `Error: <Provider display name> rejected the request: <upstream message>`
      instead of `Error: provider error: <Kind>: <message>`.
- [ ] `cargo test --workspace` and `cargo clippy --workspace --all-targets`
      stay green.

## Error Handling & Edge Cases

- A provider whose `list_models` call times out at the connect-time
  `tokio::time::timeout` wrapper (the outer timeout in `try_provider!`, not
  `list_models` itself) is unaffected by this change — that path already
  produces `notes.startup-timeout` and is now just gated behind `verbose`
  like the others.
- If `list_models` is rejected but the provider is later fixed (user tops up
  credit) and runs `/connect <id> --rekey`, the existing `/connect` flow now
  correctly surfaces success via `Ready`, or the real rejection reason via
  `Rejected`, rather than the previous silent-fallback-then-fail-on-first-prompt
  behavior.
- Local/Ollama: a `Network`-kind failure from `list_models` (Ollama not
  running yet) still falls back to the static single-entry placeholder and
  registers as before — unchanged from today, per the `Network`-kind carve-out
  above. `provider_local` has no API key, so step 1a's env-var recognition
  does not apply to it.
- `perform_connect`'s `NoCredentials` arm can now only happen after
  `creds::save` succeeded but the immediate read-back (keyring **and** the
  env-var check) both come up empty — an even narrower "backend flake"
  window than today, so the existing `notes.connect-keyring-not-found`
  message remains accurate for it.

## Risks & Open Questions

- The `DynamicCapsOutcome::Rejected` carve-out for `Network` errors means a
  systemic outage at the vendor (e.g. a 5xx mapped to `Overloaded`, not
  `Network`) will still block registration rather than falling back — this
  matches the issue's explicit inclusion of "rate-limited" as a
  should-be-detected condition, but is worth calling out: a temporarily
  overloaded-but-otherwise-valid key will show as disconnected until the
  next auto-connect (app restart) or explicit `/connect --rekey`, rather than
  registering with stale-but-usable capabilities. Accepted per the issue's
  explicit ask; flagged here for reviewer visibility.
- Extracting `status_to_error_kind` in Gemini/OpenAI touches each crate's
  `complete`-path error construction indirectly (same function, two call
  sites) — the refactor must be behavior-preserving for `complete` (identical
  match arms, just deduplicated), verified by each crate's existing
  `complete`-path error-kind tests staying green.

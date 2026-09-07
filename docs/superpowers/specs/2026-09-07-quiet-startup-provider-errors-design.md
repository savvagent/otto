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
  timeout) rather than a credential problem. Unchanged: fall back to the
  static catalog, register normally, log at `tracing::warn`.
- Every other kind (`Authentication`, `PermissionDenied`, `RateLimited`,
  `InvalidRequest`, `Overloaded`, `ContextLengthExceeded`, `ModelNotFound`,
  `Refusal`, `Internal`) — treated as "the stored key can't currently
  complete a request." `build_dynamic_caps` returns a new
  `DynamicCapsOutcome::Rejected { reason }` variant instead of a capability
  set. Callers propagate this up through `try_build_registration` as
  `Ok(None)` (same shape as "no credentials stored" — the provider is simply
  not registered) plus a `tracing::warn` log carrying the reason. This
  directly answers the issue's "check for a key first ... and validate it":
  a present-but-rejected key now behaves like a missing key at startup
  instead of silently registering a broken provider.

This changes `build_dynamic_caps`'s return type from
`(ProviderCapabilities, Option<String>)` to an enum:

```rust
pub(crate) enum DynamicCapsOutcome {
    Ready(ProviderCapabilities, Option<String>), // capabilities + optional fallback note
    Rejected(String),                             // provider unusable; reason for the tracing log
}
```

All four call sites (`provider_anthropic`, `provider_gemini`, `provider_openai`,
`provider_local`'s `try_build_registration`) match on this and return
`Ok(None)` on `Rejected`, mirroring the existing "no credentials" early
return.

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
- `crates/otto/src/plugin/builtin/provider_common.rs` — `build_dynamic_caps`
  return type + kind-based branching.
- `crates/otto/src/plugin/builtin/provider_{anthropic,gemini,openai,local}/mod.rs`
  — `try_build_registration` call-site updates to match the new
  `DynamicCapsOutcome`.
- `crates/otto/src/main.rs` — `bootstrap_pool_host`'s `try_provider!` macro:
  gate `deferred_notes.push(...)` behind `config_file.startup.verbose`.
- `crates/otto/src/config_file.rs` — new `StartupSection::verbose: bool`
  field (default `false`), parsed/serialized like the existing
  `connect_timeout_ms` field.
- `crates/otto-host/src/pool.rs` — `ProviderLease::display_name()` accessor.
- `crates/otto-host/src/session.rs` — `HostError::Provider` variant shape +
  `Display` impl; the one call site that constructs it.
- `crates/otto/locales/{en,es,hi,pt}.toml` — no new keys are required (existing
  `notes.startup-build-failed` etc. keys are reused, only their default
  visibility changes); README.md gets a one-line mention of the new
  `[startup] verbose` config key.

**Out:**
- Changing `ErrorKind` itself, or how individual provider crates
  (`provider-anthropic`, `provider-gemini`, `provider-openai`, `provider-local`)
  map upstream HTTP statuses to `ErrorKind` — the existing taxonomy already
  has everything this design needs.
- A CLI flag or env var for verbose startup (config-file only, per the
  issue's "and/or behind a verbose flag" — the config key is the simpler,
  already-precedented mechanism `StartupSection` uses for other startup
  knobs).
- Retrying a rejected key automatically, or any new `/connect` UX beyond what
  already exists (`/connect <provider> --rekey`).
- Non-startup error attribution (e.g. `/model` switch failures) — only the
  turn-time `HostError::Provider` path named in the issue.

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
- **The verbose toggle is a config-file key, not an env var or CLI flag.**
  `StartupSection` already owns `connect_timeout_ms` and `startup_providers`
  as the home for startup-tuning knobs; adding `verbose` there is the
  smallest, most consistent surface. The issue says "and/or", so a config
  key alone satisfies it.
- **`notes.not-connected-startup` remains unconditional.** It is the single
  message the issue explicitly calls the one thing a user always needs; it
  is not gated behind `verbose`.
- **No new locale keys.** The existing `notes.startup-build-failed`,
  `notes.startup-timeout`, `notes.list-models-fell-back`,
  `notes.list-models-empty` keys are reused verbatim; only the condition
  under which they reach `deferred_notes` changes. This avoids a 4-locale-file
  churn for text that doesn't change.
- **`HostError::Provider`'s struct-variant change is safe** because
  `HostError` is `#[non_exhaustive]` (forcing external `match` arms to carry
  a wildcard already) and the crate has no external plugin-facing
  constructor of this variant — confirmed via `grep -rn "HostError::Provider"`
  returning only the one call site in `session.rs` plus its own tests.

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
  credit) and runs `/connect <id> --rekey`, the existing `/connect` flow
  (`handle_slash` in each provider shim) is unaffected — it already calls
  `try_connect_from_keyring`/an equivalent explicit path, not
  `try_build_registration`; this change only touches the startup
  auto-connect path.
- Local/Ollama: a `Network`-kind failure from `list_models` (Ollama not
  running yet) still falls back to the static single-entry placeholder and
  registers as before — unchanged from today, per the `Network`-kind carve-out
  above.

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

# Afford a path back to a valid API key after a rejected connect — design

Date: 2026-09-08
Status: pending review
Related: `savvagent/otto#81`

## Problem

When a user connects a provider with an incorrect API key, Otto detects the
failure but leaves the user with no way to fix it short of already knowing
about the undocumented-in-the-UI `--rekey` flag (`README.md:145`,
`crates/otto/src/plugin/builtin/connect/screen.rs`'s `Alt+Enter`
shortcut). Concretely, two call sites swallow a rejected/erroring
`ProviderBuildOutcome` into a single dead-end note:

- `perform_connect` (`crates/otto/src/main.rs:2627-2820`) — reached both from
  the first-time modal-driven key entry (`InputMode::EnteringApiKey`'s
  `KeyCode::Enter` arm, `crates/otto/src/main.rs:4002-4009`) and from the
  provider-selector's stored-key shortcut
  (`handle_provider_selector_key`/`submit_selected_provider`,
  `crates/otto/src/main.rs:4196-4227`). On
  `ProviderBuildOutcome::Rejected(reason)` or a build `Err`, it pushes
  `notes.connect-failed` (`"Connect to %{id} failed: %{err}"`,
  `crates/otto/locales/en.toml:18`) and returns — `app.input_mode` is
  already `Editing` (set by the caller before `perform_connect` runs), so
  the user lands back at the normal prompt with no indication of what to do
  next.
- `apply_pending_pool_add` (`crates/otto/src/main.rs:1832-1935`) — reached
  from the silent stored-key reconnect path
  (`Effect::RegisterProvider` → `app.pending_pool_add`, after a provider
  plugin's `handle_slash`/`HostStarting` hook found a keyring entry and
  optimistically registered it). On the same two outcomes it does the
  identical push-note-and-return, gated by `show_notes` (unconditionally
  shown for the non-startup case this issue is about).

Both call sites already know `spec.id` and the human-readable rejection
`reason` at the point they give up — the information needed to tell the user
exactly how to fix it is on hand; it just isn't surfaced.

A third path has the same shape at turn time, not connect time. A key can
pass `perform_connect`'s validation, then later expire, get revoked, or run
out of credit — the very first prompt sent through it fails with
`HostError::Provider { name, error }` where `error.kind ==
ErrorKind::Authentication` (`crates/otto-protocol/src/error.rs:36`,
`crates/otto-host/src/session.rs:141-150`). The TUI's turn-error handler
(`WorkerMsg::Error(msg)` arm, `crates/otto/src/main.rs:3428-3479`) renders
this as a single `Entry::Note(format!("Error: {msg}"))` — e.g. `"Error:
Anthropic rejected the request: invalid x-api-key"` — with no next step,
same dead end as the connect-time case.

The existing `--rekey` mechanism (`/connect <id> --rekey`, or `Alt+Enter` on
the provider picker) already does exactly what's needed — it force-opens
the masked API-key modal even when a (bad) key is already stored
(`crates/otto/src/plugin/builtin/provider_anthropic/mod.rs:281-290` and the
matching Gemini/OpenAI/DeepSeek shims). The gap is discoverability at the
exact moment the user needs it: none of the three failure sites above
mention it. A prior design (`docs/superpowers/specs/2026-09-07-quiet-startup-provider-errors-design.md`,
`savvagent/otto#14`) deliberately scoped "any new `/connect` UX beyond what
already exists (`--rekey`)" as **Out**; this change stays inside that
boundary by pointing at the existing mechanism, not inventing a new one.

## Approach

### 1. Make the connect-time rejection note actionable — only for a keyed rejection

The `--rekey` hint only makes sense for a **credential** rejection on a
**keyed** provider — `ProviderBuildOutcome::Rejected(reason)` (the key was
read back and a real request rejected it) on a provider whose
`spec.api_key_required` is `true`. It does *not* make sense for:

- The generic build `Err(e)` arm at both call sites (`perform_connect`,
  `apply_pending_pool_add`) — this covers TLS/proxy init failures, fd
  exhaustion, or any other non-credential build error; telling the user to
  re-enter a key would not fix those and is actively misleading.
- `local`/Ollama's `Rejected` case (its `spec.api_key_required` is `false`,
  `crates/otto/src/providers.rs:90`) — a `list_models` rejection there means
  "Ollama isn't reachable," not "the key is wrong" (`local` has no key);
  `--rekey` opens a modal with nothing useful to enter.

So, unlike a pure locale-only change, both call sites need a small,
mechanical branch added: when `ProviderBuildOutcome::Rejected(reason)` and
`spec.api_key_required`, use a new locale key; otherwise keep using the
existing `notes.connect-failed` unchanged. Add:

```
notes.connect-rejected-keyed = "Connect to %{id} failed: %{err} Run /connect %{id} --rekey to try a different key."
```

(all four locales), and at each `Ok(ProviderBuildOutcome::Rejected(reason))`
arm in `perform_connect` and `apply_pending_pool_add`, change:

```rust
app.push_note(rust_i18n::t!("notes.connect-failed", id = spec.id, err = reason).to_string());
```

to:

```rust
let key = if spec.api_key_required {
    "notes.connect-rejected-keyed"
} else {
    "notes.connect-failed"
};
app.push_note(rust_i18n::t!(key, id = spec.id, err = reason).to_string());
```

The generic `Err(e)` arms at both call sites are unchanged — they keep
using `notes.connect-failed` verbatim, for every provider, since a build
error there is never known to be credential-shaped. This is a narrow,
additive branch (one new locale key, one `if` at two existing call sites)
that does not otherwise touch either function's control flow — no new
returns, no modal reopen, no change to what gets registered or when.

### 2. Surface the same hint after a turn-time authentication failure

Add a new locale key, `notes.turn-auth-failed-hint`:

```
Run /connect %{id} --rekey to enter a different API key for %{name}.
```

In the `WorkerMsg::Error` handling in `crates/otto/src/main.rs` (the
`run_app` event loop), after the existing `Entry::Note(format!("Error:
{msg}"))` push, detect whether the error that produced this `WorkerMsg`
was a `HostError::Provider { name, error }` with `error.kind ==
ErrorKind::Authentication`, and if so push the hint note naming the
provider.

This requires threading the classification from the point the error is
first observed (where it is still a typed `HostError`, not yet a `String`)
through to the `WorkerMsg` the main loop matches on, because
`WorkerMsg::Error` today only carries a `String` (the error's `Display`
output) — the `ErrorKind` is discarded before it reaches the handler. Add a
new `WorkerMsg` variant carrying the two pieces of information the hint
needs:

```rust
/// Sent instead of `Error` when `run_turn_streaming` failed specifically
/// with `HostError::Provider { error.kind: Authentication, .. }`, so the
/// main loop can additionally point the user at `--rekey`.
TurnAuthError {
    /// `Display` output of the underlying `HostError` — rendered exactly
    /// like `Error(String)` is today, so the "Error: ..." line is
    /// unchanged.
    message: String,
    /// The provider's display name (`HostError::Provider::name`), used
    /// only for the hint note's `%{name}` interpolation.
    provider_display_name: String,
},
```

At the turn-runner's error-handling site (`crates/otto/src/main.rs`, inside
the `tokio::spawn` block that awaits `runner` and currently does
`Ok(Err(e)) => { let _ = tx.send(WorkerMsg::Error(e.to_string())).await; }`),
match on `&e` first:

```rust
Ok(Err(e)) => {
    let msg = e.to_string();
    let is_auth = matches!(
        &e,
        otto_host::HostError::Provider { error, .. }
            if error.kind == otto_protocol::ErrorKind::Authentication
    );
    let sent = if is_auth {
        if let otto_host::HostError::Provider { name, .. } = &e {
            WorkerMsg::TurnAuthError {
                message: msg,
                provider_display_name: name.clone(),
            }
        } else {
            unreachable!("is_auth only true for HostError::Provider")
        }
    } else {
        WorkerMsg::Error(msg)
    };
    let _ = tx.send(sent).await;
}
```

(The plan writes this as a small helper function instead of the inline
`unreachable!` shape above — see the plan's Task 2 for the exact,
clippy-clean form; this section documents the behavior, not the final
syntax.)

**Identifying the provider a routed turn actually used.** `app.active_provider_id`
is *not* a safe stand-in for "the provider this turn ran against": `Host`'s
router (`crates/otto-host/src/session.rs`, around line 1033) can pick a
**different** pool entry than the active one for a given turn — an
`@`-override, modality routing (an image-bearing prompt redirected to a
vision-capable provider), or a `routing.toml` rule. The router always emits
`TurnEvent::RouteSelected { provider_id, model_id, reason }` before the
first `IterationStarted` (`crates/otto-host/src/session.rs:1029-1036`,
unconditionally, whether or not any routing rule actually redirected
anything), so the TUI already receives the authoritative per-turn provider
id — it just isn't retained anywhere today
(`app.apply_turn_event`'s `RouteSelected` arm, `crates/otto/src/app.rs:1114-1122`,
only pushes a `RouteBadge` entry and discards `provider_id`).

Add a new local in `run_app`'s event loop, alongside the existing
`current_turn_id`/`footer_pending_turn_id`/`turn_terminal_event_seen` locals
(`crates/otto/src/main.rs:3223-3226`):

```rust
let mut current_turn_provider_id: Option<otto_protocol::ProviderId> = None;
```

Capture it from `&e` in the `WorkerMsg::Event(e)` arm, in the same spot the
existing `html_block_stop_id` capture already reads `&e` before
`app.apply_turn_event(e)` consumes it by value (`crates/otto/src/main.rs`,
just above the `apply_turn_event` call):

```rust
if let TurnEvent::RouteSelected { provider_id, .. } = &e {
    current_turn_provider_id = Some(provider_id.clone());
}
```

Reset it to `None` whenever a turn reaches a terminal state — the same
points that already reset `turn_terminal_event_seen`/`current_turn_id`
(`TurnComplete`, `Cancelled`, `AbortedAfterGrace`, and the `WorkerMsg::Error`/
`WorkerMsg::TurnAuthError` arms themselves, after use) — so a stale id from
a previous turn can never leak into a later one that errors before its own
`RouteSelected` fires (e.g. `NoActiveProvider`, which the hint's `spec`
lookup below naturally no-ops on anyway since that `HostError` variant is
never `Provider`, so it never reaches this branch — but resetting keeps the
invariant explicit rather than incidental).

In the main loop, add a `WorkerMsg::TurnAuthError { message, provider_display_name }`
arm that performs the *exact same* turn-bookkeeping the existing
`WorkerMsg::Error(msg)` arm does (the `is_loading` reset, the synthetic
`TurnStart`/`TurnEnd { success: false }` dance, `turn_terminal_event_seen`
reset — see `crates/otto/src/main.rs:3428-3479`) using `message` in place of
`msg`, and then additionally:

1. Look up the provider id to interpolate into the hint via
   `current_turn_provider_id` (captured above), then match that against
   `crate::providers::effective_providers()` (the same catalog
   `apply_pending_pool_add` already uses) to get the `ProviderSpec`. Using
   the per-turn routed id — not `app.active_provider_id` — is what makes
   this correct when routing redirected the turn to a non-active provider.
   `provider_display_name` (from `HostError::Provider::name`) is used only
   for the hint text's `%{name}`, never for identifying which provider to
   rekey — display names aren't guaranteed unique across custom/future
   providers, but the routed `ProviderId` is authoritative.
2. If `current_turn_provider_id` is `None` (defensive — should not happen
   given `RouteSelected` fires unconditionally before any provider call, but
   the field starts `None` and a hostile/future host implementation could
   theoretically skip it) or the id doesn't resolve to a spec in
   `effective_providers()`, or `spec.api_key_required` is `false`: skip the
   hint silently — the "Error: ..." note alone still fires, so no
   information is lost, just the actionable follow-up. Otherwise push
   `notes.turn-auth-failed-hint` with `id = spec.id, name =
   provider_display_name`.

To avoid duplicating the ~50-line turn-bookkeeping block, refactor it into a
private helper (e.g. `fn record_turn_error(app: &mut App, message: String,
footer_pending_turn_id: &mut Option<u64>, current_turn_id: &mut Option<u64>,
next_turn_id: &mut u64, turn_terminal_event_seen: &mut bool) -> impl
Future<...>` — exact signature decided during implementation to match
existing local-variable ownership in `run_app`) called from both the
`WorkerMsg::Error` and `WorkerMsg::TurnAuthError` arms.

### 3. The GUI front-end (`egui_app`) needs the identical treatment

`crates/otto/src/egui_app/mod.rs` is a second, independent front-end
(`OttoApp`) sharing the same `pub(crate) WorkerMsg` enum from `main.rs` — its
`handle_worker_msg` (`crates/otto/src/egui_app/mod.rs:193-316`) is an
exhaustive `match` over every `WorkerMsg` variant with no wildcard arm, and
its own turn-spawn site (`crates/otto/src/egui_app/mod.rs`, around line 411)
independently does `WorkerMsg::Error(e.to_string())` on a `run_turn_streaming`
error, mirroring `main.rs`'s spawn block byte-for-byte. Adding
`WorkerMsg::TurnAuthError` without updating this file does not compile
(non-exhaustive match) — this is not optional follow-on work, it is required
for step 2 to build at all.

Per the module's own doc comment ("A faithful port of the six `WorkerMsg`
arms in `run_app`'s ... block — same order, same leaf-helper calls, same
plugin dispatch"), apply the mirrored change:

- Add the same `current_turn_provider_id: Option<otto_protocol::ProviderId>`
  field to `OttoApp`'s turn-tracking state (alongside its existing
  `current_turn_id`/`next_turn_id` fields), captured from `TurnEvent::RouteSelected`
  in the `WorkerMsg::Event(e)` arm exactly as in `main.rs`.
- Change the turn-spawn site's error classification identically (the same
  `HostError::Provider { error.kind: Authentication, .. }` match, emitting
  `WorkerMsg::TurnAuthError` instead of `WorkerMsg::Error` when it matches).
- Add a `WorkerMsg::TurnAuthError { message, provider_display_name }` arm to
  `handle_worker_msg` that mirrors the existing `WorkerMsg::Error(msg)` arm's
  bookkeeping (already written in that function without the `main.rs`
  version's synthetic-turn-id `footer_pending_turn_id` concept — `egui_app`
  doesn't have that field, so mirror its existing simpler shape, not
  `main.rs`'s), then applies the identical `current_turn_provider_id` →
  `effective_providers()` → `notes.turn-auth-failed-hint` lookup.

No shared helper is introduced *across* `main.rs` and `egui_app/mod.rs` in
this change (they are already two independently-maintained "faithful port"
copies per existing house style in this file) — only *within* each file, per
Approach §2's `record_turn_error` extraction.

### Why not just reopen the modal automatically?

Auto-reopening `InputMode::EnteringApiKey` the instant a connect or turn
fails was considered and rejected:

- It would interrupt whatever the user was about to type next (a fresh
  prompt, an unrelated slash command) by yanking focus into a modal they
  didn't ask for — surprising for a failure that, especially at turn time,
  might be transient (e.g. a temporarily suspended key that comes back).
- It doesn't compose with multi-provider sessions: an auth failure on one
  pool entry shouldn't force a rekey interruption while the user is actively
  working with a different, healthy active provider in the same pool.
- The prior design (`savvagent/otto#14`) explicitly scoped new `/connect`
  UX beyond `--rekey` as **Out**; inventing an auto-modal here would
  contradict that recorded decision without revisiting it.

Pointing at the existing, already-tested `--rekey` mechanism gives the user
an explicit, opt-in next step without any of the above costs, and is a
strictly additive, low-risk change.

## Scope

**In:**
- `crates/otto/locales/{en,es,hi,pt}.toml` — add new key
  `notes.connect-rejected-keyed` (the `--rekey` hint for keyed-provider
  rejections); add new key `notes.turn-auth-failed-hint`.
  `notes.connect-failed` itself is unchanged text-wise.
- `crates/otto/src/main.rs` — the `if spec.api_key_required` locale-key
  selection at both `Rejected` arms (`perform_connect`,
  `apply_pending_pool_add`); new `WorkerMsg::TurnAuthError` variant; the
  turn-runner spawn block's error classification; the main loop's new match
  arm; the shared turn-bookkeeping helper extraction.
- `crates/otto/src/egui_app/mod.rs` — the mirrored `current_turn_provider_id`
  capture, turn-spawn error classification, and `WorkerMsg::TurnAuthError`
  arm in `handle_worker_msg` (Approach §3) — required for the crate to
  compile once `WorkerMsg::TurnAuthError` exists, and for the GUI front-end
  to get the same fix as the TUI.

**Out:**
- Auto-reopening the API-key modal on any failure path (see "Why not..."
  above).
- Any control-flow change to `perform_connect` or `apply_pending_pool_add`
  beyond the one `if spec.api_key_required { ... }` locale-key selection at
  their existing `Rejected` arms — no new branches, no modal reopen, no
  change to the generic `Err(e)` arms.
- Any change to `ErrorKind`, `ProviderBuildOutcome`, `HostError`, or the
  `--rekey` flag's existing implementation — all reused as-is.
- Deleting or auto-correcting a stored-bad key. The already-documented
  `creds::save`-before-validate ordering in `perform_connect` (a key that
  fails validation is still persisted) is a pre-existing, separate behavior
  not touched here; `--rekey` already provides the correction path.
- Non-authentication turn-time errors (rate limit, overloaded, refusal,
  etc.) — this change is scoped to `ErrorKind::Authentication` only, per
  the issue's specific complaint about a bad API key.

## Public-interface changes

None. `WorkerMsg` is a private (`pub(crate)`) enum internal to the `otto`
binary crate — not part of the SPP wire format, a tool's MCP schema, the
plugin ABI, the slash-command surface, or the on-disk transcript/keyring
format (Non-Negotiable Rule 6's governed surfaces). The two changed locale
strings are user-facing text, not a wire or schema contract.

## Assumptions

- **The per-turn routed provider id (`TurnEvent::RouteSelected`), not
  `app.active_provider_id`, identifies which provider to rekey.** Routing
  (an `@`-override, modality redirection, or a `routing.toml` rule) can
  select a different pool entry than the active one for any given turn;
  `RouteSelected` fires unconditionally before the first
  `IterationStarted` and is the authoritative source. Justified in Approach
  §2 above.
- **Gating the `--rekey` hint on `spec.api_key_required` at the `Rejected`
  arms (not appending it to the shared `notes.connect-failed` key
  unconditionally) is necessary, not just cosmetic.** `notes.connect-failed`
  is also used for the generic build `Err(e)` arm (host-construction/
  proxy/TLS failures unrelated to credentials) and would otherwise be
  reused for `local`/Ollama's keyless `Rejected` case too; a blanket
  `--rekey` hint on either would be actively misleading. A new key
  (`notes.connect-rejected-keyed`) selected only when both conditions hold
  keeps the existing `notes.connect-failed` text accurate for every other
  case, at the cost of one new locale key across 4 files and a single `if`
  at each of the two existing `Rejected` arms.
- **Non-native-speaker translations for es/hi/pt are acceptable for this
  fix**, consistent with how the existing locale files were evidently
  populated (short, literal phrases mirroring the English structure) — a
  native-speaker review pass, if desired, is a follow-up, not a blocker for
  this fix.
- **`crates/otto/src/egui_app/mod.rs`'s `handle_worker_msg` is an exhaustive
  match with no wildcard arm**, so it must gain a `WorkerMsg::TurnAuthError`
  arm in the same change or the crate fails to compile; this is treated as
  in-scope, required work (Approach §3), not optional GUI parity.

## Goal & Success Criteria

After this change, every path where Otto tells the user a provider
connection or turn failed due to a bad/rejected API key also tells them the
exact command to fix it, so the issue's "never affords the user the ability
to enter a valid API key" no longer applies to any of the three sites named
in Problem.

- [ ] `/connect anthropic` (or gemini/openai/deepseek) with a key rejected
      by `list_models` shows `Connect to anthropic failed: <reason> Run
      /connect anthropic --rekey to try a different key.` (the new
      `notes.connect-rejected-keyed` text) instead of the bare failure
      message. A generic build error (the `Err(e)` arm, any provider) still
      shows the unchanged `notes.connect-failed` text with no hint.
- [ ] `local`/Ollama's `Rejected` case (its `spec.api_key_required` is
      `false`) still shows the unchanged `notes.connect-failed` text with no
      `--rekey` hint, since re-entering a (nonexistent) key would not fix a
      connectivity problem.
- [ ] The same keyed-rejection hint appears when a stored key goes bad and is
      caught by the silent-reconnect path (`apply_pending_pool_add`'s
      `Rejected` arm) **outside of startup** — i.e. when
      `apply_pending_pool_add` is invoked with `startup: false` (the runtime
      `/connect`-adjacent call site, whose notes are already shown
      unconditionally today). The `startup: true` call site's existing
      quiet-by-default policy (`show_notes = !startup || app.startup_verbose`,
      `crates/otto/src/main.rs:1889`) is untouched by this change: a
      rejected key discovered silently at launch still produces no note at
      all unless `[startup] verbose = true` is set, matching
      `savvagent/otto#14`'s deliberate quiet-startup design. When it *is*
      shown (verbose startup, or the non-startup call site), the message
      picks up the same new `notes.connect-rejected-keyed` text
      automatically (subject to the same `spec.api_key_required` gate).
- [ ] A turn routed to a non-active provider (via `@`-override, modality
      redirection, or a `routing.toml` rule) that fails with
      `ErrorKind::Authentication` shows a hint naming the *routed* provider
      (from `TurnEvent::RouteSelected`), not whatever `app.active_provider_id`
      happens to be at the time. Verified in both `main.rs`'s TUI front-end
      and `egui_app/mod.rs`'s GUI front-end.
- [ ] A turn that fails with `ErrorKind::Authentication` (e.g. a revoked key
      caught only at first-prompt time, since `list_models` validation at
      connect time can't catch a key that goes bad *after* a successful
      connect) shows both the existing `Error: <name> rejected the request:
      <message>` note and a new `Run /connect <id> --rekey to enter a
      different API key for <name>.` note, in both front-ends.
- [ ] A turn failing with any other `ErrorKind` (rate limit, overloaded,
      context length, etc.) shows only the existing `Error: ...` note — no
      new hint, since `--rekey` would not fix those.
- [ ] `cargo build --workspace` succeeds (i.e. `egui_app/mod.rs`'s
      `handle_worker_msg` match remains exhaustive after adding
      `WorkerMsg::TurnAuthError`), `cargo test --workspace` and
      `cargo clippy --workspace --all-targets` stay green, and
      `cargo fmt --all --check` passes.

## Error Handling & Edge Cases

- **No matching provider spec for `current_turn_provider_id`** (e.g. the id
  was removed from `effective_providers()` between the turn starting and
  the error arriving — not currently possible but defensive): skip the
  hint, log nothing extra (the existing `Error: ...` note already fired);
  this is a silent no-op, not a panic or unwrap.
- **`current_turn_provider_id` is `None`** (defensive only —
  `TurnEvent::RouteSelected` fires unconditionally before any provider call
  that could produce an `Authentication`-kind `HostError::Provider`, so this
  should not occur in practice): same silent-skip behavior.
- **A provider whose `api_key_required` is `false`** (`local`) producing an
  `Authentication`-kind error (unexpected, since Ollama has no auth) is
  handled by the existing `spec.api_key_required` guard — hint is skipped.
- **A turn routed to a non-active provider that then fails authentication**:
  correctly handled — see Approach §2's `TurnEvent::RouteSelected` capture;
  the hint names the routed provider, not the active one.
- **Two providers connected, the non-active one is the one whose stored key
  later goes bad**: unaffected by this change unless a turn is actually
  routed to it (via override/modality/`routing.toml`), in which case
  `current_turn_provider_id` correctly names it per Approach §2.

## Risks & Open Questions

- The turn-bookkeeping extraction (shared helper for `WorkerMsg::Error` and
  `WorkerMsg::TurnAuthError`) touches a code path with several closure-
  captured `&mut` locals (`footer_pending_turn_id`, `current_turn_id`,
  `next_turn_id`, `turn_terminal_event_seen`, and now
  `current_turn_provider_id`). The plan's Task 2 must get the exact
  signature right without changing behavior for the existing
  `WorkerMsg::Error` arm — covered by keeping all existing assertions/tests
  around turn-error `TurnStart`/`TurnEnd` symmetry green.
- This change now touches two independently-maintained front-ends
  (`main.rs`'s TUI and `egui_app/mod.rs`'s GUI) that already duplicate the
  `WorkerMsg`-handling logic by house style (no shared helper between them
  today). The plan must implement and test both mirrored changes in lockstep
  — there is no compiler enforcement that `egui_app`'s classification logic
  actually matches `main.rs`'s beyond both compiling, so a manual side-by-
  side diff check during implementation/review is called out explicitly as
  a required verification step, not just "add an arm."
- Retranslating for es/hi/pt without a native speaker (new keys
  `notes.connect-rejected-keyed`, `notes.turn-auth-failed-hint`) is a known,
  accepted quality gap (see Assumptions) — flagged here for reviewer
  visibility, not blocking.

# Afford a path back to a valid API key after a rejected connect — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Status: shipped in 0.26.2 via `savvagent/otto#81`** (see `CHANGELOG.md`). Every checkbox below is
> ticked to reflect that. This plan predates `savvagent/otto#94`/`#97`, which removed the
> `egui_app` GUI front-end entirely — every step and file reference below that targets
> `crates/otto/src/egui_app/mod.rs` or "both front-ends" describes work that *was* done (the GUI
> front-end existed and received the identical fix at the time), but that module no longer exists
> in the tree today. Those references are kept as-is for historical accuracy rather than edited to
> pretend the GUI never existed; see `savvagent/otto#99`.

**Goal:** Make the three existing dead-end failure notes (connect-time rejection, silent stored-key reconnect failure, turn-time authentication failure) point the user at the already-existing `/connect <id> --rekey` mechanism, without inventing any new `/connect` UX. Closes `savvagent/otto#81`.

**Architecture:** Three sequenced slices: (1) thread `otto_protocol::ErrorKind` through `DynamicCapsOutcome::Rejected`/`ProviderBuildOutcome::Rejected` (currently a bare `String`, discarding whether the rejection was actually a bad key vs. a rate limit/permission/quota failure) so the two connect-time call sites can gate a rekey hint correctly; (2) add a new locale key and the `if` at those two call sites; (3) classify turn-time `HostError::Provider` errors by `ErrorKind::Authentication`, capture the per-turn routed provider id from `TurnEvent::RouteSelected` (not `app.active_provider_id`, which can be stale/wrong under `@`-override or `routing.toml` routing) via a shared, unit-testable `providers::turn_auth_hint` helper, and surface a matching hint via a new `WorkerMsg::TurnAuthError` variant in **both** front-ends (`main.rs`'s TUI and `egui_app/mod.rs`'s GUI, landed in one commit since `egui_app`'s exhaustive `WorkerMsg` match would not otherwise compile).

**Tech Stack:** Existing workspace crates only (`otto-protocol`, `otto`, `otto-host` read-only). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-08-reconnect-after-api-key-error-design.md`

**Branch:** `host/reconnect-after-api-key-error` (already created off `origin/main`).

**Release line:** v0.26.2

---

## File Map

**Modified files**
- `crates/otto/src/plugin/builtin/provider_common.rs` — `DynamicCapsOutcome::Rejected`/`ProviderBuildOutcome::Rejected` gain a `kind: otto_protocol::ErrorKind` field; `build_dynamic_caps`'s construction site and existing unit tests updated.
- `crates/otto/src/plugin/builtin/provider_{anthropic,deepseek,gemini,local,openai}/mod.rs` — mechanical destructure/re-wrap at each `DynamicCapsOutcome::Rejected` → `ProviderBuildOutcome::Rejected` pass-through (no logic change).
- `crates/otto/src/main.rs` — destructure updates at all three existing `ProviderBuildOutcome::Rejected` call sites (`try_provider!` macro, `apply_pending_pool_add`, `perform_connect`); new locale-key gating at the latter two; new `WorkerMsg::TurnAuthError` variant; `current_turn_provider_id` local capturing `TurnEvent::RouteSelected`; turn-spawn error classification; new match arm in the main loop; shared turn-bookkeeping helper extraction.
- `crates/otto/src/egui_app/mod.rs` — mirrored `current_turn_provider_id` field/capture, turn-spawn classification, and `WorkerMsg::TurnAuthError` arm in `handle_worker_msg`. **This module no longer exists** — it was removed wholesale by `savvagent/otto#94`/`#97`, after this plan shipped. See `savvagent/otto#99`.
- `crates/otto/locales/{en,es,hi,pt}.toml` — new keys `notes.connect-rejected-keyed`, `notes.turn-auth-failed-hint`.

**No new files.**

---

## Task 1: Thread `ErrorKind` through `DynamicCapsOutcome`/`ProviderBuildOutcome`

**Files:**
- Modify: `crates/otto/src/plugin/builtin/provider_common.rs`
- Modify: `crates/otto/src/plugin/builtin/provider_anthropic/mod.rs`, `provider_deepseek/mod.rs`, `provider_gemini/mod.rs`, `provider_local/mod.rs`, `provider_openai/mod.rs`
- Modify: `crates/otto/src/main.rs`

- [x] **Step 1: Read the current shapes**

View `crates/otto/src/plugin/builtin/provider_common.rs:180-270` (`ProviderBuildOutcome`, `DynamicCapsOutcome`, `build_dynamic_caps`) and its existing unit tests (`~line 278` onward, referencing `DynamicCapsOutcome::Rejected(reason)` and `Rejected(_)`). Confirm `build_dynamic_caps`'s final `Err(e) => DynamicCapsOutcome::Rejected(e.message.clone())` arm discards `e.kind`. Grep `DynamicCapsOutcome::Rejected\|ProviderBuildOutcome::Rejected` across `crates/otto/src` to enumerate every call/construction site (expect: 1 construction site in `provider_common.rs`, 5 provider-plugin pass-through arms, 3 sites in `main.rs` at ~lines 650, 1912, 2689, and 3 test-pattern sites in `provider_common.rs`'s test module).

- [x] **Step 2: Add a failing test (red)**

In `provider_common.rs`'s test module, add/extend a test that mocks a `list_models` failure with `ErrorKind::RateLimited` (or `PermissionDenied`) and asserts the returned `DynamicCapsOutcome::Rejected { kind, .. }` has `kind == ErrorKind::RateLimited` (distinct from the existing `Authentication`-kind test). Run `cargo test -p otto provider_common 2>&1 | tail -30` — expect a compile failure (the field doesn't exist yet).

- [x] **Step 3: Change the enum shapes and update every downstream pattern (green)**

`otto` is a single binary crate, so `cargo test -p otto`/`cargo build -p otto` compile the whole crate at once — changing `DynamicCapsOutcome`/`ProviderBuildOutcome`'s shape and building/testing before every pattern-match site is updated will fail with unrelated pattern-mismatch errors elsewhere in the crate, not just in `provider_common.rs`. Make all of the following edits together, as one uninterrupted pass, before running any build or test:

In `provider_common.rs`, change:

```rust
pub(crate) enum DynamicCapsOutcome {
    Ready(ProviderCapabilities, Option<String>),
    Rejected { reason: String, kind: otto_protocol::ErrorKind },
}

pub(crate) enum ProviderBuildOutcome {
    Unavailable,
    Rejected { reason: String, kind: otto_protocol::ErrorKind },
    Ready(ProviderRegistration, Option<String>),
}
```

Update `build_dynamic_caps`'s final arm to `Err(e) => DynamicCapsOutcome::Rejected { reason: e.message.clone(), kind: e.kind }`. Update the 3 existing test call sites' patterns (`DynamicCapsOutcome::Rejected(reason) => ...` → `DynamicCapsOutcome::Rejected { reason, .. } => ...`, `Rejected(_)` → `Rejected { .. }`) — assertions on `reason` content are unchanged.

Then, in the same pass, continue to Steps 4 and 5 below (the 5 provider-plugin pass-through sites and the 3 `main.rs` call sites) before attempting a build. Only after all of them are updated, run `cargo test -p otto provider_common 2>&1 | tail -30` (Step 6) — expect green.

- [x] **Step 4: Update the 5 provider-plugin pass-through sites**

In each of `provider_anthropic/mod.rs`, `provider_deepseek/mod.rs`, `provider_gemini/mod.rs`, `provider_local/mod.rs`, `provider_openai/mod.rs`, find the one `DynamicCapsOutcome::Rejected(reason) => return Ok(ProviderBuildOutcome::Rejected(reason))` arm and change it to `DynamicCapsOutcome::Rejected { reason, kind } => return Ok(ProviderBuildOutcome::Rejected { reason, kind })` — pure destructure/re-wrap, no other change.

- [x] **Step 5: Update `main.rs`'s 3 call sites**

- `try_provider!` macro (~line 650, startup path): change `Ok(Ok(ProviderBuildOutcome::Rejected(reason)))` to `Ok(Ok(ProviderBuildOutcome::Rejected { reason, kind: _ }))` (kind discarded here — this site is unconditionally gated by `config_file.startup.verbose` already and is out of scope for the new hint per the spec).
- `apply_pending_pool_add` (~line 1912): change the pattern to `Ok(ProviderBuildOutcome::Rejected { reason, kind })`, keep existing behavior for now (Task 2 adds the gating logic in this arm's body).
- `perform_connect` (~line 2689): same pattern-destructure update; keep existing behavior for now (Task 2 adds the gating logic).

- [x] **Step 6: Build and test — first green-build checkpoint**

Only now, with Steps 3-5 all applied, run `cargo build --workspace 2>&1 | tail -60`. Fix any remaining pattern mismatches. Run `cargo test --workspace 2>&1 | tail -60` — expect green (no behavior change yet, purely a shape threading).

- [x] **Step 7: Commit**

```bash
git add crates/otto/src/plugin/builtin/provider_common.rs crates/otto/src/plugin/builtin/provider_anthropic crates/otto/src/plugin/builtin/provider_deepseek crates/otto/src/plugin/builtin/provider_gemini crates/otto/src/plugin/builtin/provider_local crates/otto/src/plugin/builtin/provider_openai crates/otto/src/main.rs
git commit -m "refactor(otto): thread ErrorKind through DynamicCapsOutcome/ProviderBuildOutcome

build_dynamic_caps previously discarded ErrorKind entirely for any
non-Network/NotImplemented list_models failure, collapsing a bad API key
(Authentication), a rate-limited account (RateLimited), and a disabled/
quota-exhausted org (PermissionDenied) into one indistinguishable
Rejected(String). Both Rejected variants now carry the originating kind
so callers can react differently to a genuine credential rejection than
to other rejection causes. No behavior change yet — callers still ignore
kind until the next commit."
```

---

## Task 2: Gate the connect-time `--rekey` hint on `Authentication` + `api_key_required`

**Depends on Task 1.**

**Files:**
- Modify: `crates/otto/locales/{en,es,hi,pt}.toml`
- Modify: `crates/otto/src/main.rs`

- [x] **Step 1: Add the locale key**

In `crates/otto/locales/en.toml`, add, near `connect-failed` (line ~18):

```toml
connect-rejected-keyed = "Connect to %{id} failed: %{err} Run /connect %{id} --rekey to try a different key."
```

Add the equivalent key (translated, matching the existing short-literal style of the other keys in each file) to `es.toml`, `hi.toml`, `pt.toml`.

- [x] **Step 2: Add a failing test (red)**

Add/extend a test in `crates/otto`'s test module covering `perform_connect` (or the smallest existing test harness that exercises its `Rejected` arm) asserting that an `Authentication`-kind rejection on an `api_key_required: true` provider produces a note containing `--rekey`, and that a `RateLimited`-kind rejection on the same provider does not. If no existing test harness exercises `perform_connect` directly, add the assertion at the smallest testable unit (e.g. extract the key-selection `if` into a small pure helper function `fn connect_rejected_note_key(kind: otto_protocol::ErrorKind, api_key_required: bool) -> &'static str` that can be unit-tested directly without going through the full async `perform_connect`/`apply_pending_pool_add` machinery).

Run the new test — expect failure (helper doesn't exist / always uses the old key).

- [x] **Step 3: Implement the gate (green)**

Add the helper (co-located with `perform_connect`/`apply_pending_pool_add` in `main.rs`, or in `provider_common.rs` alongside the enums it consumes):

```rust
fn connect_rejected_note_key(kind: otto_protocol::ErrorKind, api_key_required: bool) -> &'static str {
    if api_key_required && kind == otto_protocol::ErrorKind::Authentication {
        "notes.connect-rejected-keyed"
    } else {
        "notes.connect-failed"
    }
}
```

At `perform_connect`'s `Rejected { reason, kind }` arm (~line 2689), change:

```rust
app.push_note(rust_i18n::t!("notes.connect-failed", id = spec.id, err = reason).to_string());
```

to:

```rust
app.push_note(
    rust_i18n::t!(connect_rejected_note_key(kind, spec.api_key_required), id = spec.id, err = reason)
        .to_string(),
);
```

Apply the identical change at `apply_pending_pool_add`'s `Rejected { reason, kind }` arm (~line 1912), preserving its existing `show_notes` gating (the `if show_notes { ... }` wrapper is unchanged — only the locale key inside it changes).

Run the Step 2 test — expect green.

- [x] **Step 4: Manual/integration sanity check**

Run `cargo test -p otto 2>&1 | tail -60` — full crate suite green. If an existing test asserted the literal old `notes.connect-failed`-only text for an `Authentication`-kind rejection, update its expectation to the new `notes.connect-rejected-keyed` text.

- [x] **Step 5: Commit**

```bash
git add crates/otto/locales crates/otto/src/main.rs
git commit -m "fix(otto): point connect-time key rejections at --rekey

perform_connect and apply_pending_pool_add's Rejected arms now select a
new notes.connect-rejected-keyed locale key (appending a '/connect <id>
--rekey' hint) specifically when the rejection's ErrorKind is
Authentication and the provider requires a key — leaving every other
Rejected case (rate limit, permission/quota, keyless providers) on the
unchanged notes.connect-failed text, since --rekey would not fix those."
```

---

## Task 3: Surface the same hint after a turn-time authentication failure (both front-ends)

> **Moot as of `savvagent/otto#94`/`#97`:** at the time this task was executed, `egui_app` was a
> real second front-end and every GUI-facing step below did land as described. `savvagent/otto#94`/
> `#97` have since deleted `crates/otto/src/egui_app/mod.rs` entirely, so the GUI-specific
> instructions in this task (Step 6, and the GUI references throughout Steps 1-9) no longer apply
> to the current tree — the TUI (`main.rs`) remains otto's only front-end. See `savvagent/otto#99`.

**Depends on Task 1 (not Task 2 — this task is independent of the connect-time locale key, but shares the `otto_protocol::ErrorKind` import already in scope).**

**Note on commit granularity:** `egui_app` is compiled unconditionally (`mod egui_app;` in `main.rs`), and its `handle_worker_msg` (`crates/otto/src/egui_app/mod.rs:193`) is an exhaustive `match` over `WorkerMsg` with no wildcard arm. Adding `WorkerMsg::TurnAuthError` therefore breaks the crate's build the instant it's introduced, until `egui_app/mod.rs` also handles it. This task's steps are ordered TUI-first for readability, but **land the `WorkerMsg::TurnAuthError` variant and both front-ends' handling of it in the same commit** (Step 8's commit) — do not commit the variant addition (Step 3) separately, and do not expect a green build after Step 3/4/5 in isolation; the first green-build checkpoint in this task is Step 7, after both front-ends are updated.

**Files:**
- Modify: `crates/otto/locales/{en,es,hi,pt}.toml`
- Modify: `crates/otto/src/providers.rs`
- Modify: `crates/otto/src/main.rs`
- Modify: `crates/otto/src/egui_app/mod.rs`

- [x] **Step 1: Read the current turn-error path in both front-ends**

View `crates/otto/src/main.rs`'s `WorkerMsg` enum (~line 91), the turn-spawn block's `Ok(Err(e)) => tx.send(WorkerMsg::Error(e.to_string()))` (~line 3869), the `WorkerMsg::Error(msg)` match arm (~lines 3428-3479) including its `footer_pending_turn_id`/`current_turn_id`/`turn_terminal_event_seen`/`next_turn_id` bookkeeping, and the `WorkerMsg::Event(e)` arm's `html_block_stop_id` capture pattern (the `if let TurnEvent::HtmlBlockStop { index } = &e` block a few lines before `app.apply_turn_event(e)`) to mirror for `current_turn_provider_id`. Also view `crates/otto-host/src/session.rs`'s `HostError::Provider { name, error }` and `TurnEvent::RouteSelected { provider_id, .. }` (~lines 137, 238), `crates/otto/src/app.rs`'s `RouteSelected` arm in `apply_turn_event` (~line 1114) to confirm `provider_id`'s type (`otto_protocol::ProviderId`, which exposes `.as_str()` — see `crates/otto-protocol/src/provider_id.rs`) and that nothing today retains it past that arm, and `crates/otto/src/providers.rs`'s `ProviderSpec` (`id: &'static str`, `api_key_required: bool`) and `effective_providers() -> Vec<&'static ProviderSpec>`.

Then view `crates/otto/src/egui_app/mod.rs`'s `OttoApp` struct (its `current_turn_id`/`next_turn_id`/`next_tool_call_id`/`last_tool_call_id` fields — confirm it has **no** `footer_pending_turn_id` and **no** `turn_terminal_event_seen`, unlike `main.rs`), `handle_worker_msg`'s `WorkerMsg::Event(e)` arm (`crates/otto/src/egui_app/mod.rs:195-256`) and `WorkerMsg::Error(msg)` arm (`:257-296`), and its own turn-spawn site (~line 396-415) that independently sends `WorkerMsg::Error`.

- [x] **Step 2: Add the locale key**

Add to `crates/otto/locales/en.toml` (and the translated equivalents in `es.toml`, `hi.toml`, `pt.toml`):

```toml
turn-auth-failed-hint = "Run /connect %{id} --rekey to enter a different API key for %{name}."
```

- [x] **Step 3: Add a shared, unit-testable hint helper in `providers.rs`**

Both front-ends must apply the *identical* gating/lookup logic, so put it once in `crates/otto/src/providers.rs` (which already owns `ProviderSpec`/`effective_providers`) rather than duplicating it in `main.rs` and `egui_app/mod.rs`:

```rust
/// Render the `--rekey` hint for a turn-time authentication failure, or
/// `None` if the routed provider is unknown or doesn't take an API key
/// (in which case `--rekey` would have nothing useful to do).
///
/// `routed_provider_id` should come from the per-turn `TurnEvent::
/// RouteSelected` capture, not `App::active_provider_id` — routing
/// (`@`-override, modality redirection, `routing.toml`) can select a
/// different pool entry than the active one for a given turn.
pub(crate) fn turn_auth_hint(
    routed_provider_id: Option<&otto_protocol::ProviderId>,
    provider_display_name: &str,
) -> Option<String> {
    let id = routed_provider_id?;
    let spec = effective_providers()
        .into_iter()
        .find(|spec| spec.id == id.as_str())?;
    if !spec.api_key_required {
        return None;
    }
    Some(
        rust_i18n::t!("notes.turn-auth-failed-hint", id = spec.id, name = provider_display_name)
            .to_string(),
    )
}
```

Add a unit test in `providers.rs`'s existing test module asserting: `turn_auth_hint(Some(&ProviderId::new("gemini").unwrap()), "Gemini")` returns `Some(text)` where `text.contains("/connect gemini --rekey")`; `turn_auth_hint(None, "Gemini")` returns `None`; and (using `local`'s spec, `api_key_required: false`) `turn_auth_hint(Some(&ProviderId::new("local").unwrap()), "Local")` returns `None`. This is the plan's concrete regression test for the "routed id, not active id" requirement — the helper takes the routed id as an explicit parameter with no reference to `App::active_provider_id` at all, so the property is structurally enforced and directly testable without spinning up `run_app`'s TUI loop or `OttoApp`'s GUI loop (this crate's `tests/` integration tests cannot reach `src/main.rs`/`src/egui_app/` internals — see the existing note in `crates/otto/tests/`).

Run `cargo test -p otto providers:: 2>&1 | tail -30` — expect green.

- [x] **Step 4: Add `WorkerMsg::TurnAuthError`**

Add a variant to `WorkerMsg` (~line 91) in `main.rs` (shared by both front-ends):

```rust
/// Sent if `run_turn_streaming` returned an error whose ErrorKind is
/// Authentication — carries enough to render both the existing error note
/// and a --rekey hint naming the actual routed provider.
TurnAuthError {
    message: String,
    provider_display_name: String,
},
```

(This alone does not compile — `egui_app`'s exhaustive match now has a missing arm. Continue to Step 5 before attempting a build.)

- [x] **Step 5: Wire the TUI (`main.rs`)**

1. Declare `let mut current_turn_provider_id: Option<otto_protocol::ProviderId> = None;` alongside `current_turn_id`/`footer_pending_turn_id`/`turn_terminal_event_seen` (~line 3226).
2. In the `WorkerMsg::Event(e)` arm, before `app.apply_turn_event(e)` consumes `e`, add (mirroring the `html_block_stop_id` capture immediately above it):
   ```rust
   if let TurnEvent::RouteSelected { provider_id, .. } = &e {
       current_turn_provider_id = Some(provider_id.clone());
   }
   ```
3. Reset `current_turn_provider_id = None;` at the same point `turn_terminal_event_seen = true;` is set inside this arm's terminal-event branch, and also at the end of both the `WorkerMsg::Error` arm and the new `WorkerMsg::TurnAuthError` arm (mirroring where `turn_terminal_event_seen = false;` is reset at the end of today's `WorkerMsg::Error` arm) — a stale routed-provider id from a previous turn must never leak into a later one.
4. Extract the existing `WorkerMsg::Error(msg)` arm's bookkeeping body (the `is_loading` reset, the `Entry::Note` push, the synthetic `TurnStart`/`TurnEnd { success: false }` dance, `turn_terminal_event_seen` reset) into a private async helper (e.g. `record_turn_error(app: &mut App, message: String, footer_pending_turn_id: &mut Option<u32>, current_turn_id: &mut Option<u32>, next_turn_id: &mut u32, last_tool_call_id: &mut Option<u64>, turn_terminal_event_seen: &mut bool)` — exact shape adjusted to match existing local-variable ownership). Update the existing `WorkerMsg::Error(msg)` arm to call it. This helper does **not** own `current_turn_provider_id` — both call sites (the `WorkerMsg::Error` arm and the new `WorkerMsg::TurnAuthError` arm below) reset that field themselves, after the helper call, alongside their respective note-pushing.
5. At the turn-spawn block's `Ok(Err(e))` arm (~line 3869), classify before sending:
   ```rust
   Ok(Err(e)) => {
       let auth_name = if let otto_host::HostError::Provider { name, error } = &e {
           (error.kind == otto_protocol::ErrorKind::Authentication).then(|| name.clone())
       } else {
           None
       };
       match auth_name {
           Some(provider_display_name) => {
               let _ = tx
                   .send(WorkerMsg::TurnAuthError {
                       message: e.to_string(),
                       provider_display_name,
                   })
                   .await;
           }
           None => {
               let _ = tx.send(WorkerMsg::Error(e.to_string())).await;
           }
       }
   }
   ```
   (adjust the exact `HostError`/`ErrorKind` import path to match what's already in scope in this file).
6. Add a new arm in the main loop's `match msg`:
   ```rust
   WorkerMsg::TurnAuthError { message, provider_display_name } => {
       record_turn_error(app, message, &mut footer_pending_turn_id, &mut current_turn_id, &mut next_turn_id, &mut last_tool_call_id, &mut turn_terminal_event_seen).await;
       if let Some(hint) = crate::providers::turn_auth_hint(current_turn_provider_id.as_ref(), &provider_display_name) {
           app.push_note(hint);
       }
       current_turn_provider_id = None;
   }
   ```

- [x] **Step 6: Wire the GUI (`egui_app/mod.rs`)**

1. Add a `current_turn_provider_id: Option<otto_protocol::ProviderId>` field to `OttoApp`'s turn-tracking state (initialized to `None` wherever `current_turn_id`/`next_turn_id` are initialized).
2. In `handle_worker_msg`'s `WorkerMsg::Event(e)` arm, capture it from `TurnEvent::RouteSelected` identically to Step 5.2 above, before `self.app.apply_turn_event(e)` consumes `e`. Reset it to `None` after any terminal `TurnEvent` (`TurnComplete`, `Cancelled`, `AbortedAfterGrace`) is handled and at the end of both the existing `WorkerMsg::Error` arm and the new `WorkerMsg::TurnAuthError` arm. `OttoApp` has neither `footer_pending_turn_id` nor `turn_terminal_event_seen` — do **not** introduce either into `OttoApp` for this feature; only mirror its existing simpler turn-id bookkeeping.
3. At the turn-spawn site (~line 411), apply the identical classification from Step 5.5: if the `HostError` is `Provider { error, .. }` with `error.kind == ErrorKind::Authentication`, send `WorkerMsg::TurnAuthError { message, provider_display_name }` instead of `WorkerMsg::Error`.
4. Add a `WorkerMsg::TurnAuthError { message, provider_display_name }` arm to `handle_worker_msg`, mirroring the existing `WorkerMsg::Error(msg)` arm's bookkeeping shape in this file (its own synthetic-turn-id logic, not `main.rs`'s `record_turn_error`), then call `crate::providers::turn_auth_hint(self.current_turn_provider_id.as_ref(), &provider_display_name)` — note `current_turn_provider_id` lives on `OttoApp` itself (per Step 6.1), not on the nested `self.app` — and `self.app.push_note(hint)` if `Some`.

- [x] **Step 7: Build and test — first green-build checkpoint**

Run `cargo build --workspace 2>&1 | tail -60` — this is the first point since Step 4 that the workspace is expected to compile (both front-ends now handle `WorkerMsg::TurnAuthError`). Run `cargo test -p otto 2>&1 | tail -60`.

- [x] **Step 8: Manual side-by-side diff check**

Diff `main.rs`'s Step 5 changes against `egui_app/mod.rs`'s Step 6 changes side-by-side to confirm the classification logic and the shared `turn_auth_hint` call are wired identically in both front-ends — this is largely guaranteed by construction now that both call the same `providers::turn_auth_hint` helper, but confirm the `HostError`/`ErrorKind` classification `if`/`match` at each turn-spawn site is the same shape.

- [x] **Step 9: Commit**

```bash
git add crates/otto/locales crates/otto/src/providers.rs crates/otto/src/main.rs crates/otto/src/egui_app/mod.rs
git commit -m "fix(otto): hint at --rekey after a turn-time authentication failure

Both front-ends' turn-runner spawn now classify HostError::Provider
errors by ErrorKind::Authentication and, when matched, send a new
WorkerMsg::TurnAuthError instead of the generic WorkerMsg::Error. Each
main loop captures the per-turn routed provider id from
TurnEvent::RouteSelected (not App::active_provider_id, which can be
wrong under @-override/routing.toml routing) and, via the new shared
providers::turn_auth_hint helper, appends a notes.turn-auth-failed-hint
note naming the routed provider and the existing --rekey command,
alongside the unchanged Error: ... note. Landed as one commit across
both crates/otto/src/main.rs and crates/otto/src/egui_app/mod.rs since
egui_app's exhaustive WorkerMsg match would not otherwise compile."
```

---

## Task 4: Final verification

**Depends on Tasks 1-3.**

- [x] **Step 1: Full workspace build**

Run `cargo build --workspace 2>&1 | tail -60` — expect clean.

- [x] **Step 2: Full test suite**

Run `cargo test --workspace 2>&1 | tail -100` — expect green.

- [x] **Step 3: Clippy**

Run `cargo clippy --workspace --all-targets 2>&1 | tail -60` — expect clean (no new warnings).

- [x] **Step 4: Format check**

Run `cargo fmt --all --check 2>&1 | tail -60` — expect clean; if it reports diffs, run `cargo fmt --all` and amend the relevant commit(s).

- [x] **Step 5: Manual smoke test (optional but recommended given no end-to-end harness covers the TUI event loop)**

If feasible in this environment, run `cargo run -p otto` (or the headless example) against a provider with a deliberately invalid API key and confirm the connect-time note now includes the `--rekey` hint; this is a manual confirmation only, not a substitute for the automated tests in Tasks 2-4.

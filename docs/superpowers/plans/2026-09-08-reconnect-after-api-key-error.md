# Afford a path back to a valid API key after a rejected connect — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the three existing dead-end failure notes (connect-time rejection, silent stored-key reconnect failure, turn-time authentication failure) point the user at the already-existing `/connect <id> --rekey` mechanism, without inventing any new `/connect` UX. Closes `savvagent/otto#81`.

**Architecture:** Four sequenced slices: (1) thread `otto_protocol::ErrorKind` through `DynamicCapsOutcome::Rejected`/`ProviderBuildOutcome::Rejected` (currently a bare `String`, discarding whether the rejection was actually a bad key vs. a rate limit/permission/quota failure) so the two connect-time call sites can gate a rekey hint correctly; (2) add a new locale key and the `if` at those two call sites; (3) classify turn-time `HostError::Provider` errors by `ErrorKind::Authentication`, capture the per-turn routed provider id from `TurnEvent::RouteSelected` (not `app.active_provider_id`, which can be stale/wrong under `@`-override or `routing.toml` routing), and surface a matching hint via a new `WorkerMsg::TurnAuthError` variant in the TUI (`main.rs`); (4) mirror slice 3 in the GUI front-end (`egui_app/mod.rs`), which shares the same `WorkerMsg` enum via an exhaustive match with no wildcard arm and therefore must be updated in the same change or the crate does not compile.

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
- `crates/otto/src/egui_app/mod.rs` — mirrored `current_turn_provider_id` field/capture, turn-spawn classification, and `WorkerMsg::TurnAuthError` arm in `handle_worker_msg`.
- `crates/otto/locales/{en,es,hi,pt}.toml` — new keys `notes.connect-rejected-keyed`, `notes.turn-auth-failed-hint`.

**No new files.**

---

## Task 1: Thread `ErrorKind` through `DynamicCapsOutcome`/`ProviderBuildOutcome`

**Files:**
- Modify: `crates/otto/src/plugin/builtin/provider_common.rs`
- Modify: `crates/otto/src/plugin/builtin/provider_anthropic/mod.rs`, `provider_deepseek/mod.rs`, `provider_gemini/mod.rs`, `provider_local/mod.rs`, `provider_openai/mod.rs`
- Modify: `crates/otto/src/main.rs`

- [ ] **Step 1: Read the current shapes**

View `crates/otto/src/plugin/builtin/provider_common.rs:180-270` (`ProviderBuildOutcome`, `DynamicCapsOutcome`, `build_dynamic_caps`) and its existing unit tests (`~line 278` onward, referencing `DynamicCapsOutcome::Rejected(reason)` and `Rejected(_)`). Confirm `build_dynamic_caps`'s final `Err(e) => DynamicCapsOutcome::Rejected(e.message.clone())` arm discards `e.kind`. Grep `DynamicCapsOutcome::Rejected\|ProviderBuildOutcome::Rejected` across `crates/otto/src` to enumerate every call/construction site (expect: 1 construction site in `provider_common.rs`, 5 provider-plugin pass-through arms, 3 sites in `main.rs` at ~lines 650, 1912, 2689, and 3 test-pattern sites in `provider_common.rs`'s test module).

- [ ] **Step 2: Add a failing test (red)**

In `provider_common.rs`'s test module, add/extend a test that mocks a `list_models` failure with `ErrorKind::RateLimited` (or `PermissionDenied`) and asserts the returned `DynamicCapsOutcome::Rejected { kind, .. }` has `kind == ErrorKind::RateLimited` (distinct from the existing `Authentication`-kind test). Run `cargo test -p otto provider_common 2>&1 | tail -30` — expect a compile failure (the field doesn't exist yet).

- [ ] **Step 3: Change the enum shapes (green)**

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

Run `cargo test -p otto provider_common 2>&1 | tail -30` — expect green (compiles again in isolation; downstream crate-wide errors are expected until Step 4).

- [ ] **Step 4: Update the 5 provider-plugin pass-through sites**

In each of `provider_anthropic/mod.rs`, `provider_deepseek/mod.rs`, `provider_gemini/mod.rs`, `provider_local/mod.rs`, `provider_openai/mod.rs`, find the one `DynamicCapsOutcome::Rejected(reason) => return Ok(ProviderBuildOutcome::Rejected(reason))` arm and change it to `DynamicCapsOutcome::Rejected { reason, kind } => return Ok(ProviderBuildOutcome::Rejected { reason, kind })` — pure destructure/re-wrap, no other change.

- [ ] **Step 5: Update `main.rs`'s 3 call sites**

- `try_provider!` macro (~line 650, startup path): change `Ok(Ok(ProviderBuildOutcome::Rejected(reason)))` to `Ok(Ok(ProviderBuildOutcome::Rejected { reason, kind: _ }))` (kind discarded here — this site is unconditionally gated by `config_file.startup.verbose` already and is out of scope for the new hint per the spec).
- `apply_pending_pool_add` (~line 1912): change the pattern to `Ok(ProviderBuildOutcome::Rejected { reason, kind })`, keep existing behavior for now (Task 2 adds the gating logic in this arm's body).
- `perform_connect` (~line 2689): same pattern-destructure update; keep existing behavior for now (Task 2 adds the gating logic).

- [ ] **Step 6: Build and fix stragglers**

Run `cargo build --workspace 2>&1 | tail -60`. Fix any remaining pattern mismatches. Run `cargo test --workspace 2>&1 | tail -60` — expect green (no behavior change yet, purely a shape threading).

- [ ] **Step 7: Commit**

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

- [ ] **Step 1: Add the locale key**

In `crates/otto/locales/en.toml`, add, near `connect-failed` (line ~18):

```toml
connect-rejected-keyed = "Connect to %{id} failed: %{err} Run /connect %{id} --rekey to try a different key."
```

Add the equivalent key (translated, matching the existing short-literal style of the other keys in each file) to `es.toml`, `hi.toml`, `pt.toml`.

- [ ] **Step 2: Add a failing test (red)**

Add/extend a test in `crates/otto`'s test module covering `perform_connect` (or the smallest existing test harness that exercises its `Rejected` arm) asserting that an `Authentication`-kind rejection on an `api_key_required: true` provider produces a note containing `--rekey`, and that a `RateLimited`-kind rejection on the same provider does not. If no existing test harness exercises `perform_connect` directly, add the assertion at the smallest testable unit (e.g. extract the key-selection `if` into a small pure helper function `fn connect_rejected_note_key(kind: otto_protocol::ErrorKind, api_key_required: bool) -> &'static str` that can be unit-tested directly without going through the full async `perform_connect`/`apply_pending_pool_add` machinery).

Run the new test — expect failure (helper doesn't exist / always uses the old key).

- [ ] **Step 3: Implement the gate (green)**

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

- [ ] **Step 4: Manual/integration sanity check**

Run `cargo test -p otto 2>&1 | tail -60` — full crate suite green. If an existing test asserted the literal old `notes.connect-failed`-only text for an `Authentication`-kind rejection, update its expectation to the new `notes.connect-rejected-keyed` text.

- [ ] **Step 5: Commit**

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

## Task 3: Surface the same hint after a turn-time authentication failure (TUI)

**Depends on Task 1 (not Task 2 — this task is independent of the connect-time locale key, but shares the `otto_protocol::ErrorKind` import already in scope).**

**Files:**
- Modify: `crates/otto/locales/{en,es,hi,pt}.toml`
- Modify: `crates/otto/src/main.rs`

- [ ] **Step 1: Read the current turn-error path**

View `crates/otto/src/main.rs`'s `WorkerMsg` enum (~line 91), the turn-spawn block's `Ok(Err(e)) => tx.send(WorkerMsg::Error(e.to_string()))` (~line 3869), the `WorkerMsg::Error(msg)` match arm (~lines 3428-3479) including its `footer_pending_turn_id`/`current_turn_id`/`turn_terminal_event_seen`/`next_turn_id` bookkeeping, and the `WorkerMsg::Event(e)` arm's `html_block_stop_id` capture pattern (~line 3333) to mirror for `current_turn_provider_id`. Also view `crates/otto-host/src/session.rs`'s `HostError::Provider { name, error }` and `TurnEvent::RouteSelected { provider_id, .. }` (~lines 137, 238), and `crates/otto/src/app.rs`'s `RouteSelected` arm in `apply_turn_event` (~line 1114) to confirm `provider_id`'s type (`otto_protocol::ProviderId`) and that nothing today retains it past that arm.

- [ ] **Step 2: Add the locale key**

Add to `crates/otto/locales/en.toml` (and the translated equivalents in `es.toml`, `hi.toml`, `pt.toml`):

```toml
turn-auth-failed-hint = "Run /connect %{id} --rekey to enter a different API key for %{name}."
```

- [ ] **Step 3: Add `WorkerMsg::TurnAuthError`**

Add a variant to `WorkerMsg` (~line 91):

```rust
/// Sent if `run_turn_streaming` returned an error whose ErrorKind is
/// Authentication — carries enough to render both the existing error note
/// and a --rekey hint naming the actual routed provider.
TurnAuthError {
    message: String,
    provider_display_name: String,
},
```

- [ ] **Step 4: Capture `current_turn_provider_id` in the main loop**

Declare `let mut current_turn_provider_id: Option<otto_protocol::ProviderId> = None;` alongside `current_turn_id`/`footer_pending_turn_id`/`turn_terminal_event_seen` (~line 3226). In the `WorkerMsg::Event(e)` arm, before `app.apply_turn_event(e)` consumes `e`, add (mirroring the `html_block_stop_id` capture immediately above it):

```rust
if let TurnEvent::RouteSelected { provider_id, .. } = &e {
    current_turn_provider_id = Some(provider_id.clone());
}
```

Reset `current_turn_provider_id = None;` everywhere `turn_terminal_event_seen = true;` is currently set inside this arm's terminal-event branch, and also at the end of the `WorkerMsg::Error`/new `WorkerMsg::TurnAuthError` arms (mirroring where `turn_terminal_event_seen = false;` is reset at the end of today's `WorkerMsg::Error` arm), so a stale routed-provider id from a previous turn never leaks into a later one.

- [ ] **Step 5: Extract the shared turn-bookkeeping helper**

Factor the existing `WorkerMsg::Error(msg)` arm's body (the `is_loading` reset, the `Entry::Note` push, the synthetic `TurnStart`/`TurnEnd { success: false }` dance, `turn_terminal_event_seen`/`current_turn_provider_id` reset) into a private async helper, e.g.:

```rust
async fn record_turn_error(
    app: &mut App,
    message: String,
    footer_pending_turn_id: &mut Option<u32>,
    current_turn_id: &mut Option<u32>,
    next_turn_id: &mut u32,
    last_tool_call_id: &mut Option<u64>,
    turn_terminal_event_seen: &mut bool,
)
```

(exact parameter list/ownership adjusted as needed to match the existing local-variable shapes in `run_app` — this is a pure extraction, no behavior change). Update the existing `WorkerMsg::Error(msg)` arm to call it.

Run `cargo build -p otto 2>&1 | tail -30` and `cargo test -p otto 2>&1 | tail -30` — expect green (pure refactor, no new behavior yet).

- [ ] **Step 6: Classify the turn-spawn error and add the new match arm**

At the turn-spawn block's `Ok(Err(e))` arm (~line 3869), classify before sending:

```rust
Ok(Err(e)) => {
    let is_auth_failure = matches!(
        &e,
        otto_host::HostError::Provider { error, .. } if error.kind == otto_protocol::ErrorKind::Authentication
    );
    if is_auth_failure {
        let otto_host::HostError::Provider { name, .. } = &e else { unreachable!() };
        let _ = tx
            .send(WorkerMsg::TurnAuthError {
                message: e.to_string(),
                provider_display_name: name.clone(),
            })
            .await;
    } else {
        let _ = tx.send(WorkerMsg::Error(e.to_string())).await;
    }
}
```

(adjust the exact `HostError` variant destructure/import path to match what compiles — `otto_host::HostError`/`otto_protocol::ErrorKind` per the existing imports in this file). Add a new arm in the main loop's `match msg`:

```rust
WorkerMsg::TurnAuthError { message, provider_display_name } => {
    record_turn_error(app, message, &mut footer_pending_turn_id, &mut current_turn_id, &mut next_turn_id, &mut last_tool_call_id, &mut turn_terminal_event_seen).await;
    let hint = current_turn_provider_id
        .as_ref()
        .and_then(|id| crate::providers::effective_providers().into_iter().find(|s| &s.id == id))
        .filter(|spec| spec.api_key_required);
    if let Some(spec) = hint {
        app.push_note(
            rust_i18n::t!("notes.turn-auth-failed-hint", id = spec.id, name = provider_display_name)
                .to_string(),
        );
    }
    current_turn_provider_id = None;
}
```

- [ ] **Step 7: Add a regression test for the routed-provider case**

Add/extend a test (at whatever level `RouteSelected` handling is already unit-testable — e.g. via `App::apply_turn_event` plus a small harness around the new `current_turn_provider_id` capture logic, extracted into a testable pure function if the full `run_app` loop isn't unit-testable directly) asserting: given a `RouteSelected { provider_id: "gemini", .. }` event followed by a `TurnAuthError`, the hint names `gemini`, not whatever `app.active_provider_id` is set to (simulate `active_provider_id` being a different provider, e.g. `"anthropic"`, to prove the routed id — not the active one — wins).

Run the new test — expect green (if it fails, fix the capture/lookup logic, not the test).

- [ ] **Step 8: Build and full test pass**

Run `cargo build --workspace 2>&1 | tail -60` and `cargo test -p otto 2>&1 | tail -60`.

- [ ] **Step 9: Commit**

```bash
git add crates/otto/locales crates/otto/src/main.rs
git commit -m "fix(otto): hint at --rekey after a turn-time authentication failure

The turn-runner spawn now classifies HostError::Provider errors by
ErrorKind::Authentication and, when matched, sends a new
WorkerMsg::TurnAuthError instead of the generic WorkerMsg::Error. The
main loop captures the per-turn routed provider id from
TurnEvent::RouteSelected (not app.active_provider_id, which can be wrong
under @-override/routing.toml routing) and appends a
notes.turn-auth-failed-hint note naming the routed provider and the
existing --rekey command, alongside the unchanged Error: ... note."
```

---

## Task 4: Mirror Task 3 in the GUI front-end (`egui_app`)

**Depends on Task 3 (introduces `WorkerMsg::TurnAuthError`, which `egui_app`'s exhaustive `handle_worker_msg` match must handle to compile at all).**

**Files:**
- Modify: `crates/otto/src/egui_app/mod.rs`

- [ ] **Step 1: Read the mirrored structures**

View `crates/otto/src/egui_app/mod.rs`'s `OttoApp` turn-tracking fields (`current_turn_id`, `next_turn_id`, etc., near the struct definition), `handle_worker_msg`'s `WorkerMsg::Event(e)`/`WorkerMsg::Error(msg)` arms (~lines 187-296), and its own turn-spawn site (~line 396-415) that independently sends `WorkerMsg::Error`. Confirm `cargo build -p otto 2>&1 | tail -60` currently fails here with a non-exhaustive-match error after Task 3 lands (expected — this task fixes it).

- [ ] **Step 2: Add `current_turn_provider_id` to `OttoApp`**

Add a `current_turn_provider_id: Option<otto_protocol::ProviderId>` field to `OttoApp`'s turn-tracking state (initialized to `None` wherever `current_turn_id`/`next_turn_id` are initialized). In `handle_worker_msg`'s `WorkerMsg::Event(e)` arm, capture it from `TurnEvent::RouteSelected` identically to `main.rs`'s Task 3 Step 4, before `self.app.apply_turn_event(e)` consumes `e`. Reset it to `None` at the same points `main.rs` does.

- [ ] **Step 3: Classify the turn-spawn error**

At the turn-spawn site (~line 411), apply the identical classification from Task 3 Step 6: if the `HostError` is `Provider { error, .. }` with `error.kind == ErrorKind::Authentication`, send `WorkerMsg::TurnAuthError { message, provider_display_name }` instead of `WorkerMsg::Error`.

- [ ] **Step 4: Add the `WorkerMsg::TurnAuthError` arm to `handle_worker_msg`**

Add an arm mirroring the existing `WorkerMsg::Error(msg)` arm's bookkeeping shape in this file (note: `egui_app` has no `footer_pending_turn_id` concept — mirror its existing simpler synthetic-turn-id logic, not `main.rs`'s), then apply the identical `current_turn_provider_id` → `effective_providers()` → `notes.turn-auth-failed-hint` lookup and `push_note` call from Task 3 Step 6.

- [ ] **Step 5: Build and test**

Run `cargo build --workspace 2>&1 | tail -60` — expect green (this is the point the crate compiles again after Task 3). Run `cargo test -p otto 2>&1 | tail -60`.

- [ ] **Step 6: Manual side-by-side diff check**

Per the spec's Risks section, manually diff `main.rs`'s Task 3 changes against this task's changes side-by-side to confirm the classification logic (which `ErrorKind`/provider-lookup/locale-key/gating conditions trigger the hint) is identical in both front-ends — not just "both compile," but semantically matching behavior.

- [ ] **Step 7: Commit**

```bash
git add crates/otto/src/egui_app/mod.rs
git commit -m "fix(otto): mirror the --rekey turn-auth hint in the GUI front-end

egui_app/mod.rs shares the WorkerMsg enum with main.rs via an exhaustive
match with no wildcard arm; it gains the same current_turn_provider_id
capture, turn-spawn error classification, and WorkerMsg::TurnAuthError
handling introduced for the TUI in the previous commit, so both
front-ends give the user the same fix for issue #81."
```

---

## Task 5: Final verification

**Depends on Tasks 1-4.**

- [ ] **Step 1: Full workspace build**

Run `cargo build --workspace 2>&1 | tail -60` — expect clean.

- [ ] **Step 2: Full test suite**

Run `cargo test --workspace 2>&1 | tail -100` — expect green.

- [ ] **Step 3: Clippy**

Run `cargo clippy --workspace --all-targets 2>&1 | tail -60` — expect clean (no new warnings).

- [ ] **Step 4: Format check**

Run `cargo fmt --all --check 2>&1 | tail -60` — expect clean; if it reports diffs, run `cargo fmt --all` and amend the relevant commit(s).

- [ ] **Step 5: Manual smoke test (optional but recommended given no end-to-end harness covers the TUI event loop)**

If feasible in this environment, run `cargo run -p otto` (or the headless example) against a provider with a deliberately invalid API key and confirm the connect-time note now includes the `--rekey` hint; this is a manual confirmation only, not a substitute for the automated tests in Tasks 2-4.

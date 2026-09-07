# Quiet Startup Provider Errors — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make provider auto-connect at startup silent by default (except the "nothing connected" case), reject a present-but-invalid API key at connect time instead of registering a falsely-healthy provider, recognize env-var-only credentials at startup, and attribute turn-time provider failures to the provider that produced them. Closes `savvagent/otto#14`.

**Architecture:** Four independent-but-sequenced slices, in dependency order: (0) fix `ErrorKind` misclassification in Gemini/OpenAI's `list_models` error path so "Network" genuinely means "unreachable," not "any HTTP error"; (1) turn `build_dynamic_caps`/`try_build_registration` from `Result<Option<...>, String>` into explicit three-way outcome enums (`DynamicCapsOutcome`, `ProviderBuildOutcome`) so "no credentials," "credentials rejected," and "connected" are distinguishable at all three call sites (startup, `/connect`, pool-add-after-effect); (1a) let the auto-discovery path recognize an env-var key when the keyring is empty; (2) gate startup notes behind a new `[startup] verbose` config flag, threaded through `HostBoot`/`App` as a plain bool so the runtime pool-add drain can gate correctly too; (3) attribute `HostError::Provider` to the provider's display name.

**Tech Stack:** Existing workspace crates only (`otto-protocol`, `provider-anthropic`/`-gemini`/`-openai`, `otto-host`, `otto` crate, `rust-i18n`). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-07-quiet-startup-provider-errors-design.md`

**Branch:** `host/quiet-startup-noise` (already created off `origin/main`).

---

## File Map

**Modified files**
- `crates/provider-gemini/src/models.rs`, `crates/provider-gemini/src/lib.rs` — extract `status_to_error_kind`, share it between `complete` and `list_models`.
- `crates/provider-openai/src/lib.rs` — same extraction for OpenAI.
- `crates/otto/src/plugin/builtin/provider_common.rs` — `DynamicCapsOutcome` enum + `build_dynamic_caps` branching.
- `crates/otto/src/plugin/builtin/provider_anthropic/mod.rs`, `provider_gemini/mod.rs`, `provider_openai/mod.rs`, `provider_local/mod.rs` — `ProviderBuildOutcome` enum + `try_build_registration` changes; cloud shims gain env-var fallback (1a).
- `crates/otto/src/main.rs` — `HostBoot` gains `startup_verbose: bool`; `bootstrap_pool_host`'s `try_provider!` macro, `perform_connect`, `apply_pending_pool_add` (gains `startup: bool` param) all updated for the new outcome enums and verbose gating.
- `crates/otto/src/app.rs` — `App` gains `startup_verbose: bool`, set from `HostBoot` at construction.
- `crates/otto/src/config_file.rs` — `StartupSection::verbose: bool` (default `false`); existing `StartupSection { .. }` literals updated.
- `crates/otto-host/src/pool.rs` — `ProviderLease::display_name()` accessor.
- `crates/otto-host/src/session.rs` — `HostError::Provider` tuple → struct variant, `Display` impl, the one construction site.
- `README.md` — one-line mention of `[startup] verbose`.

**No new files. No locale file changes** (existing keys reused verbatim).

---

## Task 1: Fix `ErrorKind` misclassification in Gemini/OpenAI `list_models`

**Files:**
- Modify: `crates/provider-gemini/src/lib.rs`, `crates/provider-gemini/src/models.rs`
- Modify: `crates/provider-openai/src/lib.rs`

- [ ] **Step 1: Read the reference implementation**

View `crates/provider-anthropic/src/lib.rs:191-202` (`status_to_error_kind`) and its call sites in `complete`'s error path and `crates/provider-anthropic/src/models.rs`'s `list_models` error path, to confirm the exact signature/shape to mirror: `pub(crate) fn status_to_error_kind(status: u16) -> ErrorKind`.

- [ ] **Step 2: Add a failing test for Gemini (red)**

In `crates/provider-gemini/src/models.rs`, find the existing `list_models` test module. Add a test (mirroring Anthropic's `list_models_propagates_http_failure`-style test) that mocks a 401 response and asserts `ErrorKind::Authentication`, e.g.:

```rust
#[tokio::test]
async fn list_models_maps_401_to_authentication() {
    // ... mock server returning 401 ...
    let err = provider.list_models().await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Authentication);
}
```

Run `cargo test -p provider-gemini list_models_maps_401_to_authentication 2>&1 | tail -20` — expect failure (currently returns `ErrorKind::Network`).

- [ ] **Step 3: Extract and fix Gemini's status mapping (green)**

In `crates/provider-gemini/src/lib.rs`, locate the existing inline status→kind match used by `complete`'s error path (per spec: has the correct arms already). Extract it into `pub(crate) fn status_to_error_kind(status: u16) -> ErrorKind`. In `crates/provider-gemini/src/models.rs:61-71`, replace the hardcoded `ErrorKind::Network` with a call to `crate::status_to_error_kind(status)`.

Run `cargo test -p provider-gemini 2>&1 | tail -20` — expect green, including the new test.

- [ ] **Step 4: Same fix for OpenAI**

In `crates/provider-openai/src/lib.rs`, extract the correct status→kind match (from `parse_error_response`, used by `complete`) into `pub(crate) fn status_to_error_kind(status: u16) -> ErrorKind`. Update `http_status_error` (used by `list_models`, lines ~241-259) to call it instead of hardcoding `ErrorKind::Network`. Add the equivalent 401-mapping test.

Run `cargo test -p provider-openai 2>&1 | tail -20` — expect green.

- [ ] **Step 5: Full workspace check**

Run `cargo build --workspace 2>&1 | tail -20` and `cargo clippy -p provider-gemini -p provider-openai --all-targets 2>&1 | tail -30` — expect clean.

- [ ] **Step 6: Commit**

```bash
git add crates/provider-gemini crates/provider-openai
git commit -m "fix(provider-gemini,provider-openai): classify list_models HTTP errors by status

Both crates hardcoded ErrorKind::Network for every non-2xx list_models
response, unlike provider-anthropic's status_to_error_kind. A 401/403/429
from the model-catalog endpoint was indistinguishable from a genuine
connectivity failure. Extract each crate's existing (correct) complete-path
status mapping into a shared status_to_error_kind helper and reuse it for
list_models too."
```

---

## Task 2: `DynamicCapsOutcome` in `build_dynamic_caps`

**Files:**
- Modify: `crates/otto/src/plugin/builtin/provider_common.rs`

- [ ] **Step 1: Read current implementation**

View `crates/otto/src/plugin/builtin/provider_common.rs:183-216` (`build_dynamic_caps`) and its current callers (the four `try_build_registration` functions) to confirm the current `(ProviderCapabilities, Option<String>)` return shape and call sites.

- [ ] **Step 2: Add the enum and a failing test (red)**

Add:

```rust
pub(crate) enum DynamicCapsOutcome {
    /// Live capabilities (or a static fallback) plus an optional note about
    /// why the fallback was used.
    Ready(ProviderCapabilities, Option<String>),
    /// The stored key was rejected (or rate-limited/quota-exhausted) by
    /// list_models; the provider must not be registered.
    Rejected(String),
}
```

Add a unit test in the same file (or its test module) that feeds a mock `list_models` returning `Err(ErrorKind::Authentication, "invalid api key")` and asserts `DynamicCapsOutcome::Rejected(_)` is returned; and one for `ErrorKind::Network` asserting `DynamicCapsOutcome::Ready(_, Some(_))` (unchanged fallback behavior). Run `cargo test -p otto provider_common 2>&1 | tail -30` — expect the new tests to fail to compile/fail (enum doesn't exist yet / branching not yet implemented).

- [ ] **Step 3: Implement the branching (green)**

Change `build_dynamic_caps`'s return type to `DynamicCapsOutcome`. Match on the `list_models` error's `ErrorKind`:
- `Ok(models)` → `Ready(caps_from(models), None)` (unchanged path, renamed).
- `Err(e) if matches!(e.kind, ErrorKind::NotImplemented | ErrorKind::Network)` → `Ready(static_fallback_caps, Some(fallback_note))` (unchanged behavior, just now wrapped in `Ready`).
- `Err(e)` (every other kind) → `Rejected(e.message.clone())` (or an equivalent human-readable reason — do not lose the upstream message).

Run `cargo test -p otto provider_common 2>&1 | tail -30` — expect green.

- [ ] **Step 4: Commit**

```bash
git add crates/otto/src/plugin/builtin/provider_common.rs
git commit -m "feat(otto): build_dynamic_caps returns DynamicCapsOutcome

Distinguishes a rejected/unusable key (Authentication, RateLimited, etc.)
from a transient list_models failure (Network, NotImplemented) that should
still fall back to the static catalog. Callers are updated in the next
commit."
```

(Note: this commit will not compile standalone since callers aren't updated yet — the workspace does not build cleanly again until Task 5's commit lands (Tasks 2, 3, 4, and 5 together form the minimal buildable sequence: `DynamicCapsOutcome` → `ProviderBuildOutcome` shims → `startup_verbose` plumbing → `main.rs` call-site rewiring). Keep the intermediate non-compiling commits on the branch and let CI validate the combined state at the end of Task 5, or squash at merge time per repo convention. If the repo's bacon/CI setup requires every commit to build, combine Tasks 2-5 into fewer commits instead.)

---

## Task 3: `ProviderBuildOutcome` in the four provider shims

**Files:**
- Modify: `crates/otto/src/plugin/builtin/provider_anthropic/mod.rs`
- Modify: `crates/otto/src/plugin/builtin/provider_gemini/mod.rs`
- Modify: `crates/otto/src/plugin/builtin/provider_openai/mod.rs`
- Modify: `crates/otto/src/plugin/builtin/provider_local/mod.rs`

- [ ] **Step 1: Read all four `try_build_registration` implementations**

View each file's `try_build_registration` in full (anthropic: `mod.rs:147-224` per spec) to understand the current `Result<Option<(ProviderRegistration, Option<String>)>, String>` shape and the `creds::load` call at the top of each cloud shim.

- [ ] **Step 2: Add the enum**

In a shared location reachable by all four shims (`provider_common.rs` is the natural home, matching `DynamicCapsOutcome`'s location):

```rust
pub(crate) enum ProviderBuildOutcome {
    /// No key in the keyring and none in the environment fallback.
    NoCredentials,
    /// A key was found but list_models rejected it (or a rate limit/quota
    /// error occurred).
    Rejected(String),
    /// A working client was built; optional fallback-catalog note.
    Ready(ProviderRegistration, Option<String>),
}
```

- [ ] **Step 3: Update `provider_local::try_build_registration` first (simplest, no credentials/env fallback)**

`provider_local` has no API key concept (per spec, step 1a excludes it). Change its return type to `Result<ProviderBuildOutcome, String>`, mapping its current `Ok(Some(...))` → `Ok(Ready(...))`, `Ok(None)` → `Ok(NoCredentials)` (if that branch exists) or drop if unreachable, and thread its `DynamicCapsOutcome` match (from Task 2) into `Rejected`/`Ready` appropriately. Run `cargo build -p otto 2>&1 | tail -30` — expect it to fail elsewhere (other three shims + call sites not yet updated) — that's expected at this point; just confirm no error originates from `provider_local` itself (check the error list is scoped to the other 3 files/main.rs).

- [ ] **Step 4: Update the three cloud shims (Anthropic, Gemini, OpenAI)**

For each: change `try_build_registration`'s return type to `Result<ProviderBuildOutcome, String>`. Map `build_dynamic_caps`'s new `DynamicCapsOutcome::Rejected(reason)` → `Ok(ProviderBuildOutcome::Rejected(reason))`; `DynamicCapsOutcome::Ready(caps, note)` → `Ok(ProviderBuildOutcome::Ready(registration_with(caps), note))`. Where `creds::load` currently short-circuits to `Ok(None)`, change to `Ok(NoCredentials)` for now (env-var fallback is Task 6, done later once the workspace builds again, so this task stays reviewable in isolation).

Run `cargo build -p otto 2>&1 | tail -40` — expect remaining errors confined to `main.rs`'s three call sites (fixed in Task 5) — confirm no errors from the shim files themselves.

- [ ] **Step 5: Commit**

```bash
git add crates/otto/src/plugin/builtin/provider_common.rs crates/otto/src/plugin/builtin/provider_anthropic crates/otto/src/plugin/builtin/provider_gemini crates/otto/src/plugin/builtin/provider_openai crates/otto/src/plugin/builtin/provider_local
git commit -m "feat(otto): try_build_registration returns ProviderBuildOutcome

Three-way outcome (NoCredentials / Rejected / Ready) replaces the
two-way Result<Option<...>, String> so callers can distinguish a missing
key from a present-but-rejected one. main.rs call sites updated next."
```

---

## Task 4: `StartupSection::verbose` + `HostBoot`/`App` plumbing

**Files:**
- Modify: `crates/otto/src/config_file.rs`
- Modify: `crates/otto/src/main.rs`
- Modify: `crates/otto/src/app.rs`

- [ ] **Step 1: Add the config field**

In `crates/otto/src/config_file.rs`, add `verbose: bool` (default `false`, `#[serde(default)]`) to `StartupSection`, next to `connect_timeout_ms`/`startup_providers`. Grep `StartupSection {` across `crates/otto/src/config_file.rs` (existing literals around lines ~830 and ~871 construct `StartupSection` explicitly, e.g. in `Default` impls and tests) and update every explicit struct literal to include `verbose: false` (or switch the literal to `..Default::default()` where that's already the pattern used for other fields). Add/extend a round-trip serialization test confirming `[startup] verbose = true` parses correctly and the field defaults to `false` when omitted.

Run `cargo test -p otto config_file 2>&1 | tail -20` — expect green (this step must not leave any `StartupSection { .. }` literal missing the new field, or the crate won't compile).

- [ ] **Step 2: Add `startup_verbose` to `HostBoot` and `App`**

In `crates/otto/src/main.rs`: add `pub startup_verbose: bool` to `HostBoot` (~line 221-233); set it inside `bootstrap_pool_host` (~line 495) from the `config_file: &ConfigFile` parameter it already receives (`config_file.startup.verbose`). In `crates/otto/src/app.rs`, add a matching `startup_verbose: bool` field to the `App` struct (near its other startup-derived fields) and initialize it in `App::new`; then in `main.rs`'s `build_app_with_host`, set `app.startup_verbose` from `initial.as_ref().map(|b| b.startup_verbose).unwrap_or(false)` after `App::new(...)` is called.

Run `cargo build -p otto 2>&1 | tail -20` — expect green (additive fields only).

- [ ] **Step 3: Gate the `try_provider!` macro's notes**

In `bootstrap_pool_host`'s `try_provider!` macro, wrap the existing `deferred_notes.push(...)` calls for `notes.startup-build-failed`, `notes.startup-timeout`, `notes.list-models-fell-back`, `notes.list-models-empty`, and the new `Rejected`-reason note behind `if config_file.startup.verbose { ... }`, keeping the existing unconditional `tracing::warn!`/`tracing::debug!` calls. Leave `notes.not-connected-startup` unconditional. (Note: the `Rejected`-reason note itself is wired in Task 5, once `try_provider!` is updated to match on `ProviderBuildOutcome` — this step only needs to gate the *existing* four note kinds; revisit this gating when Task 5 adds the fifth.)

- [ ] **Step 4: Commit**

```bash
git add crates/otto/src/config_file.rs crates/otto/src/main.rs crates/otto/src/app.rs
git commit -m "feat(otto): add [startup] verbose config flag; gate startup notes

Per-provider auto-connect notes (build failures, timeouts, catalog
fallback) are now only pushed into the transcript when
[startup] verbose = true (default false). They remain fully logged via
tracing::warn regardless. HostBoot/App carry startup_verbose forward so
the async pool-add drain (apply_pending_pool_add, Task 5) can apply the
same gate."
```

---

## Task 5: Update `main.rs` call sites for `ProviderBuildOutcome`

**Depends on Task 3 (introduces `ProviderBuildOutcome`) and Task 4 (introduces `config_file.startup.verbose` / `app.startup_verbose`).** Tasks 2, 3, and this task together are the minimal set that must land before `cargo build --workspace` succeeds again — Task 3's shim changes alone do not compile until this task's call-site updates land, since `try_provider!`, `perform_connect`, and `apply_pending_pool_add` still match on the old `Result<Option<...>, String>` shape until this task runs. Land Tasks 2, 3, 4, and 5 as one buildable sequence (separate commits are fine; each commit does not need to compile in isolation, but the branch must build after Task 5's commit).

**Files:**
- Modify: `crates/otto/src/main.rs`

- [ ] **Step 1: `try_provider!` macro (startup)**

Update the macro body to match on `ProviderBuildOutcome`: `NoCredentials` → unchanged (skip, no note). `Rejected(reason)` → do not add to `providers`; `tracing::warn!(provider = ..., reason = %reason, "provider key rejected at startup")`; push a `notes.startup-build-failed`-shaped note (reusing the existing locale key/format) gated behind `config_file.startup.verbose` (per Task 4 Step 3 — this is the fifth gated note kind referenced there). `Ready(reg, note)` → unchanged existing handling (register + optional gated fallback note).

- [ ] **Step 2: `perform_connect` (explicit `/connect`)**

Update its match on `try_build_registration`'s result: `NoCredentials` → unchanged existing `notes.connect-keyring-not-found` message. `Rejected(reason)` → push `notes.connect-failed` with `err = reason`, **unconditionally** (not gated by verbose — this is a direct response to a user action). `Ready(reg, note)` → unchanged.

- [ ] **Step 3: `apply_pending_pool_add` — add `startup: bool` parameter**

Change the signature to `pub(crate) async fn apply_pending_pool_add(app: &mut App, host_slot: &HostSlot, startup: bool)`. Update its match on `try_build_registration`'s result:
- `Ready(reg, Some(note))` → gate `app.push_note(note)` behind `!startup || app.startup_verbose`.
- `NoCredentials` → unchanged control flow, but gate the existing `notes.connect-keyring-not-found` push the same way.
- `Rejected(reason)` → gate the `notes.connect-failed` push the same way.
- The `Err` branches from `host.add_provider(...)` (excluding `AlreadyRegistered`, which stays a silent no-op) → gate their `notes.connect-failed` push the same way.
- Verify (re-read the function body) that all of these gated pushes still occur **before** the `PoolError::AlreadyRegistered` check inside the `host.add_provider(reg).await` match, preserving current code order — only the `push_note` calls become conditional, nothing is reordered.

- [ ] **Step 4: Update all four call sites of `apply_pending_pool_add`**

The startup call (~line 3071, immediately after the `HostStarting` dispatch inside `run_app`'s startup sequence) passes `true`. The other three calls (~1092, ~3478, ~3764 — all reached after the TUI event loop is running, in response to slash-command dispatch or bound actions) pass `false`.

- [ ] **Step 5: Build and fix remaining compile errors**

Run `cargo build --workspace 2>&1 | tail -60`. Fix any remaining type mismatches at these call sites and anywhere else the old `Result<Option<...>, String>`/tuple shape was matched on directly (grep `try_build_registration` and `build_dynamic_caps` across `crates/otto/src` to confirm no stragglers). This is the point at which the workspace must build cleanly again.

- [ ] **Step 6: Run existing tests**

Run `cargo test -p otto 2>&1 | tail -60` — fix any tests that asserted on the old return shapes.

- [ ] **Step 7: Commit**

```bash
git add crates/otto/src/main.rs
git commit -m "feat(otto): wire ProviderBuildOutcome through startup, /connect, pool-add

Startup rejects a bad key without registering it (gated note behind
[startup] verbose). /connect and the pool-add drain surface the real
rejection reason via the existing notes.connect-failed key instead of a
misleading 'key not found' message. apply_pending_pool_add gains a
startup: bool so its one startup-triggered call site (right after
HostStarting) stays quiet by default while the three runtime call sites
(slash commands, bound actions) keep today's always-shown /connect UX."
```

---

## Task 6: Env-var credential recognition (step 1a)

**Depends on Task 5 (workspace must build cleanly with `ProviderBuildOutcome` wired through before adding a new branch to it).**

**Files:**
- Modify: `crates/otto/src/plugin/builtin/provider_anthropic/mod.rs`, `provider_gemini/mod.rs`, `provider_openai/mod.rs`

- [ ] **Step 1: Establish a test seam before writing tests**

The cloud shims currently construct concrete providers against their real default API URLs and invoke `list_models` directly with no injectable seam — the existing `provider_common.rs` keyring mock and each plugin's `with_test_client`-style helpers only affect rendering/plugin-level tests, not `try_build_registration` itself. Before writing the env-var tests below, add a minimal seam: factor the "resolve credential source, then call `.build()` and validate via `build_dynamic_caps`" logic behind a small internal function that accepts an injectable `list_models` result (or an injectable base URL pointed at a local mock HTTP server, mirroring the Axum-based mock-server pattern already used in `provider-anthropic`/`provider-gemini`/`provider-openai`'s own crate-level tests). Keep this seam test-only (`#[cfg(test)]`) and scoped to each shim file; do not change the public `try_build_registration` signature beyond what Task 3 already introduced.

- [ ] **Step 2: Add failing tests (red)**

For each of the three cloud shims, using the Step 1 seam, add a test that: clears/mocks the keyring lookup to return `Ok(None)`, sets the vendor env var(s) to a non-empty value via `std::env::set_var` (test-scoped; serialize these tests or use a per-test env-var guard, since `std::env::set_var` is process-global and cloud-shim tests may run concurrently — check existing tests in these files for a `#[serial]`-style pattern or add one), and asserts `try_build_registration` reaches `Ready` via the mocked `list_models` success (not `NoCredentials`). For Gemini specifically, add **two** tests: one setting only `GEMINI_API_KEY`, one setting only `GOOGLE_API_KEY` — both must be recognized, since `provider-gemini`'s builder falls back to either.

Run `cargo test -p otto provider_anthropic:: provider_gemini:: provider_openai:: 2>&1 | tail -40` — expect the new tests to fail.

- [ ] **Step 3: Implement (green)**

In each shim, where `creds::load(PROVIDER_ID)` returns `Ok(None)` **or `Err(_)`** (log the `Err` case via `tracing::warn!` — a keyring backend fault must not silently block an env-only setup, but should still be diagnosable), check whether the vendor's env var(s) yield a **non-empty** string before returning `NoCredentials`: Anthropic checks `ANTHROPIC_API_KEY`; OpenAI checks `OPENAI_API_KEY`; Gemini checks whether **either** `GEMINI_API_KEY` **or** `GOOGLE_API_KEY` is non-empty (matching the builder's own two-variable fallback order). If a non-empty value is present, call `.build()` without `.api_key(..)` so the provider builder's existing env fallback resolves it, then proceed through the same `build_dynamic_caps` validation as the keyring path.

Run `cargo test -p otto provider_anthropic:: provider_gemini:: provider_openai:: 2>&1 | tail -40` — expect green.

- [ ] **Step 4: Commit**

```bash
git add crates/otto/src/plugin/builtin/provider_anthropic/mod.rs crates/otto/src/plugin/builtin/provider_gemini/mod.rs crates/otto/src/plugin/builtin/provider_openai/mod.rs
git commit -m "feat(otto): recognize env-var-only credentials at auto-discovery

try_build_registration previously short-circuited to NoCredentials
whenever the keyring had no entry, even if the vendor's env var(s)
(ANTHROPIC_API_KEY, GEMINI_API_KEY/GOOGLE_API_KEY, OPENAI_API_KEY) were
set — the provider builders already support that fallback but the TUI
shim never reached it. A keyring backend error is now treated the same
as \"no entry\" for this fallback check only, and is still logged."
```

---

## Task 7: Attribute turn-time `HostError::Provider` to the provider

**Files:**
- Modify: `crates/otto-host/src/pool.rs`
- Modify: `crates/otto-host/src/session.rs`

- [ ] **Step 1: Add `ProviderLease::display_name()`**

In `crates/otto-host/src/pool.rs`, add a `display_name(&self) -> &str` (or `String`, matching `PoolEntry::display_name()`'s existing return type) accessor to `ProviderLease`, populated from the owning `PoolEntry::display_name` inside `PoolEntry::lease()`. Add/extend a unit test asserting a leased provider's `display_name()` matches its `PoolEntry`'s.

Run `cargo test -p otto-host pool 2>&1 | tail -30` — expect green.

- [ ] **Step 2: Add a failing test for the new `Display` format (red)**

In `crates/otto-host/src/session.rs`'s test module, add/update a test asserting `HostError::Provider { name: "Anthropic".into(), error }.to_string()` equals `"Anthropic rejected the request: <message>"`. Run `cargo test -p otto-host session 2>&1 | tail -30` — expect failure (variant doesn't exist yet / wrong Display format).

- [ ] **Step 3: Change the variant shape and `Display` impl (green)**

Change `HostError::Provider(ProviderError)` (tuple, ~line 135-137) to `Provider { name: String, error: ProviderError }` (struct variant). Update its `Display` impl to `"{name} rejected the request: {message}"` using `error.message`. Confirm via `grep -rn "HostError::Provider" crates/` that the only construction site is in `Host::run_turn_streaming` and update it: capture the lease's `display_name()` into a local `String` **before** the existing `drop(lease)` call, then change `resp_result.map_err(HostError::Provider)?` (~line 1288) to `resp_result.map_err(|error| HostError::Provider { name: provider_display_name, error })?`.

Run `cargo test -p otto-host 2>&1 | tail -40` — expect green.

- [ ] **Step 4: Confirm the TUI rendering picks it up unchanged**

View `crates/otto/src/main.rs:3230`'s `Entry::Note(format!("Error: {msg}"))` rendering — confirm it needs no code change (it renders `HostError`'s `Display` output, which now includes the provider name automatically).

- [ ] **Step 5: Commit**

```bash
git add crates/otto-host/src/pool.rs crates/otto-host/src/session.rs
git commit -m "feat(otto-host): attribute HostError::Provider to the provider

HostError::Provider changes from a tuple variant to a struct variant
carrying the provider's display name (via the new
ProviderLease::display_name() accessor), captured before the lease is
dropped. Display now renders '<Provider> rejected the request: <message>'
instead of the unattributed 'provider error: <Kind>: <message>'.
HostError is #[non_exhaustive] and this is the sole construction site, so
no external match arms are affected."
```

---

## Task 8: Full validation pass

**Files:** none (validation only)

- [ ] **Step 1: Full test suite**

Run `cargo test --workspace 2>&1 | tail -100`. Fix any remaining failures.

- [ ] **Step 2: Clippy**

Run `cargo clippy --workspace --all-targets 2>&1 | tail -100`. Fix any new warnings introduced by this change (pre-existing warnings unrelated to these files are out of scope).

- [ ] **Step 3: Format check**

Run `cargo fmt --all --check 2>&1 | tail -60`. If it fails, run `cargo fmt --all` and review the diff is confined to touched files before committing.

- [ ] **Step 4: README update**

Add a one-line mention of the new `[startup] verbose` config key to `README.md`'s config-file documentation section (find the existing `[startup]` section entries like `connect_timeout_ms`/`startup_providers` and add `verbose` alongside them, following the same doc format).

- [ ] **Step 5: Manual smoke test (documented, not automated)**

If feasible in this environment, manually verify: (a) startup with one valid provider produces no startup notes; (b) `/connect` with a deliberately-invalid key shows the real rejection reason, not "key not found." If not feasible without live API keys, note this in the PR description as a manual-verification gap covered by the unit/integration tests added in Tasks 1-7 instead.

- [ ] **Step 6: Commit any remaining fixes**

```bash
git add -A
git commit -m "chore: fmt/clippy fixes and README update for [startup] verbose"
```

---

## Post-implementation

- [ ] Update spec `Status:` header from `pending review` to `IMPLEMENTED` in a final doc-only commit once the PR is open (or once merged, per repo convention — check other recent specs for the exact convention used).
- [ ] Dispatch the mandatory review trio (Rust-expert, architecture, security-review) before opening the PR.
- [ ] Open PR referencing `savvagent/otto#14`; merge via squash after review sign-off; cut a release per `RELEASING.md`.

# issue-68-provider-selector-fuzzy-search Implementation Plan

**Goal:** Enhance the existing `/connect` provider selector so users can type to fuzzy-filter the provider list in place, while preserving the current keyring-backed API-key behavior for keyed providers and the immediate-connect flow for keyless providers.

**Architecture:** The provider catalog remains `crate::providers::effective_providers()`; this plan adds small selector/search helpers around that catalog rather than creating a second registry. `crates/otto/src/app.rs` owns the selector query state plus filtered-selection helpers, `crates/otto/src/main.rs` continues to own the keyboard event loop and credential branching, and `crates/otto/src/ui.rs` renders the same modal with an optional filter row / empty state. No `otto-host` changes, no transport changes, and no secret-storage changes.

**Tech Stack:** Rust 2024, existing `ratatui`/`crossterm` TUI stack, existing `tui-textarea` for the API-key modal only, existing keyring access through `crate::creds`, and unit tests inside the `otto` crate.

**Spec:** `docs/superpowers/specs/2026-09-07-issue-68-provider-selector-fuzzy-search-design.md` — read it first. This plan implements it exactly.

**Release line:** `v0.26.0` (MINOR: new `/connect` interaction capability, no public-interface break)

**Branch:** `otto/connect-provider-fuzzy-search`

## File Map

**New files:**
- `docs/superpowers/specs/2026-09-07-issue-68-provider-selector-fuzzy-search-design.md` (already committed)
- `docs/superpowers/plans/2026-09-07-issue-68-provider-selector-fuzzy-search.md` (this file)

**Modified files:**
- `crates/otto/src/providers.rs` — provider-selector search threshold and/or provider-query helper(s) shared by the `/connect` flow.
- `crates/otto/src/app.rs` — provider-selector query state, filtered-provider helpers, API-key-modal entry updated to take the selected filtered provider directly, and unit tests for fuzzy matching / cursor behavior.
- `crates/otto/src/main.rs` — `InputMode::SelectingProvider` keyboard handling for typing/backspace/escape/enter over the filtered selector while preserving stored-key vs prompt gating.
- `crates/otto/src/ui.rs` — `/connect` modal render changes for the optional filter row, filtered list, and no-results state.

## Task 1: Add provider-selector fuzzy state and helper coverage

**Files:**
- Modify: `crates/otto/src/providers.rs`
- Modify: `crates/otto/src/app.rs`

- [ ] Add failing unit tests first in `crates/otto/src/app.rs` covering: empty query returns the full provider list; fuzzy query matches both provider id and display name; queries with no matches return an empty filtered list; cursor clamping/reset keeps `provider_index` valid when the filtered result set shrinks; clearing the query restores the active provider when visible or the first provider otherwise; opening the selector resets stale query state and prefers the active provider when visible; `enter_api_key_for` (or its replacement helper) preserves the existing placeholder behavior for stored vs missing credentials.
- [ ] Run `cargo test -p otto -- provider_selector` and expect the new selector-focused tests to fail before implementation because the filter/query helpers do not exist yet.
- [ ] In `crates/otto/src/providers.rs`, add the minimal shared selector metadata needed by the feature: a discoverability threshold constant for when the `/connect` modal should show its filter row without prior typing, and/or a small provider-query helper that matches a provider against a case-insensitive subsequence query using `display_name` + `id`. Keep the catalog source of truth as `effective_providers()`; do not add a second registry.
- [ ] In `crates/otto/src/app.rs`, add provider-selector query state plus helper methods that: derive the filtered provider list from `effective_providers()`, update/reset the query, clamp `provider_index` into the filtered range, resolve the currently selected filtered provider, and enter the API-key modal using the selected `ProviderSpec` directly instead of indexing the unfiltered catalog. Keep the current keyring-placeholder behavior intact.
- [ ] Re-run `cargo test -p otto -- provider_selector` and expect the selector helper tests to pass.
- [ ] Public-interface check: record that this task changes no SPP wire format, tool schema, plugin ABI, slash command, env var, or on-disk format; any new helper in `providers.rs` is internal-only.
- [ ] Host-swap `RwLock` check: this task touches `crates/otto/src/app.rs` but must not introduce any new `.await` or host-lock usage there; verify the new selector helpers are synchronous state transforms only.
- [ ] ProgressDispatcher check: not applicable — no streaming provider path is touched.
- [ ] Format and commit: `cargo fmt --all` then `git commit -m "otto: add provider selector fuzzy state helpers"`.

## Task 2: Wire typed filtering into the `/connect` modal and validate behavior

**Files:**
- Modify: `crates/otto/src/main.rs`
- Modify: `crates/otto/src/ui.rs`
- Modify: `crates/otto/src/app.rs`

- [ ] Add failing tests first in the narrowest files that pin the user-visible behavior: in `crates/otto/src/ui.rs`, cover the short-list hidden filter row, long-list discoverable filter row, and no-results empty-state rendering; in `crates/otto/src/main.rs` and/or `crates/otto/src/app.rs`, cover `InputMode::SelectingProvider` typing/backspace/escape behavior and confirm Enter still routes keyed providers through stored-key lookup before falling back to the API-key modal, while keyless providers skip prompting entirely; include the “clear query returns focus to the active provider / first provider” path after Backspace or Esc.
- [ ] Run `cargo test -p otto -- connect_provider_selector` (or the exact new test-name substring) and expect the new UI/input behavior tests to fail before implementation.
- [ ] In `crates/otto/src/main.rs`, extend the `InputMode::SelectingProvider` key handling so printable characters append to the selector query, Backspace deletes, Esc clears the active query before dismissing the modal, Up/Down move within the filtered result set, and Enter selects the filtered provider only when a match exists. Preserve the existing `api_key_required` / `creds::load(spec.id)` branching exactly: keyless providers connect immediately; keyed providers use the stored credential when present; keyed providers open the API-key modal only when the key is absent (or a keyring read error surfaces the existing note + prompt fallback).
- [ ] In `crates/otto/src/ui.rs`, render the filtered provider list from the new `App` helpers, add a compact filter row that is visible only when the query is non-empty or the provider count exceeds the discoverability threshold, and render a clear no-results message when the current query matches nothing. Keep the modal keyboard hints aligned with the new typed-filter behavior and preserve the uncluttered short-list presentation when the query is empty.
- [ ] Re-run `cargo test -p otto -- connect_provider_selector` and expect the new selector UI/input tests to pass.
- [ ] Run `cargo build --workspace --all-targets` and expect a green build.
- [ ] Run `cargo test --workspace --no-fail-fast` and expect a green test suite.
- [ ] Run `cargo clippy --workspace --all-targets` and expect zero warnings/errors.
- [ ] Run `cargo fmt --all --check` and expect no formatting drift.
- [ ] Run `cargo run -p otto` if the environment can launch an interactive TUI, and manually verify that `/connect` initially shows the uncluttered short list, typing filters the provider list in place, no-match queries show an empty state without closing the modal, Enter on a keyless provider skips the API-key prompt, and Enter on a keyed provider with no stored credential opens the API-key modal. If the environment is headless, record that limitation in the implementation report and rely on the automated checks plus the focused selector tests.
- [ ] Public-interface check: confirm again that the shipped diff is internal TUI behavior only — no schema/ABI/slash/env/on-disk changes.
- [ ] Host-swap `RwLock` check: this task touches `crates/otto/src/main.rs` and may make follow-on edits in `crates/otto/src/app.rs`; preserve the existing discipline that any `Arc<RwLock<Option<Arc<Host>>>>` guard is cloned/dropped before `.await`, and do not add any new host-lock-taking code to the selector branch.
- [ ] ProgressDispatcher check: not applicable — no streaming provider path is touched.
- [ ] Format and commit: `cargo fmt --all` then `git commit -m "otto: wire typed filtering into connect selector"`.

## Task 3: Note the mandatory follow-on release PR

**Files:**
- Modify: none in this feature branch (record only in PR body / workflow tracking)

- [ ] Note in the feature PR body that this ticket is not fast-path because it adds a new interactive `/connect` affordance across multiple runtime files/state-machine paths.
- [ ] Note in the feature PR body that a dedicated release PR will follow immediately after merge, per `RELEASING.md`, to bump `workspace.package.version` and internal `workspace.dependencies` versions to `0.26.0`, update `CHANGELOG.md`, and tag/push `v0.26.0`.
- [ ] Format and commit: no code commit required for this note-only task; it is satisfied during Phase 4 PR authoring and the post-merge release workflow (which this run intentionally stops before executing).

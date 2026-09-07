# issue-65-self-update-fix Implementation Plan

**Goal:** Restore reliable self-update detection and install behavior so Otto notices newly published releases promptly at startup and `/update` performs an authoritative live check before installing the latest release.

**Architecture:** The fix stays inside `crates/otto`'s `internal:self-update` plugin. `check.rs` remains the pure GitHub-release/semver helper, `cache.rs` remains a filesystem TTL cache helper, and `mod.rs` owns the policy that decides when cached state is trustworthy and when `/update` must fetch live data. No `otto-host`, provider-pool, tool-transport, or host-swap changes are in scope.

**Tech Stack:** Rust 2024, tokio async tasks and timers, reqwest-backed GitHub release checks, semver classification, serde-backed config/cache helpers, and existing README slash-command documentation.

**Spec:** `docs/superpowers/specs/2026-09-07-issue-65-self-update-fix-design.md` — read it first. This plan implements it exactly.

**Release line:** `v0.25.3`

**Branch:** `otto/issue-65-self-update-fix`

## File Map

**New files**
- None.

**Modified files**
- `crates/otto/src/plugin/builtin/self_update/mod.rs` — change first-tick cache trust policy, make `/update` do a live re-check, and add regression tests for stale-cache startup and slash-command installs.
- `README.md` — document that `/update` performs a live re-check before deciding whether to install.

## Task 1: Lock in the self-update regressions with tests

**Files:**
- Modify: `crates/otto/src/plugin/builtin/self_update/mod.rs`

- [ ] Add failing tests in `crates/otto/src/plugin/builtin/self_update/mod.rs` covering: (a) a fresh cache entry equal to the running version no longer suppresses a startup fetch when a newer release exists, and (b) `/update` invoked from an `UpToDate` or `Unknown` state performs a live GitHub check and installs when the fetched tag is newer. Expected result: the new tests fail against the current stale-cache and state-only slash behavior.
- [ ] Run `cargo test -p otto --bin otto plugin::builtin::self_update::tests::first_tick_equal_cache_revalidates_live -- --nocapture && cargo test -p otto --bin otto plugin::builtin::self_update::tests::slash_update_rechecks_live_and_installs_when_newer -- --nocapture`. Expected result: one or both tests fail before implementation.
- [ ] Public-interface check: record in the task ledger that the `/update` slash-command surface keeps the same name/args/env vars and only corrects documented behavior.
- [ ] Host-swap/RwLock check: not applicable — no `app.rs` / `tui.rs` lock-bearing async path touched.
- [ ] ProgressDispatcher check: not applicable — no streaming provider path touched.
- [ ] Format and commit: after the task is implemented and passing, run `cargo fmt --all` and `git commit -m "otto: cover self-update stale-cache regressions"`.

## Task 2: Fix startup revalidation, live `/update`, and docs

**Files:**
- Modify: `crates/otto/src/plugin/builtin/self_update/mod.rs`
- Modify: `README.md`

- [ ] Implement the first-tick cache policy in `crates/otto/src/plugin/builtin/self_update/mod.rs` so cached results are trusted only when they already prove a newer release exists; equal-version cache entries must trigger an immediate live `check_for_update` instead of publishing `UpToDate` from cache.
- [ ] Refactor `/update` in `crates/otto/src/plugin/builtin/self_update/mod.rs` so, unless updates are disabled, the command performs a live `check_for_update`, publishes the resulting state, and runs `run_install` when the fetched result is `Available`; preserve the existing in-progress/dev/disabled/restart-needed notes for those states.
- [ ] Update `README.md`'s `/update` row to document the live re-check semantics and the corrected startup-detection behavior.
- [ ] Run `cargo test -p otto --bin otto plugin::builtin::self_update::tests -- --nocapture`. Expected result: self-update plugin tests pass, including the new regressions.
- [ ] Public-interface check: confirm the slash command name, `OTTO_NO_UPDATE_CHECK`, `--no-update-check`, cache file path, and `[update]` config shape stay unchanged; note only the behavior correction in the task ledger and PR body.
- [ ] Host-swap/RwLock check: not applicable — no `Arc<RwLock<Option<Arc<Host>>>>` path touched.
- [ ] ProgressDispatcher check: not applicable — no streaming provider path touched.
- [ ] Format and commit: `cargo fmt --all` and `git commit -m "otto: revalidate self-update state before installing"`.

## Task 3: Validate the branch and prepare release handoff

**Files:**
- Modify only if validation exposes issue-65-related regressions.

- [ ] Run `cargo build --workspace --all-targets`. Expected result: success.
- [ ] Run `cargo test --workspace --no-fail-fast`. Expected result: success.
- [ ] Run `cargo clippy --workspace --all-targets`. Expected result: success.
- [ ] Run `cargo fmt --all --check`. Expected result: success.
- [ ] Public-interface check: verify the eventual PR summary explicitly calls out that startup no longer trusts equal-version update cache entries and that `/update` now performs a live re-check before installing.
- [ ] Host-swap/RwLock check: verify no task introduced an `.await` while holding an `Arc<RwLock<Option<Arc<Host>>>>` guard.
- [ ] ProgressDispatcher check: verify no streaming-provider path was touched, so no forwarder-abort change was required.
- [ ] Format and commit: if validation fixes are needed, run `cargo fmt --all` and `git commit -m "otto: fix self-update validation findings"`; otherwise record in the task ledger that no extra code commit was required.

## Task 4: Note the mandatory post-merge release PR

**Files:**
- No code changes in this branch; this task documents the release handoff only.

- [ ] Record that after this PR merges, a dedicated release PR must bump `workspace.package.version` and internal `workspace.dependencies` versions in `Cargo.toml`, update `CHANGELOG.md` for `v0.25.3`, run the release validation commands from `RELEASING.md`, and publish tag `v0.25.3` per Phase 4 step 12.
- [ ] Public-interface check: confirm the release PR needs only a PATCH bump because issue #65 is a bug fix with no breaking interface shape change.
- [ ] Host-swap/RwLock check: not applicable.
- [ ] ProgressDispatcher check: not applicable.
- [ ] Format and commit: no additional commit required unless the plan itself is revised.

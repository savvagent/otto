# issue-102-splash-test-name Implementation Plan

**Goal:** Rename `splash.rs`'s `shared_content_marks_logo_rows_centered_for_frontend_parity` test to `shared_content_marks_logo_rows_centered_for_startup_and_splash_parity`, matching what the test actually pins now that the egui front-end is gone (`splash::shared_content` has two consumers — `splash::render` and the `/splash` plugin screen — not two front-ends), per `savvagent/otto#102`.

**Architecture:** No behavior change — a `#[test]` fn name only. No public interface touched.

**Tech Stack:** Existing workspace crate (`otto`) only.

**Fast-path:** no design spec per otto-development trivial-task criteria — single-file rename with no behavior delta and a one-sentence acceptance criterion.

**Branch:** `otto/splash-test-name`

## File Map

**Modified files**
- `crates/otto/src/splash.rs` — rename the test function.
- `docs/superpowers/plans/2026-09-10-issue-102-splash-test-name.md` — this plan.

## Task 1: Rename the test

**Files:** Modify `crates/otto/src/splash.rs`

- [x] Rename `shared_content_marks_logo_rows_centered_for_frontend_parity` to `shared_content_marks_logo_rows_centered_for_startup_and_splash_parity` at `crates/otto/src/splash.rs:382`.
- [x] Run `cargo test -p otto -- splash` — confirm the renamed test passes and the old name no longer appears.
- [x] Run `cargo test --workspace` — confirm nothing else references the old name and the workspace stays green.
- [x] Format and commit: `cargo fmt --all` then `git commit -m "otto: rename splash test to describe startup/splash parity, not egui"`.

Release: this is a cosmetic, non-breaking test rename; it rides the next regular release cut rather than forcing an out-of-band one.

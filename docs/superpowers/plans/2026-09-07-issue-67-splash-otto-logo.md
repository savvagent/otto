# issue-67-splash-otto-logo Implementation Plan

**Goal:** Ship issue #67 by replacing the startup splash screen's stale `SAVVAGENT` ASCII-art banner with a clean `OTTO` block logo, keeping the existing splash layout/style intact apart from the displayed text and centering width.

**Architecture:** This is a presentation-only change inside `crates/otto/src/splash.rs`. The splash renderer, sandbox-state logic, and TUI layout stay in place; only the `LOGO` string rows and the width constant used by `center_rect` change so the banner remains horizontally centered with uniform row lengths.

**Tech Stack:** Rust 2024, ratatui splash rendering in `crates/otto/src/splash.rs`, existing unit tests in the same file, and targeted Cargo test/fmt validation.

**Spec:** Fast-path: no design spec per `otto-development` trivial-task criteria. This change is a single-source-file constant update with no public-interface change, no crate-boundary change, no deploy/distribution change, and acceptance criteria that fit in one sentence.

**Release line:** `v0.25.3` (PATCH: visual bug fix, no public-interface change)

**Branch:** `otto/issue-67-splash-logo`

## File Map

**New files:**
- `docs/superpowers/plans/2026-09-07-issue-67-splash-otto-logo.md` — fast-path implementation record for issue #67.

**Modified files:**
- `crates/otto/src/splash.rs` — replace the startup splash ASCII-art rows with an `OTTO` logo and update the centering width constant to the new logo width.

## Task 1: Replace the splash banner with OTTO and validate the width

**Files:**
- Modify: `crates/otto/src/splash.rs`

- [ ] Update `LOGO` in `crates/otto/src/splash.rs` to six box-drawing/block-style rows that clearly spell `OTTO`, matching the existing splash art height and keeping every row the same length via explicit padding where needed.
- [ ] Update `LOGO_WIDTH` in `crates/otto/src/splash.rs` to the actual character width of the new padded logo rows so `center_rect` still centers the splash banner correctly.
- [ ] Verify the row widths directly (script or targeted inspection) and expect every `LOGO` line to report the same length, matching `LOGO_WIDTH`.
- [ ] Run `cargo test -p otto splash::tests` and expect the existing splash tests to pass after the constant change.
- [ ] Run `cargo fmt --all --check` and expect no formatting drift.
- [ ] Public-interface check: record in the task notes/PR body that this task does **not** change the SPP wire format, any tool schema, plugin ABI, slash command, env var, or on-disk format.
- [ ] Host-swap/RwLock check: not applicable — this task does not touch `crates/otto/src/app.rs` or `crates/otto/src/tui.rs`, and it adds no async/lock behavior.
- [ ] ProgressDispatcher check: not applicable — no streaming provider path touched.
- [ ] Format and commit: if needed run `cargo fmt --all`, then `git commit -m "otto: update splash logo to otto"`.

## Task 2: Note the deferred release follow-on

**Files:**
- Modify: none in this feature branch (record only in PR body / workflow tracking)

- [ ] Note in the feature PR body that this issue is fast-path eligible because it only updates a named splash constant and its centering width in one source file.
- [ ] Note in the feature PR body that the normal dedicated release PR for `v0.25.3` remains a follow-on after merge per `RELEASING.md`, but this run intentionally stops before merge because parallel issue work would race the shared release/tag workflow.
- [ ] Format and commit: no code commit required for this note-only task; it is satisfied during Phase 4 PR authoring.

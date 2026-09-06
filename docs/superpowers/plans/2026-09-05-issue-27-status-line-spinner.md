# issue-27-status-line-spinner Implementation Plan

**Goal:** Ship issue #27 by replacing the ratatui TUI footer's text-only working-state indicator with a themed circular spinner from `tui-spinner`, while preserving the idle footer, keeping the GUI footer and plugin ABI unchanged, and validating the visual fix through the repo's standard Rust checks plus a manual TUI run if the environment supports it.

**Architecture:** The footer status semantics remain owned by `internal:home-footer`, which continues to emit localized `StyledLine` text for `home.footer.center`. The TUI-specific presentation change happens one layer later in `crates/savvagent/src/ui.rs`, where the already-flattened footer line is composed for ratatui rendering. `crates/savvagent/src/main.rs` supplies a monotonic render tick plus model-turn-active state from its existing `current_turn_id` loop-local, so the spinner animation stays in the TUI shell and does not leak ratatui widget concerns into `savvagent-plugin`, the shared egui render model, or `savvagent-host`.

**Tech Stack:** Rust 2024, ratatui 0.30, `tui-spinner` 0.4.17 (`CircleSpinner`), existing `savvagent_plugin::StyledLine` footer-slot pipeline, crossterm poll-loop in `crates/savvagent/src/main.rs`, and Rust unit tests in `crates/savvagent/src/ui.rs`.

**Spec:** `docs/superpowers/specs/2026-09-05-issue-27-status-line-spinner-design.md` — read it first. This plan implements it exactly.

**Release line:** `v0.21.1` (PATCH: visual status-line fix, no public-interface change)

**Branch:** `savvagent/status-line-spinner`

**File Map**

**New files:**
- None.

**Modified files:**
- `Cargo.toml` — add the workspace-level `tui-spinner` dependency matching ratatui 0.30.
- `crates/savvagent/Cargo.toml` — opt the `savvagent` crate into `tui-spinner`.
- `crates/savvagent/src/ui.rs` — add the busy-footer spinner composition/rendering helper(s) and unit tests.
- `crates/savvagent/src/main.rs` — thread a render tick and `current_turn_id.is_some()` through the ratatui render call.

## Task 1: Add the dependency and implement/test the TUI footer spinner

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/savvagent/Cargo.toml`
- Modify: `crates/savvagent/src/ui.rs`

- [ ] Add/adjust unit tests in `crates/savvagent/src/ui.rs` first so the new busy-footer helper is covered directly: idle footer unchanged, busy footer includes the working label, busy footer includes spinner glyph output, and two different ticks produce different spinner frames.
- [ ] Run `cargo test -p savvagent --bin savvagent ui::tests` and expect the new footer-focused tests to fail before implementation because the spinner helper/render path does not exist yet.
- [ ] Add `tui-spinner = "0.4.17"` to the root `Cargo.toml` `[workspace.dependencies]` and `tui-spinner.workspace = true` to `crates/savvagent/Cargo.toml`, preserving the existing dependency ordering/comments style.
- [ ] In `crates/savvagent/src/ui.rs`, add a small TUI-only helper that converts `CircleSpinner::new(tick).radius(1)` into the footer's center segment while preserving the existing localized working label as text alongside the spinner and falling back to the prior text-only path if the spinner output is unexpectedly empty.
- [ ] In `crates/savvagent/src/ui.rs`, keep the existing footer-slot flattening semantics for left/right groups, and scope the spinner to the busy center segment only when `turn_active == true`; idle rendering must stay byte-for-byte equivalent to today's footer line.
- [ ] Run `cargo test -p savvagent --bin savvagent ui::tests` again and expect the new/updated footer tests to pass.
- [ ] Public-interface check: record in the task notes/PR body that this task does **not** change the SPP wire format, any tool schema, plugin ABI, slash command, env var, or on-disk format; the new dependency is internal only.
- [ ] Host-swap/RwLock check: not applicable — this task does not touch `crates/savvagent/src/app.rs` or `crates/savvagent/src/tui.rs`, and it adds no `.await` in render code.
- [ ] ProgressDispatcher check: not applicable — no streaming provider path touched.
- [ ] Format and commit: `cargo fmt --all` then `git commit -m "savvagent: add tui-spinner footer busy indicator"`.

## Task 2: Wire the render tick through the TUI loop and validate the full workspace

**Files:**
- Modify: `crates/savvagent/src/main.rs`
- Modify: `crates/savvagent/src/ui.rs`

- [ ] In `crates/savvagent/src/main.rs`, add a monotonic render tick local to `run_app`, increment it once per completed `terminal.draw`, and pass both that tick and `current_turn_id.is_some()` into the `ui::render` call without altering worker ordering or the host-event dispatch flow.
- [ ] In `crates/savvagent/src/ui.rs`, update the render signature and footer call site to consume the new `tick` and `turn_active` parameters without changing unrelated layout behavior.
- [ ] Run `cargo build --workspace --all-targets` and expect a green build.
- [ ] Run `cargo test --workspace` and expect a green test suite.
- [ ] Run `cargo clippy --workspace --all-targets` and expect zero warnings/errors (CI uses `RUSTFLAGS=-D warnings`).
- [ ] Run `cargo fmt --all --check` and expect no formatting drift.
- [ ] Run `cargo run -p savvagent` if this environment can launch an interactive TUI, and manually confirm the footer stays `idle` at rest and swaps to an animated circular spinner once a turn starts; if the environment is headless, record that limitation in the implementation report and rely on the automated build/test/clippy/fmt checks plus the targeted `ui.rs` tests.
- [ ] Public-interface check: confirm again that the merged task remains additive-internal only and needs no README/ABI/schema updates in the feature PR.
- [ ] Host-swap/RwLock check: this task touches `crates/savvagent/src/main.rs`, not `app.rs`/`tui.rs`; preserve the existing discipline of cloning/dropping host locks before `.await` by not changing any lock-taking code paths.
- [ ] ProgressDispatcher check: not applicable — no streaming provider path touched.
- [ ] Format and commit: if Task 1's commit did not already cover all code changes, run `cargo fmt --all` and `git commit -m "savvagent: wire footer spinner animation tick"` for the remaining diff.

## Task 3: Note the mandatory follow-on release PR

**Files:**
- Modify: none in this feature branch (record only in PR body / workflow tracking)

- [ ] Note in the feature PR body that a dedicated release PR will follow immediately after merge, per `RELEASING.md`, to bump `workspace.package.version` and internal `workspace.dependencies` versions to `0.21.1`, rename `CHANGELOG.md`'s `[Unreleased]` section to `0.21.1 - 2026-09-05`, add a fresh empty `[Unreleased]`, and tag/push `v0.21.1`.
- [ ] Note in the PR body that this ticket is **not** fast-path because it adds a new crate dependency edge and changes runtime TUI behavior.
- [ ] Format and commit: no code commit required for this note-only task; it is satisfied during Phase 4 PR authoring and release execution.

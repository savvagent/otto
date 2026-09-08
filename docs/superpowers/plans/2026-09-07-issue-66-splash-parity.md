# issue-66-splash-parity Implementation Plan

**Goal:** Fix `savvagent/otto#66` by making `/splash` render the same startup
splash content and behavior as the initial splash overlay, using a single shared
rendering path and targeted tests to prevent drift.

**Architecture:** `crates/otto/src/splash.rs` remains the sole source of truth
for splash content and sandbox truthfulness. `crates/otto/src/plugin/builtin/splash/`
reuses that shared content helper for `SplashScreen::render`, while
`crates/otto/src/ui.rs` routes the fullscreen ratatui `"splash"` screen id
through the existing centered startup renderer so `/splash` matches startup in
the TUI without leaving egui or other screen consumers blank.

**Tech Stack:** Rust 2024, ratatui fullscreen rendering in `crates/otto`,
built-in plugin screens via `otto-plugin`, and targeted/unit tests in the same
crate.

**Spec:** `docs/superpowers/specs/2026-09-07-issue-66-splash-parity-design.md`
— read it first. This plan implements it exactly.

**Release line:** `v0.25.3` (PATCH: internal bug fix, no public-interface
change)

**Branch:** `otto/issue-66-splash-parity`

**File Map**

**New files:**
- None.

**Modified files:**
- `crates/otto/src/splash.rs` — shared splash renderer remains the single
  source of truth; expose the minimal shared content helper/tests needed to
  support startup and `/splash`.
- `crates/otto/src/ui.rs` — route fullscreen `"splash"` screens through the
  startup renderer and add targeted screen-paint tests.
- `crates/otto/src/plugin/builtin/splash/screen.rs` — simplify the screen to
  shared-content behavior and update key-handling / no-extra-tips tests.
- `crates/otto/src/plugin/builtin/splash/mod.rs` — remove stale cached HUD
  state / hook wiring if it is no longer used after the shared-render change.
- `crates/otto/src/plugin/effects.rs` — inject `app.splash_sandbox` into the
  `/splash` screen-open path if screen construction needs app-owned state.
- `crates/otto/src/egui_app/view.rs` — keep fullscreen egui overlays in sync
  with the shared splash content, centering, and no-wrap logo rows.
- `crates/otto/src/egui_app/mod.rs` — keep egui global shortcut precedence
  aligned with splash any-key dismissal semantics.
- `crates/otto/src/main.rs` — keep ratatui global quit chords from bypassing
  the topmost splash screen's own key handling.
- `crates/otto/src/plugin/screen_stack.rs` — cache screen ids alongside active
  screens so splash special-casing does not add per-frame `Screen::id()`
  allocations.

## Task 1: Route `/splash` through the shared splash renderer

**Files:**
- Modify: `crates/otto/src/ui.rs`
- Modify: `crates/otto/src/splash.rs`
- Modify: `crates/otto/src/plugin/effects.rs`
- Modify: `crates/otto/src/egui_app/view.rs`
- Modify: `crates/otto/src/plugin/screen_stack.rs`

- [ ] Add targeted tests in `crates/otto/src/ui.rs` first covering the
      fullscreen splash-screen render path: a fake `"splash"` screen should
      paint startup-splash content (logo/tagline/hint) via the shared renderer,
      while a non-splash fullscreen screen should still paint its own
      `Screen::render` output.
- [ ] Add or adjust targeted tests in `crates/otto/src/splash.rs` first if
      needed so the shared content helper can be asserted directly for the
      inline hint and sandbox line without spinning up the full UI.
- [ ] Run `cargo test -p otto --bin otto ui::tests::paint_screen` and expect
      the new splash-path test to fail before implementation because fullscreen
      plugin screens do not yet reuse `crate::splash::render`.
- [ ] Update `crates/otto/src/ui.rs` so `paint_screen` routes screen id
      `"splash"` through `crate::splash::render(...)` with the current
      `app.splash_sandbox`, without changing fullscreen rendering for any other
      screen id.
- [ ] In `crates/otto/src/splash.rs`, extract the smallest shared content
      helper needed so the startup renderer and `SplashScreen::render` both
      source the same logo/tagline/sandbox/hint lines without splitting into
      divergent render logic.
- [ ] In `crates/otto/src/plugin/effects.rs`, make sure opening `/splash`
      constructs the screen with the current `app.splash_sandbox` rather than a
      fallback/default value.
- [ ] In `crates/otto/src/egui_app/view.rs`, make fullscreen splash overlays
      consume the shared splash content with centered, non-wrapping logo rows
      so egui matches ratatui instead of reflowing the art.
- [ ] In `crates/otto/src/plugin/screen_stack.rs`, cache the active screen id
      when pushing screens so splash-specific render/input branches can consult
      the top id without adding new per-frame `String` allocations.
- [ ] Run `cargo test -p otto --bin otto ui::tests::paint_screen` again and
      expect the new targeted tests to pass.
- [ ] Public-interface check: record that this task does **not** change the SPP
      wire format, tool schemas, plugin ABI, slash-command surface, env vars,
      or on-disk formats; it only changes internal TUI rendering.
- [ ] Host-swap/RwLock check: not applicable — this task does not touch
      `crates/otto/src/app.rs` or `crates/otto/src/tui.rs`, and it introduces
      no `.await`.
- [ ] ProgressDispatcher check: not applicable — no streaming provider path is
      touched.
- [ ] Format and commit: `cargo fmt --all` then
      `git commit -m "otto: share splash renderer across startup and slash screen"`.

## Task 2: Align the splash plugin screen behavior with the shared hint

**Files:**
- Modify: `crates/otto/src/plugin/builtin/splash/screen.rs`
- Modify: `crates/otto/src/plugin/builtin/splash/mod.rs`
- Modify: `crates/otto/src/splash.rs`
- Modify: `crates/otto/src/main.rs`
- Modify: `crates/otto/src/egui_app/mod.rs`

- [ ] Add/adjust tests in `crates/otto/src/plugin/builtin/splash/screen.rs`
      first so `/splash` closes on a representative non-Esc key and no longer
      asserts on the removed text-only dismiss hint, while also asserting
      `tips()` is empty.
- [ ] Run `cargo test -p otto --bin otto plugin::builtin::splash` and expect
      the new dismissal-behavior test to fail before implementation because the
      screen currently only closes on Esc/Enter and still models the old custom
      content path.
- [ ] Simplify `crates/otto/src/plugin/builtin/splash/screen.rs` so it only
      provides the splash screen id, shared-content rendering sourced from
      `crates/otto/src/splash.rs`, empty `tips()`, and any-key dismissal
      semantics that match the startup hint.
- [ ] Remove stale cached connect-status HUD state and unused hook handling from
      `crates/otto/src/plugin/builtin/splash/mod.rs` if no longer needed by the
      shared-render path; keep the existing slash-command surface unchanged.
- [ ] In `crates/otto/src/main.rs` and `crates/otto/src/egui_app/mod.rs`,
      make sure frontend-level global shortcuts do not preempt the topmost
      splash screen, so `/splash` really closes on any key across both
      frontends.
- [ ] Run `cargo test -p otto --bin otto plugin::builtin::splash` again and
      expect the splash plugin tests to pass.
- [ ] Public-interface check: confirm again that the slash command remains
      `/splash` and no external schema, ABI, env var, or on-disk format changes.
- [ ] Host-swap/RwLock check: not applicable — no `app.rs`/`tui.rs` lock path
      is touched.
- [ ] ProgressDispatcher check: not applicable — no streaming provider path is
      touched.
- [ ] Format and commit: `cargo fmt --all` then
      `git commit -m "otto: align slash splash behavior with startup overlay"`.

## Task 3: Validate the full workspace and note the release follow-on

**Files:**
- Modify: none in this task unless test fallout requires small scoped fixes in
  the already-listed files

- [ ] Run `cargo build --workspace --all-targets` and expect a green build.
- [ ] Run `cargo test --workspace --no-fail-fast` and expect the full test suite
      to pass.
- [ ] Run `cargo clippy --workspace --all-targets` and expect zero
      warnings/errors.
- [ ] Run `cargo fmt --all --check` and expect no formatting drift.
- [ ] Run `cargo run -p otto` if this environment can launch the TUI, manually
      invoke `/splash`, and confirm it matches the startup splash visually; if
      the environment is effectively headless, record that limitation and rely
      on the targeted/full automated checks plus the PR-branch CI run.
- [ ] Public-interface check: confirm the final diff remains internal-only and
      requires no README, SPEC, or ABI update beyond the committed design/plan
      docs.
- [ ] Host-swap/RwLock check: not applicable — no `app.rs`/`tui.rs` changes and
      no awaited lock guards were introduced.
- [ ] ProgressDispatcher check: not applicable — no provider streaming changes.
- [ ] Note in the feature PR body that the follow-on dedicated release PR will,
      after merge, bump `workspace.package.version` and internal
      `workspace.dependencies` versions to `0.25.3`, update `CHANGELOG.md`, and
      tag/push `v0.25.3` per `RELEASING.md`.
- [ ] Format and commit: if Task 1 and Task 2 already committed all code/doc
      changes, no additional code commit is required; otherwise run
      `cargo fmt --all` and commit the remaining scoped diff with
      `git commit -m "otto: finish splash parity validation"`.

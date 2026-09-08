# issue-80-slash-command-input Implementation Plan

**Goal:** Make the inline `/` command palette keep working while also showing the slash and the currently highlighted command in the prompt input, per `savvagent/otto#80`.

**Architecture:** Keep the plugin-driven palette flow intact. `PaletteScreen` remains the owner of filter/cursor state and continues to drive slash execution, while `Effect::PrefillInput` mirrors the current selection into `App::input_textarea`. The palette-specific `open_screen` branch seeds the initial preview so the prompt and inline list stay synchronized from the first frame.

**Tech Stack:** Existing `crates/otto` plugin/runtime modules only (`command_palette/screen.rs`, `plugin/effects.rs`, existing effect + textarea plumbing). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-08-issue-80-slash-command-input-design.md` — read it first. This plan implements it exactly.

**Release line:** v0.26.4

**Branch:** `otto/issue-80-slash-command-input`

## File Map

**New files**
- None.

**Modified files**
- `crates/otto/src/plugin/builtin/command_palette/screen.rs` — derive prompt preview text from the current filtered selection and emit prompt-prefill/clear effects as palette state changes.
- `crates/otto/src/plugin/effects.rs` — seed the initial prompt preview when the palette screen opens and cover the app-level behavior with regression tests.
- `docs/superpowers/specs/2026-09-08-issue-80-slash-command-input-design.md` — already committed design source of truth for this change.
- `docs/superpowers/plans/2026-09-08-issue-80-slash-command-input.md` — this implementation plan.

## Task 1: Mirror palette selection into the prompt during interaction

**Files:**
- Modify: `crates/otto/src/plugin/builtin/command_palette/screen.rs`

- [ ] Read the current palette behavior in `crates/otto/src/plugin/builtin/command_palette/screen.rs`, especially `filtered`, `render`, and `on_key`, and confirm the prompt preview must come from plugin screen state rather than the legacy `App::palette_filter` helpers.
- [ ] Add failing tests in `crates/otto/src/plugin/builtin/command_palette/screen.rs` for the prompt-preview helper / emitted effects: initial selected-command preview, the empty-command-list `/` fallback, typed-filter fallback when there is no match, `Up`/`Down` preview updates, and `Esc` / empty-result `Enter` clearing the preview. Run `cargo test -p otto command_palette::screen -- --nocapture` and expect the new assertions to fail before implementation.
- [ ] Implement a small `PaletteScreen` helper that derives the prompt preview text from the current filtered selection, and update `on_key` so `Char`, `Backspace`, `Up`, and `Down` return `Effect::PrefillInput` with the latest preview while preserving existing palette navigation behavior.
- [ ] Keep immediate execution semantics intact: `Enter` on a no-argument command must still run the slash immediately, but the effect stack must clear the preview deterministically; `Enter` on an argument-taking command must still prefill `/<command> `; `Esc` and the empty-result `Enter` path must close the screen and clear the preview.
- [ ] Record the public-interface check in code review notes: no SPP wire format, tool schema, plugin ABI surface, slash-command name, env var, or on-disk format changes — this is internal runtime behavior only.
- [ ] Run `cargo test -p otto command_palette::screen -- --nocapture` again and expect the targeted palette tests to pass.
- [ ] Run `cargo fmt --all` and commit the task with `git commit -m "otto: mirror slash palette selection into prompt"`.

## Task 2: Seed the preview on open and verify app-level behavior

**Files:**
- Modify: `crates/otto/src/plugin/effects.rs`
- Modify: `crates/otto/src/plugin/builtin/command_palette/screen.rs`

- [ ] Read the palette-specific `open_screen` branch in `crates/otto/src/plugin/effects.rs` and confirm it constructs a concrete `PaletteScreen` before boxing it, which is the right place to seed the initial preview.
- [ ] Add a failing regression test in `crates/otto/src/plugin/effects.rs` that opens the palette through `apply_effects(... Effect::OpenScreen { id: "palette", ... })` and asserts the app prompt is immediately prefilled with the first highlighted slash command; include a second assertion that immediate-run command execution does not leave stale preview text behind after the effect ordering settles. Run `cargo test -p otto plugin::effects -- --nocapture` and expect the new assertions to fail before implementation.
- [ ] Update the palette-specific `open_screen` branch to derive the initial preview from the concrete `PaletteScreen`, call `app.prefill_input(...)`, and then push the screen so the prompt shows the highlighted command from the first frame.
- [ ] Verify the effect ordering for open/close/execute paths stays deterministic in tests so stale prompt text cannot survive `Esc`, empty-result `Enter`, or immediate-run `Enter`.
- [ ] Record the public-interface check in code review notes: no SPP wire format, tool schema, plugin ABI surface, slash-command names, env vars, or on-disk formats changed.
- [ ] This task does not touch `crates/otto/src/app.rs` or `crates/otto/src/tui.rs`, so the host-swap `RwLock` rule is unaffected; note that explicitly in review.
- [ ] This task does not touch any streaming provider path, so the `ProgressDispatcher` forwarder-abort invariant is unaffected; note that explicitly in review.
- [ ] Run `cargo test -p otto plugin::effects -- --nocapture` and `cargo test -p otto command_palette::screen -- --nocapture` and expect both targeted suites to pass.
- [ ] Run `cargo clippy --workspace --all-targets` and expect it to stay clean for the modified palette/effects paths.
- [ ] Run `cargo fmt --all` and commit the task with `git commit -m "otto: seed slash prompt preview on palette open"`.
- [ ] Release note for Phase 4: after this feature PR merges, cut a dedicated release PR per `RELEASING.md` to ship the fix as v0.26.4; that dedicated release PR, not this feature branch, will bump `workspace.package.version`, every internal `workspace.dependencies` version in `Cargo.toml`, and `CHANGELOG.md`.

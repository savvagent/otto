# rename-quit-to-exit Implementation Plan

**Goal:** Rename the built-in session-termination slash command from `/quit` to `/exit` everywhere
users invoke, discover, and read about it, while preserving the existing `Effect::Quit` shutdown
path and intentionally removing `/quit` rather than keeping a compatibility alias.

**Architecture:** The rename stays entirely within `crates/otto`'s built-in plugin and UI
surfaces. `internal:quit` currently contributes the slash command from
`crates/otto/src/plugin/builtin/quit/mod.rs`; the home command list in `app.rs`, builtin
registration expectations in `plugin/mod.rs`, and command-palette fixtures/tests in
`plugin/builtin/command_palette/screen.rs` all mirror that command name. `plugin/effects.rs` owns
only the stable internal `Effect::Quit -> App::request_quit()` mapping and remains behaviorally
unchanged apart from comment/test wording. Locale catalogs and `README.md` must match the new
public slash-command spelling. No host/tool/provider transport, provider pool, or transcript/keyring
surface is touched.

**Tech Stack:** Rust 2024, built-in plugin manifests (`otto-plugin`), ratatui/egui-facing app
state in `crates/otto`, rust-i18n locale catalogs under `crates/otto/locales/`, GitHub
CLI for issue/PR/release workflow.

**Spec:** `docs/superpowers/specs/2026-09-05-rename-quit-to-exit-design.md` — read it first. This
plan implements it exactly.

**Release line:** `v0.22.0`

**Branch:** `plugin/rename-quit-to-exit`

## Known Plan Gaps

- A third critique dispatch reported that the committed spec/plan files were not present, but both files do exist in the feature worktree and have already been committed on `plugin/rename-quit-to-exit`. Treat that critique result as an environment-visibility false positive, not a planning gap.

## File Map

**New files**
- `docs/superpowers/specs/2026-09-05-rename-quit-to-exit-design.md` — committed design spec for the
  breaking slash-command rename.
- `docs/superpowers/plans/2026-09-05-rename-quit-to-exit.md` — committed implementation plan.

**Modified files**
- `crates/otto/src/plugin/builtin/quit/mod.rs` — rename the slash command, manifest metadata,
  and tests from quit to exit while keeping `Effect::Quit` behavior.
- `crates/otto/src/plugin/builtin/mod.rs` — update the builtin-plugin doc comment for the core
  exit command.
- `crates/otto/src/plugin/builtin/command_palette/screen.rs` — rename command-palette fixture
  and regression coverage to `/exit`.
- `crates/otto/src/plugin/effects.rs` — update regression-test prose/comments that document the
  slash command feeding `Effect::Quit`.
- `crates/otto/src/app.rs` — rename the home command list entry and any nearby slash-surface
  comments.
- `crates/otto/src/plugin/mod.rs` — update builtin registration expectations from
  `internal:quit` to `internal:exit`.
- `crates/otto/src/plugin/manifests.rs` — keep startup manifest conflicts strict in general
  while skipping only discovered user-command collisions against the new built-in `/exit`.
- `crates/otto/src/main.rs` — cover the startup non-crash path for conflicting `commands/exit.md`
  and document the intentional test-only HOME-lock lint suppression.
- `crates/otto/locales/en.toml`
- `crates/otto/locales/es.toml`
- `crates/otto/locales/pt.toml`
- `crates/otto/locales/hi.toml` — update rendered summary/description strings so user-facing
  localized text advertises `/exit`.
- `README.md` — rename the documented slash command row from `/quit` to `/exit`.

## Task 1: Rename the slash command surface to `/exit`

**Files:**
- Modify: `crates/otto/src/plugin/builtin/quit/mod.rs`
- Modify: `crates/otto/src/plugin/builtin/mod.rs`
- Modify: `crates/otto/src/plugin/builtin/command_palette/screen.rs`
- Modify: `crates/otto/src/plugin/effects.rs`
- Modify: `crates/otto/src/app.rs`
- Modify: `crates/otto/src/plugin/mod.rs`
- Modify: `crates/otto/src/plugin/manifests.rs`
- Modify: `crates/otto/src/main.rs`
- Modify: `crates/otto/locales/en.toml`
- Modify: `crates/otto/locales/es.toml`
- Modify: `crates/otto/locales/pt.toml`
- Modify: `crates/otto/locales/hi.toml`
- Modify: `README.md`

- [x] Write/adjust failing tests first in `crates/otto/src/plugin/builtin/quit/mod.rs`,
      `crates/otto/src/plugin/builtin/command_palette/screen.rs`, and any builtin-registration
      expectation in `crates/otto/src/plugin/mod.rs` so they expect `exit` / `internal:exit`
      instead of `quit` / `internal:quit`.
- [x] Run targeted tests before implementation:
      `cargo test -p otto -- exit_returns_quit_effect`;
      `cargo test -p otto -- exit_is_listed_and_runs_via_runslash`;
      `cargo test -p otto -- register_builtins_pr8_complete`.
      Expect failure because the tests now expect `/exit` / `internal:exit` before the production
      code is renamed.
- [x] Implement the rename in the listed files: expose `/exit` as the command name, rename the core
      builtin plugin id to `internal:exit`, update the home command list and palette fixture, revise
      locale-rendered strings, and update README documentation. Keep `Effect::Quit`,
      `App::request_quit`, Ctrl-C, and Ctrl-D behavior unchanged. If the rename exposes a startup
      conflict with discovered user commands named `exit`, make that path graceful by keeping the
      builtin owner and skipping only the conflicting discovered command entry; do not weaken the
      hard-error behavior for the plugin's own built-in `/reload-commands` surface or for other
      plugin conflicts.
- [x] Grep for lingering public-surface references:
      `rg -n '/quit|internal:quit|name: "quit"|quit-summary|quit-description' crates/otto/src crates/otto/locales README.md`
      and confirm only intentional historical/non-user-facing references remain, if any.
- [x] Run targeted validation after implementation:
      `cargo test -p otto -- exit_returns_quit_effect`;
      `cargo test -p otto -- manifest_marks_exit_as_core`;
      `cargo test -p otto -- exit_is_listed_and_runs_via_runslash`;
      `cargo test -p otto -- register_builtins_pr8_complete`.
      Expect all to pass with `/exit` semantics.
- [x] Public-interface check: record that this task makes a **breaking slash-command rename** on the
      README-documented public interface (`/quit` removed, `/exit` added). Call it out in the PR
      body, the architecture-review prompt, and the release PR's `CHANGELOG.md` entry and MINOR bump.
- [x] Host-swap `RwLock` check: this task touches `crates/otto/src/app.rs`; verify it changes
      only static command metadata/comments and introduces no `.await` while any `Arc<RwLock<...>>`
      guard is held.
- [x] ProgressDispatcher check: not applicable — no streaming provider path is touched.
- [x] Run full required validation:
      `cargo build --workspace --all-targets`;
      `cargo test --workspace`;
      `cargo clippy --workspace --all-targets`;
      `cargo fmt --all --check`.
      Expect all four commands to pass with `RUSTFLAGS=-D warnings` cleanliness preserved.
- [x] Format and commit: `cargo fmt --all` then
      `git commit -m "otto: rename /quit slash command to /exit"`.

## Task 2: Record the release follow-through required after merge

**Files:**
- No feature-branch code changes; this task records the mandatory release follow-through.

- [x] Note in the feature PR body and release handoff that a dedicated release PR must be opened
      immediately after merge to bump `workspace.package.version` and all internal
      `workspace.dependencies` versions to `0.22.0`, add the breaking `/quit` -> `/exit` entry to
      `CHANGELOG.md`, run the required validation, and ship the `v0.22.0` tag per `RELEASING.md`.

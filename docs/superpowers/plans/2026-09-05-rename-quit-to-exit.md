# rename-quit-to-exit Implementation Plan

**Goal:** Rename the built-in session-termination slash command from `/quit` to `/exit` everywhere
users invoke, discover, and read about it, while preserving the existing `Effect::Quit` shutdown
path and intentionally removing `/quit` rather than keeping a compatibility alias.

**Architecture:** The rename stays entirely within `crates/savvagent`'s built-in plugin and UI
surfaces. `internal:quit` currently contributes the slash command from
`crates/savvagent/src/plugin/builtin/quit/mod.rs`; the home command list in `app.rs`, builtin
registration expectations in `plugin/mod.rs`, and command-palette fixtures/tests in
`plugin/builtin/command_palette/screen.rs` all mirror that command name. `plugin/effects.rs` owns
only the stable internal `Effect::Quit -> App::request_quit()` mapping and remains behaviorally
unchanged apart from comment/test wording. Locale catalogs and `README.md` must match the new
public slash-command spelling. No host/tool/provider transport, provider pool, or transcript/keyring
surface is touched.

**Tech Stack:** Rust 2024, built-in plugin manifests (`savvagent-plugin`), ratatui/egui-facing app
state in `crates/savvagent`, rust-i18n locale catalogs under `crates/savvagent/locales/`, GitHub
CLI for issue/PR/release workflow.

**Spec:** `docs/superpowers/specs/2026-09-05-rename-quit-to-exit-design.md` — read it first. This
plan implements it exactly.

**Release line:** `v0.22.0`

**Branch:** `plugin/rename-quit-to-exit`

## File Map

**New files**
- `docs/superpowers/specs/2026-09-05-rename-quit-to-exit-design.md` — committed design spec for the
  breaking slash-command rename.
- `docs/superpowers/plans/2026-09-05-rename-quit-to-exit.md` — committed implementation plan.

**Modified files**
- `crates/savvagent/src/plugin/builtin/quit/mod.rs` — rename the slash command, manifest metadata,
  and tests from quit to exit while keeping `Effect::Quit` behavior.
- `crates/savvagent/src/plugin/builtin/mod.rs` — update the builtin-plugin doc comment for the core
  exit command.
- `crates/savvagent/src/plugin/builtin/command_palette/screen.rs` — rename command-palette fixture
  and regression coverage to `/exit`.
- `crates/savvagent/src/plugin/effects.rs` — update regression-test prose/comments that document the
  slash command feeding `Effect::Quit`.
- `crates/savvagent/src/app.rs` — rename the home command list entry and any nearby slash-surface
  comments.
- `crates/savvagent/src/plugin/mod.rs` — update builtin registration expectations from
  `internal:quit` to `internal:exit`.
- `crates/savvagent/locales/en.toml`
- `crates/savvagent/locales/es.toml`
- `crates/savvagent/locales/pt.toml`
- `crates/savvagent/locales/hi.toml` — update rendered summary/description strings so user-facing
  localized text advertises `/exit`.
- `README.md` — rename the documented slash command row from `/quit` to `/exit`.

## Task 1: Rename the slash command surface to `/exit`

**Files:**
- Modify: `crates/savvagent/src/plugin/builtin/quit/mod.rs`
- Modify: `crates/savvagent/src/plugin/builtin/mod.rs`
- Modify: `crates/savvagent/src/plugin/builtin/command_palette/screen.rs`
- Modify: `crates/savvagent/src/plugin/effects.rs`
- Modify: `crates/savvagent/src/app.rs`
- Modify: `crates/savvagent/src/plugin/mod.rs`
- Modify: `crates/savvagent/locales/en.toml`
- Modify: `crates/savvagent/locales/es.toml`
- Modify: `crates/savvagent/locales/pt.toml`
- Modify: `crates/savvagent/locales/hi.toml`
- Modify: `README.md`

- [ ] Write/adjust failing tests first in `crates/savvagent/src/plugin/builtin/quit/mod.rs`,
      `crates/savvagent/src/plugin/builtin/command_palette/screen.rs`, and any builtin-registration
      expectation in `crates/savvagent/src/plugin/mod.rs` so they expect `exit` / `internal:exit`
      instead of `quit` / `internal:quit`.
- [ ] Run targeted tests before implementation:
      `cargo test -p savvagent quit_returns_quit_effect quit_is_listed_and_runs_via_runslash register_builtins_pr8_complete -- --nocapture`
      (or, if the runner rejects multiple names, run the same test functions via the smallest number
      of `cargo test -p savvagent <name>` invocations needed). Expect failure on `/quit`-named
      assertions before code changes land.
- [ ] Implement the rename in the listed files: expose `/exit` as the command name, rename the core
      builtin plugin id to `internal:exit`, update the home command list and palette fixture, revise
      locale-rendered strings, and update README documentation. Keep `Effect::Quit`,
      `App::request_quit`, Ctrl-C, and Ctrl-D behavior unchanged.
- [ ] Grep for lingering public-surface references:
      `rg -n '/quit|internal:quit|name: "quit"|quit-summary|quit-description' crates/savvagent/src crates/savvagent/locales README.md`
      and confirm only intentional historical/non-user-facing references remain, if any.
- [ ] Run targeted validation after implementation:
      `cargo test -p savvagent quit_returns_quit_effect`;
      `cargo test -p savvagent manifest_marks_quit_as_core`;
      `cargo test -p savvagent quit_is_listed_and_runs_via_runslash`;
      `cargo test -p savvagent register_builtins_pr8_complete`.
      Expect all to pass with `/exit` semantics.
- [ ] Public-interface check: record that this task makes a **breaking slash-command rename** on the
      README-documented public interface (`/quit` removed, `/exit` added). Call it out in the PR
      body, the architecture-review prompt, and the release PR's `CHANGELOG.md` entry and MINOR bump.
- [ ] Host-swap `RwLock` check: this task touches `crates/savvagent/src/app.rs`; verify it changes
      only static command metadata/comments and introduces no `.await` while any `Arc<RwLock<...>>`
      guard is held.
- [ ] ProgressDispatcher check: not applicable — no streaming provider path is touched.
- [ ] Run full required validation:
      `cargo build --workspace --all-targets`;
      `cargo test --workspace`;
      `cargo clippy --workspace --all-targets`;
      `cargo fmt --all --check`.
      Expect all four commands to pass with `RUSTFLAGS=-D warnings` cleanliness preserved.
- [ ] Format and commit: `cargo fmt --all` then
      `git commit -m "savvagent: rename /quit slash command to /exit"`.

## Task 2: Record the release follow-through required after merge

**Files:**
- No feature-branch code changes; this task records the mandatory release follow-through.

- [ ] Confirm the feature PR itself does **not** bump versions or edit `CHANGELOG.md`; per
      `RELEASING.md` and the repo workflow, those land in a dedicated release PR after merge.
- [ ] When opening the release PR, bump the shared workspace version from `0.21.0` to `0.22.0`,
      update every internal `workspace.dependencies` version to match, and add a `CHANGELOG.md`
      entry that explicitly calls out the breaking `/quit` -> `/exit` rename under the release's
      dated section.
- [ ] Validate the release PR with `cargo fmt --all -- --check`,
      `cargo clippy --workspace --all-targets`, and `cargo test --workspace` before requesting its
      mandatory review trio.
- [ ] Format and commit: `cargo fmt --all` (if needed) then
      `git commit -m "release: cut v0.22.0"` in the dedicated release worktree after this feature PR
      merges.

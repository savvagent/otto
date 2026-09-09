# issue-94-remove-egui Implementation Plan

**Goal:** Delete the experimental `eframe`/`egui` GUI front-end from `crates/otto` so the ratatui TUI is the single front-end, removing the `otto gui` entry point, the `egui_app` module, the four GUI-only dependencies, and the GUI-shaped prompt-prefill accommodation in shared `App` state — with TUI behavior unchanged, per `savvagent/otto#94`.

**Architecture:** This is subtractive, not a refactor. Coupling is one-directional: nothing outside `crates/otto/src/egui_app/` imports from it (the only inbound references are `mod egui_app;` at `crates/otto/src/main.rs:42`, the `otto gui` argv branch at `:183-190`, and two doc comments), while `egui_app` imports TUI-owned items (`app::App`, `app::Entry`, `palette::Palette`, `ui::ToolEntryRender`, `plugin::builtin::themes::catalog::Theme`, `plugin::builtin::themes::xterm_colors::xterm_256_rgb`) that all stay. Because `egui_app` compiles as a unit and `crates/otto` is linted with `-D warnings`, the module, its entry point, its dependencies, and the `App::pending_prefill` bridge it is the sole consumer of must all come out in one commit — any partial deletion leaves `dead_code` errors and has no green intermediate state worth landing. Prose and changelog updates follow as a separate, independently-green commit.

**Tech Stack:** Existing `crates/otto` only. No new dependencies; four removed (`eframe`, `egui`, `egui-file-dialog`, `futures`). `cargo check --workspace` regenerates `Cargo.lock`.

**Spec:** `docs/superpowers/specs/2026-09-08-issue-94-remove-egui-design.md` — read it first. This plan implements it exactly.

**Release line:** v0.27.0 — MINOR, because removing the README-documented `otto gui` entry point is a breaking change under this repo's pre-1.0 convention (`CHANGELOG.md`).

**Branch:** `otto/issue-94-remove-egui`

## File Map

**Deleted files**
- `crates/otto/src/egui_app/mod.rs` — the `eframe::App` shell, `GuiApp` bootstrap, turn execution.
- `crates/otto/src/egui_app/view.rs` — home view, prompt, slot painting.
- `crates/otto/src/egui_app/convert.rs` — theme/styled-line/key-event → egui conversions.
- `crates/otto/src/egui_app/screen.rs` — plugin `Screen` overlay geometry + painting.
- `crates/otto/src/egui_app/render_model.rs` — off-thread `RenderModel` snapshot cache (no non-egui caller).
- `crates/otto/src/egui_app/fonts.rs` — monospace font setup.
- `crates/otto/src/egui_app/widgets/mod.rs`, `widgets/canvas.rs`, `widgets/file_picker.rs` — GUI widgets.

**Modified files**
- `crates/otto/src/main.rs` — drop `mod egui_app;` and the `otto gui` argv branch; reword the `bootstrap_app_and_host` / `HostBoot` doc comments that describe a second front-end.
- `crates/otto/src/app.rs` — remove the `pending_prefill` field, its initializer, and `take_pending_prefill`.
- `crates/otto/src/plugin/effects.rs` — drop the two `take_pending_prefill` assertions from `prefill_input_replaces_textarea_contents`; reword the egui-referencing comments.
- `crates/otto/src/canvas_input.rs` — module doc only: it is shell-agnostic and the TUI dispatches through it; logic unchanged.
- `crates/otto/src/plugin/builtin/themes/xterm_colors.rs` — doc comment only.
- `crates/otto/Cargo.toml` — remove `eframe`, `egui`, `egui-file-dialog`, `futures`.
- `Cargo.toml` (workspace root) — remove the same four from `[workspace.dependencies]` plus their GUI-specific pin comments.
- `Cargo.lock` — regenerated, not hand-edited.
- `README.md` — remove the `cargo run -p otto -- gui` line and the "Experimental GUI (v0.19.0, in progress)" callout.
- `CHANGELOG.md` — `### Removed` entry under `## [Unreleased]`.
- `docs/superpowers/specs/2026-05-26-v0.19.0-egui-frontend-design.md` — status flipped to abandoned, citing #94.

**Already committed**
- `docs/superpowers/specs/2026-09-08-issue-94-remove-egui-design.md` — the design source of truth.
- `docs/superpowers/plans/2026-09-08-issue-94-remove-egui.md` — this plan.

## Task 1: Delete the egui front-end, its entry point, its deps, and the prompt-prefill bridge

**Files:**
- Delete: the nine files under `crates/otto/src/egui_app/`
- Modify: `crates/otto/src/main.rs`, `crates/otto/src/app.rs`, `crates/otto/src/plugin/effects.rs`, `crates/otto/Cargo.toml`, `Cargo.toml`, `Cargo.lock`

**Note on test ordering:** this task is a deletion with no new behavior, so there is no meaningful failing test to write first. The verification contract is instead: (a) capture a green baseline before touching anything, (b) after deletion the same suite is still green with the egui-only tests gone and no other test lost, (c) `clippy -D warnings` is the completeness check for residual dead code. Steps below are ordered so a red state is impossible to mistake for a green one.

- [ ] Capture the baseline in the worktree: run `cargo test --workspace 2>&1 | tail -40` and record the per-crate test counts, especially `crates/otto`'s. The expected `crates/otto` reduction is **60 tests**: 57 `#[test]`/`#[tokio::test]` attributes under `crates/otto/src/egui_app/` plus the 3 in `crates/otto/src/plugin/builtin/themes/xterm_colors.rs` (deleted below). Confirm both counts yourself before deleting (`grep -rc '#\[test\]\|#\[tokio::test\]' crates/otto/src/egui_app/` and the same on `xterm_colors.rs`) so the post-deletion delta is checked against a number, not eyeballed.
- [ ] Delete the whole directory: `git rm -r crates/otto/src/egui_app`.
- [ ] In `crates/otto/src/main.rs`, remove the `mod egui_app;` declaration (`:42`) and the entire `otto gui` argv branch (`:183-190`), including its explanatory comment. Do not add a replacement branch, shim, or "the GUI was removed" message — per the spec's Assumptions, `otto gui` simply falls through to the TUI. Confirm the fallthrough is safe by reading the one other `std::env::args()` consumer in the crate — `crates/otto/src/plugin/builtin/self_update/mod.rs:133`, which passes argv to `opt_out_from` and only tests for the literal `--no-update-check`. A stray `gui` argument is therefore ignored and the TUI launches normally: no panic, no hang, no unrecognized-argument error. Do not expect `env::args()` to be absent from the crate — it is not.
- [ ] In `crates/otto/src/app.rs`, remove the `pending_prefill: Option<String>` field and its doc comment (`:527-534`), its `pending_prefill: None` initializer (`:979`), and the `take_pending_prefill` method with its doc comment (`:1987-1993`). Also remove the two bridge lines *inside* `prefill_input` at `:1973-1974` — the `// Bridge for out-of-`App` prompt buffers (egui's `OttoApp::prompt`).` comment and `self.pending_prefill = Some(text.clone());` — after which `text.clone()` in the following line collapses to `text`. Everything else about `prefill_input` (its signature, its `input_textarea` assignment, its cursor move) stays exactly as-is: the TUI and the #80 palette prompt-preview both depend on it.
- [ ] In `crates/otto/src/plugin/effects.rs`, remove **both** `take_pending_prefill` test call sites — there are two, and they are not equivalent:
      - `:1342-1352`, in `prefill_input_replaces_textarea_contents` — delete the two bridge assertions, keep the test's `input_textarea` assertions, so the test still proves `Effect::PrefillInput` reaches the prompt.
      - `:3211-3215`, in the refused-palette-open test — delete `assert_eq!(app.take_pending_prefill(), None, "a refused palette open must not stage a PrefillInput")`. This guards the #80 palette surface, so it is deleted on a stated rationale, not because it stops compiling: the same test already asserts `app.input_textarea.lines() == &draft[..]` at `:3205-3209` ("palette open over a non-empty prompt must not alter the draft"), and once the bridge is gone `input_textarea` is the only thing a prefill can touch, so that surviving assertion covers the whole property. Do **not** delete the draft assertion above it, and do not weaken the test to compensate.
- [ ] Remove `eframe`, `egui`, `egui-file-dialog`, and `futures` from the `[dependencies]` section of `crates/otto/Cargo.toml` (`:89-92`), including the "Native GUI front-end (v0.19.0 egui migration)" comment block above them.
- [ ] Remove **only** `eframe`, `egui`, and `egui-file-dialog` from `[workspace.dependencies]` in the root `Cargo.toml`, including the eframe default-features warning comment and the `egui-file-dialog` version-pin comment.
- [ ] **Do NOT remove `futures = "0.3"` from the root `[workspace.dependencies]`** (root `Cargo.toml:83`), even though issue #94's acceptance criteria say to remove it from both manifests. Seven other workspace members declare `futures.workspace = true` and use it in real code — `otto-host`, `provider-anthropic`, `provider-deepseek`, `provider-gemini`, `provider-local`, `provider-openai`, `tool-web` — so deleting the root entry makes all seven fail to resolve and kills `cargo check --workspace` in the next step. This is a deliberate deviation from the issue AC, recorded in the spec's Premise corrections; do not "fix" it back. Verify with `grep -rn '^futures' crates/*/Cargo.toml` before and after.
- [ ] Leave `image`, `blitz-*`, `anyrender*`, and `peniko` untouched — they belong to `otto-canvas` and to `frame_to_dynamic_image` in `crates/otto/src/app.rs`, not to the GUI. Leave `[workspace.metadata.dist.dependencies.apt]` untouched.
- [ ] Regenerate the lockfile with `cargo check --workspace` (never hand-edit `Cargo.lock`). Expect a large deletion-only diff; confirm with `git diff --stat Cargo.lock` and `grep -c '^name = "eframe"' Cargo.lock` returning 0.
- [ ] Delete `crates/otto/src/plugin/builtin/themes/xterm_colors.rs` (`git rm`) and the `pub mod xterm_colors;` declaration at `crates/otto/src/plugin/builtin/themes/mod.rs:12`. Its only non-test consumer was `egui_app/convert.rs:14`; the TUI never calls `xterm_256_rgb` because ratatui resolves indexed colors natively. Confirm with `grep -rn xterm_256_rgb crates/otto/src` returning nothing after the deletion. Its 3 unit tests go with it — that is expected and is already accounted for in the 60-test baseline delta above, NOT an instance of the "any other missing test is a mistake" rule. Do not preserve the module behind `#[allow(dead_code)]`.
- [ ] Run `cargo clippy --workspace --all-targets -- -D warnings` and expect it clean. Treat any `dead_code` / `unused_imports` finding as this task's remaining work: delete what genuinely has no TUI caller, and do not delete anything the TUI still calls. Re-check whether `#[allow(dead_code)]` on `HostBoot` (`crates/otto/src/main.rs:253`) is still earned — remove the attribute if every field is now read, keep it if not, and state which in the task report. Note that the sibling types just below it (`McpManagerSeed`, `McpServerAuthSummary`, …) all carry the same attribute, so stripping it from `HostBoot` alone is locally inconsistent; keeping it is the defensible default unless clippy says otherwise.
- [ ] Run `cargo build` (bare — required because `crates/otto` owns the `otto-tool-fs` bin the TUI spawns at runtime) and `cargo build --workspace --all-targets`; both must succeed.
- [ ] Run `cargo test --workspace` and compare against the baseline: the only test-count reduction may be the egui-only tests counted in step 1 plus the two removed bridge assertions (which do not change the test count, only the assertion count). Any other missing test is a mistake to fix before committing.
- [ ] Verify the *code and manifest* deletion is total: `rg 'egui|eframe' crates/ -g '**/Cargo.toml'` and `rg 'egui|eframe' Cargo.toml Cargo.lock` must both return no hits, and `rg -w futures crates/otto/src` must return nothing. Note the `**/` in that glob — a `*/Cargo.toml` glob silently matches nothing and would make the manifest check a no-op.
- [ ] Expect `rg 'egui|eframe' crates/ -g '*.rs'` to STILL report hits at this commit — the GUI-referencing doc comments in `canvas_input.rs`, `plugin/effects.rs`, and `main.rs` are reworded in Task 2, not here. The zero-hit assertion belongs to Task 2's completeness check, not this one. Do **not** run a bare `rg egui crates/` at any point: `crates/otto/locales/pt.toml:80,81,240,242` contain the substring inside ordinary Portuguese words (`Prosseguindo`, `seguinte`) that have nothing to do with the GUI and must not be edited.
- [ ] Record the public-interface check: this REMOVES the README-documented `otto gui` entry point. No SPP wire type, `ProviderHandler`/`ProviderClient` method, tool MCP schema, plugin ABI surface, slash command, README-documented env var, or on-disk transcript/keyring format changes — so Non-Negotiable Rule 6 is not triggered by its own letter and must not be cited as if a listed surface changed. Per the spec's "Public-interface changes" section the change is nonetheless treated as **breaking** (a CLI entry point a shell history or script may invoke), which is why it is named here, flagged to the architecture review, gets an explicit `CHANGELOG.md` entry (Task 2), and takes a MINOR bump (v0.27.0).
- [ ] This task touches `crates/otto/src/app.rs` and `crates/otto/src/main.rs`: verify explicitly that no `.await` was introduced or left executing while a host-swap `Arc<RwLock<Option<Arc<Host>>>>` read guard is held. The change removes a plain `Option<String>` field and an argv branch and adds no async code, so the expected finding is "unaffected" — state that explicitly rather than skipping the check.
- [ ] This task touches no streaming provider path, so the `rmcp` `ProgressDispatcher` forwarder-abort invariant is unaffected; state that explicitly in the task report.
- [ ] Run `cargo fmt --all`, then `cargo fmt --all --check` to confirm clean, and commit with `git commit -m "otto: remove the experimental egui GUI front-end"`.

## Task 2: Update the prose, the changelog, and the superseded v0.19.0 spec

**Files:**
- Modify: `README.md`, `CHANGELOG.md`, `crates/otto/src/canvas_input.rs`, `crates/otto/src/plugin/effects.rs`, `crates/otto/src/plugin/builtin/themes/xterm_colors.rs`, `crates/otto/src/main.rs`, `docs/superpowers/specs/2026-05-26-v0.19.0-egui-frontend-design.md`

- [ ] In `README.md`, delete the `# Or launch the experimental native GUI (egui) instead of the TUI.` comment and its `cargo run -p otto -- gui` line from the quickstart block, and delete the whole `> **Experimental GUI (v0.19.0, in progress).** …` blockquote that follows it (~L96-119 — confirm the end of the blockquote by content, not line number). Re-read the surrounding quickstart afterwards to confirm the remaining prose still reads as a single-front-end document with no dangling "instead of the TUI" phrasing.
- [ ] Reword the doc comments that describe the GUI as a second caller, without changing any logic:
      - `crates/otto/src/canvas_input.rs:1-9` — module doc currently says "Both the ratatui TUI (`main.rs`) and the egui GUI (`egui_app/widgets/canvas.rs`)". Restate it as the TUI's shell-agnostic key/mouse dispatch, keeping the explanation of *why* the helpers live outside `main.rs`.
      - `crates/otto/src/plugin/effects.rs:668` — drop the egui half of the comparison; keep the TUI rule being described. This is the ONLY egui comment left in this file at Task 2: the other one (`:1345`, the `"…the pending_prefill bridge that the egui prompt drains"` assertion message) was deleted with the bridge assertions in Task 1. Re-locate `:668` by content rather than line number, since Task 1 shifted every line after it.
      - `crates/otto/src/plugin/builtin/themes/xterm_colors.rs` — nothing to do here: the whole file was deleted in Task 1 because `egui_app/convert.rs` was its only consumer. If it still exists at this point, Task 1 was done wrong; stop and fix Task 1 rather than rewording a doc comment on dead code.
      - `crates/otto/src/main.rs:241-252` — there are **three** stacked doc comments here where there should be two. Reword `:241-244` (`bootstrap_app_and_host`) to drop "and the egui front-end (`egui_app::run`)". Delete `:245-248` outright — it is an orphan second `HostBoot` summary whose only reason to exist is explaining the GUI's off-thread bootstrap ("see the GUI's `GuiApp` bootstrap in `egui_app`"); do not reword it. Leave `:249-252`, the real `HostBoot` doc block, untouched — it is already GUI-free.
- [ ] Add a `### Removed` entry under `## [Unreleased]` in `CHANGELOG.md` naming `otto gui` explicitly, stating that the experimental `eframe`/`egui` front-end and its dependencies are gone, that the ratatui TUI is unchanged and is now the only front-end, and that the removed code remains recoverable from git history. Reference `(#94)` in this repo's established style.
- [ ] Flip `docs/superpowers/specs/2026-05-26-v0.19.0-egui-frontend-design.md`'s status line to record the migration as abandoned, with a pointer to #94 and to this change's spec. Do not delete or move it, and do not touch the five egui plan documents — they are the historical record of shipped work and this repo has no archive directory.
- [ ] Confirm no locale catalog changes are needed: the three `t!()` keys `egui_app` used (`notes.disconnect-completed`, `notes.disconnect-worker-failed`, `notes.not-connected-connect-first`) are each still referenced from `crates/otto/src/main.rs`, so no key is orphaned. Verify with `grep -rn "disconnect-completed\|disconnect-worker-failed\|not-connected-connect-first" crates/otto/src` and state the result.
- [ ] Record the public-interface check: docs-and-comments only in this task; the breaking removal itself was recorded in Task 1, and this task is where it becomes discoverable in `CHANGELOG.md`.
- [ ] Now assert the full completeness check that Task 1 deferred: `rg 'egui|eframe' crates/ -g '*.rs'` must return no hits, and `rg -i 'egui|otto gui' README.md` must return no hits. `docs/` and the two historical `CHANGELOG.md` mentions inside already-shipped release sections (`:171`, `:208-209`) are expected to still match and must be left alone.
- [ ] Run `cargo test --workspace` (doc comments are compiled) and `cargo clippy --workspace --all-targets -- -D warnings`; both must stay green.
- [ ] Run `cargo fmt --all`, then commit with `git commit -m "docs: drop the egui front-end from the docs and changelog"`.

## Task 3: Release note

- [ ] No code changes in this task. After this feature PR merges, a dedicated release PR per `RELEASING.md` cuts **v0.27.0** (MINOR — breaking removal of the `otto gui` entry point): that PR, not this branch, bumps `workspace.package.version` and every internal `workspace.dependencies` version in the root `Cargo.toml`, and renames `## [Unreleased]` to `## 0.27.0 - <date>`. Do not duplicate the version bump here.
- [ ] Note for the release PR author: `## [Unreleased]` was empty at branch time even though #89 (`provider-deepseek: classify list_models HTTP errors by status code`) and #92 (`otto: mirror highlighted slash command into the prompt input`) merged after the v0.26.3 tag. Those two merges have no changelog entries. Surface this when cutting v0.27.0 rather than silently shipping them undocumented.

## Task 2b: Sweep stale two-front-end comments

Added after Tasks 1 and 2 were committed and reviewed. Task 2's completeness check was
`rg 'egui|eframe' crates/ -g '*.rs'`, which by construction cannot see a comment that never
spells the framework's name. Several comments describing the deleted front-end said "the GUI",
"the winit event loop", "the paint thread", "the window", or `build_model` (a function that lived
in the removed `egui_app/render_model.rs`) — so they passed a zero-hit check while still narrating
a two-front-end world, and in places pointing at subsystems this branch deleted. `winit` was never
a direct dependency at all; it arrived transitively through `eframe` and left with it. Two
independent reviewers of the branch flagged the same gap.

**Sites fixed (comments and markdown only — no logic, symbol, or test-name changes):**

- `crates/otto/src/main.rs` — the `bootstrap_host_only`, `bootstrap_app_and_host`, and
  `build_app_with_host` doc comments described a host-off-thread / `App`-on-the-UI-thread split
  that only the GUI ever performed. Verified that each of the two halves now has exactly one
  production caller (`bootstrap_app_and_host`, which awaits them back to back on one task) and
  said so honestly, keeping the `Send`/`!Send` explanation of *why* the two functions are shaped
  the way they are rather than inventing a new justification for the split.
- `crates/otto/src/plugin/slots.rs` and `crates/otto/src/plugin/tool_summaries.rs` — the rationale
  for the deliberate non-blocking `try_lock` on the render path was written in terms of the winit
  event loop and `build_model`. The pattern is still correct, so the rationale was re-grounded in
  what is true now: this runs on the TUI's redraw path (`ui::compute_home_frame_data`, awaited just
  before `terminal.draw`), where a blocking acquire stalls the redraw, and the registry-read-lock /
  plugin-`Mutex` ordering against a write-preferring `registry.write()` waiter can deadlock it
  outright.
- `crates/otto/src/plugin/effects.rs` — a test doc comment claiming "Both front-ends already gate
  their palette opener on an empty prompt". There is one front-end.
- `crates/provider-local/src/lib.rs` — the `connect_timeout` rationale named "the GUI bootstrap"
  as the caller it protects; it is otto's startup bootstrap.
- `CHANGELOG.md` — the `### Removed` entry said `otto gui` no longer launches a window without
  saying what it does instead. Now records that the argument is ignored and the TUI launches
  (verified: `main()` has no argv branch, and the crate's only other `std::env::args()` consumer,
  `crates/otto/src/plugin/builtin/self_update/mod.rs:133`, tests solely for `--no-update-check`).
- `docs/superpowers/specs/2026-05-26-v0.19.0-egui-frontend-design.md` — its
  `Supersedes: nothing (the ratatui TUI is removed, not deprecated in place)` line sat directly
  under the new "abandoned" status and read as a contradiction; it is now marked as the abandoned
  plan's intent. The document body is untouched.

**The check that should have been used**, and that this task asserts:

```
rg -in 'egui|eframe|winit|build_model' crates/ -g '*.rs'   # must be empty
rg -in 'GUI|front-end|frontend' crates/ -g '*.rs'          # review every hit by hand
```

The second grep cannot be a zero-hit assertion — legitimate hits remain — so it is a
read-every-result check, not an automated one.

**Deliberately left for separate follow-ups:** renaming the test
`shared_content_marks_logo_rows_centered_for_frontend_parity` in `crates/otto/src/splash.rs` (a
code change, not a comment change), and relocating the misattached `bootstrap_app_and_host` doc
comment that sits above `pub(crate) struct HostBoot` in `crates/otto/src/main.rs` (pre-existing).

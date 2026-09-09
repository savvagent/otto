# Remove the experimental egui GUI front-end — design

Date: 2026-09-08
Status: pending review
Related: `savvagent/otto#94`
Roadmap: reverses the unshipped Plan 5 of the v0.19.0 egui migration (`docs/superpowers/plans/2026-05-29-v0.19.0-egui-teardown.md`)

## Problem

`crates/otto` currently ships two front-ends. The default is the ratatui TUI; `otto gui` launches an experimental `eframe`/`egui` window (`crates/otto/src/main.rs:183-190`, `crates/otto/src/egui_app/`).

The GUI was built as Plans 1–4 of a five-plan migration whose stated end state was "make egui the only front-end" and delete ratatui entirely (`docs/superpowers/plans/2026-05-29-v0.19.0-egui-teardown.md`). Plan 5 was never executed. The result is a half-finished migration held in a stable state: the TUI remains the default and the fully-supported surface, while the GUI is documented in `README.md` as experimental and explicitly missing plugin-bound home accelerators, high-fidelity markdown rendering, and inline canvas rendering.

The cost of holding that state is concrete:

- `crates/otto/src/egui_app/` is 3,176 lines across 9 files that every TUI change must be kept consistent with.
- `eframe` pulls 253 transitive crates into `crates/otto` — the workspace's `default-members` — so a bare `cargo build` compiles the entire GUI stack (glow, winit, x11 + wayland windowing) for developers who only touch the TUI.
- The GUI is not in `PRD.md`. `PRD.md` contains no mention of a windowed front-end at all; the product of record is a terminal coding agent.
- Shared code carries GUI-shaped accommodations that exist for no other caller, notably the out-of-`App` prompt-prefill bridge (`crates/otto/src/app.rs:527-534`, `:1987-1993`).

Issue #94 resolves the stalled migration in the opposite direction from Plan 5: delete the egui front-end and keep the ratatui TUI as the single front-end.

## Approach

The coupling is one-directional, which makes this a subtractive change rather than a refactor. Nothing outside `crates/otto/src/egui_app/` imports from it — the only inbound references are `main.rs`'s `mod egui_app;` declaration (`crates/otto/src/main.rs:42`), the `otto gui` dispatch (`:183-190`), and two doc comments. `egui_app` imports *from* the crate (`app::App`, `app::Entry`, `palette::Palette`, `ui::ToolEntryRender`, `plugin::builtin::themes::catalog::Theme`, `plugin::builtin::themes::xterm_colors::xterm_256_rgb`), all of which are TUI-owned and stay.

### 1. Delete the front-end module and its entry point

Remove the whole `crates/otto/src/egui_app/` directory (`mod.rs`, `view.rs`, `convert.rs`, `screen.rs`, `render_model.rs`, `fonts.rs`, `widgets/{mod,canvas,file_picker}.rs`), the `mod egui_app;` declaration, and the `otto gui` argv branch in `main()`. After the branch is gone, `main()` runs the TUI path unconditionally — the same code it already runs for every non-`gui` invocation, so there is no behavior change for any existing TUI user.

`otto gui` becomes an unrecognized first argument. The TUI's existing argv handling determines what that means; the change must not turn a stray `gui` argument into a panic or a silent no-op that looks like a hang.

### 2. Drop the GUI-only dependencies

Remove `eframe`, `egui`, and `egui-file-dialog` from **both** `crates/otto/Cargo.toml` and `[workspace.dependencies]` in the root `Cargo.toml`, along with the comment blocks that explain their GUI-specific pins (the eframe default-features warning and the `egui-file-dialog` version pin note).

`futures` is different and must be handled separately: remove it from `crates/otto/Cargo.toml` **only**, and leave `futures = "0.3"` in the root `[workspace.dependencies]` exactly as it is. `crates/otto`'s use of it was egui-only (the foreign-executor `block_on` in the synchronous paint pass, `egui_app/mod.rs:88`), but seven other workspace members declare `futures.workspace = true` and use it in real code: `otto-host`, `provider-anthropic`, `provider-deepseek`, `provider-gemini`, `provider-local`, `provider-openai`, and `tool-web`. Deleting the root entry makes all seven fail to resolve.

`image` stays: it is used by `frame_to_dynamic_image` in `crates/otto/src/app.rs:152-176` and is a transitive dep of `ratatui-image` and `blitz-paint`. (`ui.rs` renders through `ratatui_image`, not `image` directly.)

### 3. Remove the GUI-only accommodation in shared state

`App::prefill_input` (`crates/otto/src/app.rs:1972-1985`) does two things: it replaces `input_textarea` (what the TUI reads) and it stages the same text on `pending_prefill`, a one-shot bridge that exists solely because the egui shell keeps its prompt buffer outside `App` (`OttoApp::prompt`). With egui gone, the bridge has no consumer.

Remove the `pending_prefill` field, its initializer, and `App::take_pending_prefill`. `prefill_input` keeps its signature and its `input_textarea` behavior, but its body loses the two bridge lines at `crates/otto/src/app.rs:1973-1974` (the `// Bridge for out-of-App prompt buffers` comment and `self.pending_prefill = Some(text.clone());`), after which `text.clone()` collapses to `text`. `prefill_input` itself stays — the TUI depends on it, and the palette's prompt-preview behavior from #80 must not regress.

`take_pending_prefill` has **two** test call sites, both of which must come out, and they are not equivalent:

- `crates/otto/src/plugin/effects.rs:1342-1352`, in `prefill_input_replaces_textarea_contents` — two assertions that the bridge is staged and that draining it is one-shot. Pure bridge coverage; delete both and keep the test's `input_textarea` assertions, which are what actually prove `Effect::PrefillInput` reaches the prompt.
- `crates/otto/src/plugin/effects.rs:3211-3215`, in the refused-palette-open test — `assert_eq!(app.take_pending_prefill(), None, "a refused palette open must not stage a PrefillInput")`. This one guards the #80 surface this spec names as the highest regression risk, so deleting it needs a reason rather than a compile error. The reason: the same test already asserts `app.input_textarea.lines() == &draft[..]` at `:3205-3209` with the message "palette open over a non-empty prompt must not alter the draft". Once the bridge is gone, `input_textarea` is the only thing a prefill could touch, so that surviving assertion covers the full property. Delete the bridge assertion; do **not** delete the draft assertion above it, and do not weaken the test to compensate.

### 4. Update prose that describes a two-front-end world

- `README.md`: drop the `cargo run -p otto -- gui` line and the "Experimental GUI (v0.19.0, in progress)" block (~L96-118).
- Doc comments naming the GUI as a second caller: `crates/otto/src/canvas_input.rs:1-9` (module doc), `crates/otto/src/plugin/effects.rs:668`, `:1345`, `crates/otto/src/plugin/builtin/themes/xterm_colors.rs:8-10`, and `crates/otto/src/main.rs:243-248` (`bootstrap_app_and_host`'s "shared by the TUI and the egui front-end" framing).
- `crates/otto/src/main.rs:241-252` currently holds **three** stacked doc comments where there should be two. `:241-244` documents `bootstrap_app_and_host` ("Shared by the ratatui TUI (`run_app`) and the egui front-end"); `:245-248` is an orphan second `HostBoot` summary that exists only to explain the GUI's off-thread bootstrap ("see the GUI's `GuiApp` bootstrap in `egui_app`"); `:249-252` is the real `HostBoot` doc block and is GUI-free already. Reword `:241-244` to drop the second front-end, delete `:245-248` outright rather than rewording it, and leave `:249-252` alone. `HostBoot` and `bootstrap_host_only` are still used by the TUI path (`main.rs:414-435`) and stay; the `#[allow(dead_code)]` on `HostBoot` (`:253`) should be re-evaluated once the GUI's field reads are gone.
- `docs/superpowers/specs/2026-05-26-v0.19.0-egui-frontend-design.md`: flip its status to record that the migration was abandoned, citing #94. The five plans and two specs stay in place — they are the historical record of shipped work, and this repo has no archive directory.

### 5. Delete `xterm_colors`, which loses its only consumer

`crates/otto/src/plugin/builtin/themes/xterm_colors.rs` exposes `pub(crate) fn xterm_256_rgb`, and its only non-test caller anywhere in the crate is `egui_app/convert.rs:14` (plus 17 call sites inside that file). The TUI never calls it — ratatui resolves indexed colors natively, which is why the helper was written for the GUI sink in the first place.

Once `egui_app` is gone the function is dead, and `crates/otto` is a binary crate, so `dead_code` applies to `pub` items too and `clippy -D warnings` fails. Delete `xterm_colors.rs` outright along with the `pub mod xterm_colors;` declaration at `crates/otto/src/plugin/builtin/themes/mod.rs:12`. Its three unit tests go with it; that is correct, not a lost-coverage regression, because the code under test no longer exists. Do not keep the module alive behind an `#[allow(dead_code)]` — that preserves an unreachable helper and hides the fact that nothing needs it.

### 6. Let the compiler find the rest

`crates/otto` is compiled with `-D warnings` in CI. Anything that becomes dead once `egui_app` is gone — a `pub(crate)` helper in `main.rs` with no remaining caller, an unused import — surfaces as a `dead_code` or `unused_imports` error rather than needing to be enumerated up front. `cargo clippy --workspace --all-targets` is the authority on completeness here, not a hand-written list.

## Scope

**In:**
- Delete `crates/otto/src/egui_app/` (9 files).
- Remove `mod egui_app;` and the `otto gui` argv branch from `crates/otto/src/main.rs`.
- Remove `eframe`, `egui`, `egui-file-dialog` from `crates/otto/Cargo.toml` **and** the root `Cargo.toml`; remove `futures` from `crates/otto/Cargo.toml` **only**; regenerate `Cargo.lock`.
- Delete `crates/otto/src/plugin/builtin/themes/xterm_colors.rs` and its `pub mod` declaration — `egui_app` was its only consumer.
- Remove `App::pending_prefill` + `App::take_pending_prefill` and the bridge assertions in `crates/otto/src/plugin/effects.rs`.
- Update `README.md` and the GUI-referencing doc comments listed above.
- Flip the v0.19.0 egui design spec's status to abandoned.
- A `CHANGELOG.md` `### Removed` entry under `## [Unreleased]`.

**Out:**
- Any change to TUI behavior. The ratatui front-end is byte-for-byte the same front-end after this change.
- Deleting the historical egui plans/specs under `docs/superpowers/`.
- Touching `crates/otto/src/canvas_input.rs`'s logic. It is shell-agnostic, the TUI dispatches through it (`main.rs`), and only its module doc mentions the GUI.
- Locale catalogs. The three `t!()` keys `egui_app` uses (`notes.disconnect-completed`, `notes.disconnect-worker-failed`, `notes.not-connected-connect-first`) are all also used by `main.rs`, so no key becomes orphaned and no catalog changes.
- The `image`, `blitz-*`, `anyrender*`, and `peniko` deps — those belong to `otto-canvas`/`ui.rs`, not the GUI.
- Removing the ratatui TUI or reviving any part of Plan 5.
- Any host, provider, tool, plugin-ABI, or wire-format change.

## Public-interface changes

**Breaking, and deliberate.** `otto gui` is a documented developer-facing entry point in `README.md`, and removing it removes a way to launch the program.

Strictly, it is *not* one of Non-Negotiable Rule 6's enumerated surfaces: no SPP wire type, `ProviderHandler`/`ProviderClient` method, tool MCP schema, plugin ABI surface, slash command, README-documented env var, or on-disk transcript/keyring format is affected, and no plugin can observe the difference. Rule 6 is therefore not triggered by its own letter, and the plan should not cite it as if a listed surface changed. This spec nonetheless applies Rule 6's *treatment* — named here, flagged to the architecture review, called out in `CHANGELOG.md`, reflected in the version bump — because a CLI entry point someone's shell history or script may invoke is user-visible in the way the rule exists to protect. That is the conservative call and it costs nothing pre-1.0.

Per this repo's pre-1.0 SemVer convention (`CHANGELOG.md`), a breaking change is a **MINOR** bump. The release cut after this PR merges is therefore `v0.27.0`, not a patch on `v0.26.4` (the workspace version while this branch was open). `CHANGELOG.md` gets a `### Removed` entry naming `otto gui` explicitly so the removal is discoverable by anyone whose muscle memory or script invokes it.

The GUI was documented as experimental and in-progress from the day it shipped, so there is no deprecation window: the README callout is the deprecation notice, and it goes away with the feature.

## Premise corrections

- **Issue #94's framing that the migration "was never executed" is accurate but incomplete.** Plans 1–4 *were* executed and shipped (PRs #104–#107 per the Plan 5 roadmap block); only Plan 5, the teardown that would have deleted ratatui, was not. What is being removed is shipped, working, partial code — not an abandoned branch.
- **Issue #94's claim that `futures` should be removed "from both the workspace root and `crates/otto` manifests" is wrong and would break the build.** The narrow claim behind it is true — zero `futures` references in `crates/otto/src` outside `egui_app` — but seven other workspace members consume the root `[workspace.dependencies]` entry via `futures.workspace = true`. Only the `crates/otto/Cargo.toml` entry may go. The issue's acceptance criterion should be corrected to match.
- **Issue #94 does not mention `xterm_colors`.** `egui_app/convert.rs` is the sole consumer of `xterm_256_rgb`, so the module dies with the GUI and must be deleted for `clippy -D warnings` to pass. This is in scope even though the issue does not name it.
- **Issue #94 suggests the prompt-prefill bridge "may now be dead" and should be checked.** It is dead in full: `take_pending_prefill` has exactly one caller family (the egui paint pass) plus its own test assertion. `prefill_input` is *not* dead and must stay.
- **Issue #94 does not name a version bump.** This spec fixes it at MINOR (`v0.27.0`) on the breaking-change reading above, rather than leaving it to the release step.

## Assumptions

- **Keep the historical plans/specs, don't delete them.** This repo's record-as-shipped convention treats `docs/superpowers/` as the project's history; deleting five shipped plans would erase the record of work that really happened. Marking the design spec abandoned preserves both the history and the current truth.
- **`otto gui` gets no compatibility shim or friendly "the GUI was removed" message.** Adding one would mean keeping an argv branch alive for a feature the README described as experimental, and this repo is pre-1.0. The `CHANGELOG.md` entry is the notice.
- **MINOR bump rather than PATCH**, because removing a documented entry point is breaking under this repo's convention even though the entry point was labeled experimental. Choosing PATCH would understate it; choosing MINOR costs nothing pre-1.0.
- **Deletion happens in one task, not incrementally per file.** `egui_app` compiles as a unit and nothing imports from it, so a partial deletion has no green intermediate state worth landing.
- **Clippy with `-D warnings` is treated as the completeness check** for residual dead code, rather than enumerating every `pub(crate)` item in `main.rs` up front. The compiler cannot miss what a grep can.

## Goal & Success Criteria

`crates/otto` builds and ships one front-end — the ratatui TUI — with no egui code, no egui dependencies, and no GUI-shaped accommodations left in shared state, and with TUI behavior unchanged.

- [ ] `crates/otto/src/egui_app/` does not exist, and `rg 'egui|eframe' crates/ -g '*.rs' -g '**/Cargo.toml'` returns no hits. Note the `**/` — a `*/Cargo.toml` glob silently matches nothing, which would make the manifest half of this check a no-op. (Do **not** grep bare `egui` across all of `crates/` — `crates/otto/locales/pt.toml` contains the substring inside ordinary Portuguese words at `:80`, `:81`, `:240`, `:242` (`Prosseguindo`, `seguinte`), which have nothing to do with the GUI and must not be touched.)
- [ ] `eframe`, `egui`, `egui-file-dialog`, and `futures` appear in neither `Cargo.toml`, and `Cargo.lock` no longer resolves `eframe`/`egui`/`egui-file-dialog`.
- [ ] `cargo build`, `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo fmt --all --check` are all green.
- [ ] `cargo run -p otto` launches the TUI, renders the home view, and `/` opens the command palette with the prompt preview from #80 still working — verified by hand, since CI cannot.
- [ ] `README.md` contains no GUI launch instruction or experimental-GUI callout, and `CHANGELOG.md` has a `### Removed` entry naming `otto gui`.
- [ ] `docs/superpowers/specs/2026-05-26-v0.19.0-egui-frontend-design.md` records the migration as abandoned with a pointer to #94.

## Error Handling & Edge Cases

- **`otto gui` after removal.** The argument falls through to whatever the TUI already does with an unrecognized first argument. Determine that behavior during implementation and state it in the plan; if it would panic or produce a confusing hang, that is a defect to fix, not to inherit. A plain TUI launch that ignores the argument is acceptable.
- **Residual dead code in `main.rs`.** Helpers that existed only for the GUI bootstrap (`bootstrap_host_only`'s split, `HostBoot`'s `#[allow(dead_code)]`) may go partly or fully unused. Do not delete anything the TUI still calls; let clippy decide, and re-check whether `#[allow(dead_code)]` on `HostBoot` is still earned.
- **`Cargo.lock` churn.** Dropping `eframe` removes a large transitive subtree. The lockfile diff will be large and that is expected; it must be regenerated by `cargo check --workspace`, not hand-edited.
- **Linux build deps.** `libfontconfig1-dev` is still required after this change — it belongs to Blitz via `otto-canvas`, not to egui. It is installed by `.github/workflows/ci.yml`, not by `[workspace.metadata.dist.dependencies.apt]` (which lists only `libdbus-1-dev` and `pkg-config`). Leave both untouched.
- **Windows CI carve-out.** `otto-canvas`/`otto` remain excluded from the Windows test matrix for the documented Blitz font-discovery hang. That exclusion is unrelated to egui and must not be "fixed" as part of this change.

## Risks & Open Questions

- **A shared helper turns out to be GUI-only in practice.** `render_model.rs` mirrors `ui::HomeFrameData` for the egui paint pass; `ui::compute_home_frame_data` and `ToolEntryRender` are TUI-owned and stay, but if any TUI-side code path was introduced purely to feed the GUI snapshot, clippy will flag it. Resolve by deletion only when there is genuinely no TUI caller.
- **The palette prompt-preview behavior from #80 is the highest-regression-risk surface**, because #80's design deliberately routed through the same `PrefillInput` effect that feeds the egui bridge being deleted. The TUI path writes `input_textarea` directly and is independent of the bridge, but this deserves an explicit test run and a manual check rather than trust.
- **Reversibility.** If the GUI is ever wanted again, it is recoverable from git history at the commit before this change; the plans and specs that describe it stay in the tree. Worth stating in the CHANGELOG entry.
- **Revival guard: do not restore the `pending_prefill` shape.** A `git revert` of this change would bring back not just the GUI but the anti-pattern it forced — a one-shot `Option<String>` on `App` that a foreign paint pass drains each frame, making `App` carry a field no owner of `App` reads. If a second front-end is ever wanted, the seam is the `Effect` vocabulary in `otto-plugin`: the new shell consumes effects like any other consumer rather than reaching into `App` for state staged on its behalf. State that only a non-`App` prompt buffer reads does not belong on `App`, whatever brings it back.
- **Open question deferred to the plan:** whether removing the `otto gui` branch changes `init_tracing`'s behavior or any startup ordering that the GUI path set up differently. Verify during implementation; the TUI path is unchanged code, so the expected answer is no.

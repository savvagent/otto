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

Remove `eframe`, `egui`, `egui-file-dialog`, and `futures` from `crates/otto/Cargo.toml` and from `[workspace.dependencies]` in the root `Cargo.toml`, along with the comment blocks that explain their GUI-specific pins (the eframe default-features warning and the `egui-file-dialog` version pin note). `futures` goes with them because `grep` over `crates/otto/src` finds zero uses outside `egui_app` — it was added for the foreign-executor `block_on` in the synchronous egui paint pass.

`image` stays: it is used by `ui.rs` for canvas rendering and is a transitive dep of `ratatui-image` and `blitz-paint`.

### 3. Remove the GUI-only accommodation in shared state

`App::prefill_input` (`crates/otto/src/app.rs:1972-1985`) does two things: it replaces `input_textarea` (what the TUI reads) and it stages the same text on `pending_prefill`, a one-shot bridge that exists solely because the egui shell keeps its prompt buffer outside `App` (`OttoApp::prompt`). With egui gone, the bridge has no consumer.

Remove the `pending_prefill` field, its initializer, and `App::take_pending_prefill`, and drop the two bridge assertions from `prefill_input_replaces_textarea_contents` (`crates/otto/src/plugin/effects.rs:1342-1352`). `prefill_input` itself stays — the TUI depends on it, and the palette's prompt-preview behavior from #80 must not regress.

### 4. Update prose that describes a two-front-end world

- `README.md`: drop the `cargo run -p otto -- gui` line and the "Experimental GUI (v0.19.0, in progress)" block (~L96-118).
- Doc comments naming the GUI as a second caller: `crates/otto/src/canvas_input.rs:1-9` (module doc), `crates/otto/src/plugin/effects.rs:668`, `:1345`, `crates/otto/src/plugin/builtin/themes/xterm_colors.rs:8-10`, and `crates/otto/src/main.rs:243-248` (`bootstrap_app_and_host`'s "shared by the TUI and the egui front-end" framing).
- `crates/otto/src/main.rs`'s `HostBoot` doc block explains the struct's `Send`-only shape by reference to the GUI's off-thread bootstrap. `HostBoot` and `bootstrap_host_only` are still used by the TUI path (`main.rs:414-435`) and stay; only the rationale prose changes. The `#[allow(dead_code)]` on `HostBoot` should be re-evaluated once the GUI's field reads are gone.
- `docs/superpowers/specs/2026-05-26-v0.19.0-egui-frontend-design.md`: flip its status to record that the migration was abandoned, citing #94. The five plans and two specs stay in place — they are the historical record of shipped work, and this repo has no archive directory.

### 5. Let the compiler find the rest

`crates/otto` is compiled with `-D warnings` in CI. Anything that becomes dead once `egui_app` is gone — a `pub(crate)` helper in `main.rs` with no remaining caller, an unused import — surfaces as a `dead_code` or `unused_imports` error rather than needing to be enumerated up front. `cargo clippy --workspace --all-targets` is the authority on completeness here, not a hand-written list.

## Scope

**In:**
- Delete `crates/otto/src/egui_app/` (9 files).
- Remove `mod egui_app;` and the `otto gui` argv branch from `crates/otto/src/main.rs`.
- Remove `eframe`, `egui`, `egui-file-dialog`, `futures` from `crates/otto/Cargo.toml` and the root `Cargo.toml`; regenerate `Cargo.lock`.
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

**Breaking, and deliberate** (Non-Negotiable Rule 6). `otto gui` is a documented developer-facing entry point in `README.md`, and removing it removes a way to launch the program. It is not an SPP wire-format, tool-schema, plugin-ABI, or on-disk-format change — no transcript, keyring entry, `StreamEvent`, `ToolDef`, or slash command is affected, and no plugin can observe the difference.

Per this repo's pre-1.0 SemVer convention (`CHANGELOG.md`), a breaking change is a **MINOR** bump. The release cut after this PR merges is therefore `v0.27.0`, not a patch on `v0.26.3`. `CHANGELOG.md` gets a `### Removed` entry naming `otto gui` explicitly so the removal is discoverable by anyone whose muscle memory or script invokes it.

The GUI was documented as experimental and in-progress from the day it shipped, so there is no deprecation window: the README callout is the deprecation notice, and it goes away with the feature.

## Premise corrections

- **Issue #94's framing that the migration "was never executed" is accurate but incomplete.** Plans 1–4 *were* executed and shipped (PRs #104–#107 per the Plan 5 roadmap block); only Plan 5, the teardown that would have deleted ratatui, was not. What is being removed is shipped, working, partial code — not an abandoned branch.
- **Issue #94 lists `futures` as removable "only egui_app uses it."** Verified: zero `futures` references in `crates/otto/src` outside `egui_app`.
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

- [ ] `crates/otto/src/egui_app/` does not exist, and `rg egui crates/` returns no hits outside `docs/`.
- [ ] `eframe`, `egui`, `egui-file-dialog`, and `futures` appear in neither `Cargo.toml`, and `Cargo.lock` no longer resolves `eframe`/`egui`/`egui-file-dialog`.
- [ ] `cargo build`, `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo fmt --all --check` are all green.
- [ ] `cargo run -p otto` launches the TUI, renders the home view, and `/` opens the command palette with the prompt preview from #80 still working — verified by hand, since CI cannot.
- [ ] `README.md` contains no GUI launch instruction or experimental-GUI callout, and `CHANGELOG.md` has a `### Removed` entry naming `otto gui`.
- [ ] `docs/superpowers/specs/2026-05-26-v0.19.0-egui-frontend-design.md` records the migration as abandoned with a pointer to #94.

## Error Handling & Edge Cases

- **`otto gui` after removal.** The argument falls through to whatever the TUI already does with an unrecognized first argument. Determine that behavior during implementation and state it in the plan; if it would panic or produce a confusing hang, that is a defect to fix, not to inherit. A plain TUI launch that ignores the argument is acceptable.
- **Residual dead code in `main.rs`.** Helpers that existed only for the GUI bootstrap (`bootstrap_host_only`'s split, `HostBoot`'s `#[allow(dead_code)]`) may go partly or fully unused. Do not delete anything the TUI still calls; let clippy decide, and re-check whether `#[allow(dead_code)]` on `HostBoot` is still earned.
- **`Cargo.lock` churn.** Dropping `eframe` removes a large transitive subtree. The lockfile diff will be large and that is expected; it must be regenerated by `cargo check --workspace`, not hand-edited.
- **Linux build deps.** `libfontconfig1-dev` is still required after this change — it belongs to Blitz via `otto-canvas`, not to egui. Do not remove it from the dist apt dependencies.
- **Windows CI carve-out.** `otto-canvas`/`otto` remain excluded from the Windows test matrix for the documented Blitz font-discovery hang. That exclusion is unrelated to egui and must not be "fixed" as part of this change.

## Risks & Open Questions

- **A shared helper turns out to be GUI-only in practice.** `render_model.rs` mirrors `ui::HomeFrameData` for the egui paint pass; `ui::compute_home_frame_data` and `ToolEntryRender` are TUI-owned and stay, but if any TUI-side code path was introduced purely to feed the GUI snapshot, clippy will flag it. Resolve by deletion only when there is genuinely no TUI caller.
- **The palette prompt-preview behavior from #80 is the highest-regression-risk surface**, because #80's design deliberately routed through the same `PrefillInput` effect that feeds the egui bridge being deleted. The TUI path writes `input_textarea` directly and is independent of the bridge, but this deserves an explicit test run and a manual check rather than trust.
- **Reversibility.** If the GUI is ever wanted again, it is recoverable from git history at the commit before this change; the plans and specs that describe it stay in the tree. Worth stating in the CHANGELOG entry.
- **Open question deferred to the plan:** whether removing the `otto gui` branch changes `init_tracing`'s behavior or any startup ordering that the GUI path set up differently. Verify during implementation; the TUI path is unchanged code, so the expected answer is no.

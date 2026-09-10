# issue-96-palette-prompt-echoes-filter Implementation Plan

**Goal:** While the `/` command palette is open, make the prompt input echo exactly what the user typed (`/` + filter) instead of the resolved name of the highlighted row, so the resolved command value reaches the prompt only on selection, per `savvagent/otto#96`.

**Architecture:** Keep the plugin-driven palette flow and its screen-owns-input model intact. `PaletteScreen` remains the owner of filter/cursor state and continues to drive slash execution; only the *content* it reports back through `Effect::PrefillInput` narrows, from "the highlighted command" to "the typed filter". Navigation keys stop emitting a prefill at all, matching `connect/screen.rs`. Two gaps open up as a direct consequence and are closed in the same change: backspacing past the leading `/` must close the palette (otherwise the prompt contains a character the user cannot delete), and the sheet needs a no-match empty state (otherwise dropping the now-duplicated `> <filter>` header leaves a blank panel). Dropping that header also shifts the render windowing budget from three reserved chrome rows to two.

**Tech Stack:** Existing `crates/otto` modules only (`command_palette/screen.rs`, `plugin/effects.rs`, `app.rs`) plus the four locale catalogs. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-09-issue-96-palette-prompt-echoes-filter-design.md` — read it first, including "This is a reversal, not a narrowing" and "The trade this accepts". This plan implements it exactly.

**Release line:** v0.27.1 (PATCH — a fix to behavior shipped by #92). Sequencing note: release PR #111 (v0.27.0) is open at the time of writing. If #111 merges first, this lands as v0.27.1. If this merges first, its CHANGELOG entry goes under `## [Unreleased]` and ships as part of v0.27.0. Check which is true at Phase 4 step 12 rather than assuming.

**Branch:** `plugin/palette-prompt-echoes-filter`

## File Map

**New files**
- None.

**Modified files**
- `crates/otto/src/plugin/builtin/command_palette/screen.rs` — `prompt_preview` returns the literal filter; `Up`/`Down` emit nothing; `Backspace` on an empty filter closes; `render` drops the `> <filter>` header, reserves two chrome rows, and gains a no-match empty state; tests rewritten.
- `crates/otto/src/app.rs` — `prefill_input` cursor column counts chars, not bytes.
- `crates/otto/src/plugin/effects.rs` — `"palette"` `open_screen` branch comment; rename/rewrite `palette_open_screen_prefills_prompt_with_first_command`.
- `crates/otto/locales/{en,es,pt,hi}.toml` — new `picker.command-palette.no-matches` key.
- `docs/superpowers/specs/2026-09-08-issue-80-slash-command-input-design.md` — superseded-by note on the one overturned assumption; `> **Status:**` stays IMPLEMENTED.
- `docs/superpowers/specs/2026-09-09-issue-96-palette-prompt-echoes-filter-design.md` — the design source of truth.
- `docs/superpowers/plans/2026-09-09-issue-96-palette-prompt-echoes-filter.md` — this plan.
- `CHANGELOG.md` — a `Fixed` entry referencing #96.

## Task 1: Narrow the prompt preview to the typed filter

**Files:**
- Modify: `crates/otto/src/plugin/builtin/command_palette/screen.rs`

- [ ] Read `filtered`, `prompt_preview`, `render`, `on_key` and confirm the preview derives from screen-local state only, so no `App` plumbing changes are needed for this task.
- [ ] Rewrite the `prompt_preview_*` tests to the spec's success criteria so they fail first: opening shows `/`; typing `c` then `o` shows `/co` regardless of highlight; `Up`/`Down` return `vec![]` and leave the preview at `/co`; `Backspace` on a non-empty filter yields `/c`; a non-matching filter still yields `/<filter>`. Run `cargo test -p otto command_palette::screen` and confirm the new assertions fail.
- [ ] Replace `prompt_preview`'s three-way branch with `format!("/{}", self.filter)` and rewrite its doc comment — the stale doc describing "the currently highlighted command" is what would mislead the next reader.
- [ ] Change `Up` and `Down` to mutate `self.cursor` only and return `Ok(vec![])`, matching `connect/screen.rs:214-222`.
- [ ] Leave `Char` emitting `Effect::PrefillInput { text: self.prompt_preview() }`.
- [ ] Verify the selection paths are untouched and their existing ordering tests stay green without edits: `Enter` no-arg → `Stack([CloseScreen, PrefillInput{""}, RunSlash])`; `Enter` `requires_arg` → `Stack([CloseScreen, PrefillInput{"/<cmd> "}])`; `Esc` and empty-result `Enter` → `[CloseScreen, PrefillInput{""}]`.
- [ ] Public-interface check: no SPP wire format, tool schema, plugin ABI, slash-command name, env var, or on-disk format change — internal TUI behavior only.
- [ ] Run `cargo test -p otto command_palette::screen`; expect green.
- [ ] `cargo fmt --all` and commit: `git commit -m "otto: echo the typed filter in the palette prompt"`.

## Task 2: Close the palette on backspace past the leading slash

**Files:**
- Modify: `crates/otto/src/plugin/builtin/command_palette/screen.rs`

- [ ] Add a failing test: with an empty filter, `Backspace` emits `[CloseScreen, PrefillInput{""}]`; with filter `c`, it emits only `PrefillInput{"/"}` and does not close. Run `cargo test -p otto command_palette::screen` and confirm failure.
- [ ] Change the `Backspace` arm so `self.filter.pop()` returning `None` produces `[CloseScreen, PrefillInput { text: String::new() }]`, and a successful pop keeps today's behavior (reset `cursor` to 0, emit the new preview).
- [ ] Cite the precedent in the code comment — `app.rs:1460-1462` documents closing the palette on backspace past the leading `/` — so this reads as the established intent rather than a new invention.
- [ ] Run `cargo test -p otto command_palette::screen`; expect green.
- [ ] `cargo fmt --all` and commit: `git commit -m "otto: close the palette on backspace past the slash"`.

## Task 3: Drop the filter header, add a no-match state, rebalance the window

**Files:**
- Modify: `crates/otto/src/plugin/builtin/command_palette/screen.rs`
- Modify: `crates/otto/locales/en.toml`, `crates/otto/locales/es.toml`, `crates/otto/locales/pt.toml`, `crates/otto/locales/hi.toml`

- [ ] Add failing render tests first:
      (a) no rendered line starts with `> `;
      (b) with `commands` non-empty and a filter matching nothing, the sheet renders the `no-matches` line — assert it is not blank, which is the regression this guards;
      (c) with a list longer than the region, sweep `region.height` over `1..=14` and, at cursor top/middle/bottom for each, assert the `▶` marker is present in the emitted lines and that the emitted line count never exceeds `height - 1` (the row `ui.rs::paint_screen` overpaints with `tips()`);
      (d) the empty-`commands` path still renders `no-commands`.
      Run `cargo test -p otto command_palette::screen` and confirm failure.
- [ ] Add `no-matches` to `crates/otto/locales/en.toml` under `[picker.command-palette]`, then to `es.toml`, `pt.toml`, `hi.toml` — `crates/otto/tests/locales.rs::every_locale_has_the_same_key_set_as_en` fails if any catalog is missed. Match each catalog's existing tone and the parenthesised style of its `no-commands` value.
- [ ] Remove the leading `StyledLine::plain(format!("> {}", self.filter))` from `render`.
- [ ] Add the no-match branch: when `filtered()` is empty and `commands` is not, push the muted `picker.command-palette.no-matches` line, mirroring the existing `no-commands` treatment.
- [ ] Change `capacity` from `.saturating_sub(3).max(1)` to `.saturating_sub(2).max(1)` and rewrite the comment above it to name the two chrome rows that actually remain — the spacer/scroll-hint line and the sheet's last row, overpainted by `tips()`. Keep the explanation of *why* the last row is reserved (the window anchors the cursor on its last row, so a row hidden under `tips()` would be the selected one and `▶` would vanish for every scrolled list). Record that `.max(1)` is not panic-protection — capacity `0` yields a valid empty slice — but a floor that, at `height ≤ 2`, forces a row into a budget with no room; this change shrinks that broken range from `height ≤ 3` to `height ≤ 2`.
- [ ] Note in the comment that the blank spacer row stays unconditional because making it conditional on `hint` would be circular (`hint` derives from `capacity`).
- [ ] Update the three existing render tests deliberately rather than letting them pass or fail by accident:
      `overflowing_list_windows_around_cursor_with_scroll_hint` (`:486-537`) — at `height: 6` capacity goes 3 → 4, so its `/cmd03` and `↓17 more below` / `↑17 more above` assertions all shift by one;
      `list_exactly_filling_capacity_renders_every_row` (`:576-599`) — the boundary moves from 9 rows to 10; update the name if it encodes the number, the `// capacity = height(12) - 3 = 9` comment, and the fixture, because it would otherwise stay green while testing nothing;
      `long_list_fits_shows_no_scroll_hint` (`:459-485`) — stale capacity comment.
- [ ] Confirm the `self.commands.is_empty()` early return still precedes the windowing arithmetic.
- [ ] Run `cargo test -p otto command_palette::screen` and `cargo test -p otto --test locales`; expect green.
- [ ] `cargo fmt --all` and commit: `git commit -m "otto: drop the palette sheet's duplicate filter header"`.

## Task 4: Fix the prefill cursor column and realign the open path

**Files:**
- Modify: `crates/otto/src/app.rs`
- Modify: `crates/otto/src/plugin/effects.rs`

- [ ] Add a failing test in `app.rs`: `prefill_input` with a multi-byte string (e.g. `"/café"`) leaves the cursor column at the char count, not the byte length. Run `cargo test -p otto prefill` and confirm failure.
- [ ] Change the cursor column in `App::prefill_input` from `l.len()` to `l.chars().count()` (`app.rs:1963-1974`), and note in the doc comment that the prompt is now filled from arbitrary typed characters on every palette keystroke, which is what makes the distinction load-bearing.
- [ ] Rename and rewrite `palette_open_screen_prefills_prompt_with_first_command` (`effects.rs:3082-3147`) to expect `/`; the current version computes the alphabetically-first slash and hard-asserts it.
- [ ] Update the `"palette"` `open_screen` branch comment to describe seeding `/`. The `app.prefill_input(screen.prompt_preview())` call itself does not change — verify by reading it, and say so in the commit body: the behavior change is entirely upstream in `prompt_preview`.
- [ ] Re-read the empty-prompt guard above it and confirm its reasoning still holds — the palette owns the draft for as long as it is open. Leave the guard alone; update its comment only if it names the old seeded text.
- [ ] `rg -n 'prompt_preview|highlighted' crates/otto/src docs/ README.md` and fix any stale doc comment or test name still claiming the prompt mirrors the highlighted command.
- [ ] Run `cargo test -p otto`; expect green.
- [ ] `cargo fmt --all` and commit: `git commit -m "otto: seed the palette prompt with a bare slash"`.

## Task 5: Record the superseded assumption, changelog, full verification

**Files:**
- Modify: `docs/superpowers/specs/2026-09-08-issue-80-slash-command-input-design.md`
- Modify: `CHANGELOG.md`

- [ ] Add a note under `## Assumptions` in the #80 spec marking the "show the full slash command path, not just the raw typed filter" assumption as superseded by `savvagent/otto#96`, pointing at this change's spec. Do **not** flip that spec's `> **Status:**` — it stays IMPLEMENTED, because it accurately records what #92 shipped.
- [ ] Add a `### Fixed` entry to `CHANGELOG.md` under the correct heading per the Release line sequencing note. Leave the existing v0.26.4 entry describing #92's behavior alone — the new entry supersedes it rather than rewriting history. Mention the behavior change, the backspace-closes affordance, and (#96).
- [ ] Run `cargo build` (bare — required for TUI work because `crates/otto` owns the `otto-tool-fs` binary the TUI spawns at runtime).
- [ ] Run `cargo test --workspace`; expect green.
- [ ] Run `cargo fmt --all --check` and `cargo clippy --workspace --all-targets` (CI uses `RUSTFLAGS=-D warnings`); expect clean.
- [ ] Launch `cargo run -p otto` and confirm by eye, because green tests do not cover the actual terminal render: pressing `/` puts `/` in the prompt; typing `co` leaves `/co` there while the list filters; arrows move `▶` without touching the prompt; no `> co` row appears above the prompt; a nonsense filter shows the no-matches line rather than a blank panel; backspacing past the `/` closes the palette; Enter on a no-arg command runs it and leaves the prompt empty.
- [ ] `cargo fmt --all` and commit: `git commit -m "docs: record the superseded #80 assumption and changelog #96"`.

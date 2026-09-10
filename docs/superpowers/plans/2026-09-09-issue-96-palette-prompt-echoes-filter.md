# issue-96-palette-prompt-echoes-filter Implementation Plan

**Goal:** While the `/` command palette is open, make the prompt input echo exactly what the user typed (`/` + filter) instead of the resolved name of the highlighted row, so the resolved command value reaches the prompt only on selection, per `savvagent/otto#96`.

**Architecture:** Keep the plugin-driven palette flow and its screen-owns-input model intact. `PaletteScreen` remains the owner of filter/cursor state and continues to drive slash execution; only the *content* it reports back through `Effect::PrefillInput` narrows, from "the highlighted command" to "the typed filter". Navigation keys stop emitting a prefill at all, matching `connect/screen.rs`. Two gaps open up as a direct consequence and are closed in the same change: backspacing past the leading `/` must close the palette (otherwise the prompt holds a character the user cannot delete), and the sheet needs a no-match empty state (otherwise dropping the now-duplicated `> <filter>` header leaves a blank panel). Dropping that header also shifts the render windowing budget from three reserved chrome rows to two.

**Tech Stack:** Existing `crates/otto` modules only (`command_palette/screen.rs`, `plugin/effects.rs`) plus the four locale catalogs. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-09-issue-96-palette-prompt-echoes-filter-design.md` — read it first, including "This is a reversal, not a narrowing" and "The trade this accepts". This plan implements it exactly.

**Commit discipline:** every task ends with `cargo test -p otto` green — not a narrower filter. The `command_palette::screen` filter hides a cross-file coupling (`effects.rs`'s palette-open test hard-asserts the old seed), so a task that only runs the narrow filter can leave the tree red and look green. Task 1 therefore folds in the `effects.rs` test rewrite rather than deferring it.

**Release line:** v0.27.1 (PATCH — a fix to behavior shipped by #92). Sequencing note: release PR #111 (v0.27.0) is open at the time of writing. If #111 merges first, this lands as v0.27.1. If this merges first, its CHANGELOG entry goes under `## [Unreleased]` and ships as part of v0.27.0. Check which is true in Task 4 rather than assuming.

**Branch:** `plugin/palette-prompt-echoes-filter`

## File Map

**New files**
- None.

**Modified files**
- `crates/otto/src/plugin/builtin/command_palette/screen.rs` — `prompt_preview` returns the literal filter; `Up`/`Down` emit nothing; `Backspace` on an empty filter closes; `render` drops the `> <filter>` header, reserves two chrome rows, and gains a no-match early return; tests rewritten.
- `crates/otto/src/plugin/effects.rs` — two stale comments and the palette-open test's seed assertion.
- `crates/otto/locales/{en,es,pt,hi}.toml` — new `picker.command-palette.no-matches` key.
- `docs/superpowers/specs/2026-09-08-issue-80-slash-command-input-design.md` — superseded-by note on the one overturned assumption.
- `docs/superpowers/specs/2026-09-09-issue-96-palette-prompt-echoes-filter-design.md` — the design source of truth.
- `docs/superpowers/plans/2026-09-09-issue-96-palette-prompt-echoes-filter.md` — this plan.
- `CHANGELOG.md` — a `Fixed` entry referencing #96.

**Explicitly not modified:** `crates/otto/src/app.rs`. The byte-vs-char cursor column in `prefill_input` was investigated and is not a bug — `CursorMove::Jump` clamps through `fit_col` (`min(col, chars().count())`). See the spec's Risks section; do not "fix" it here.

## Task 1: Narrow the prompt preview to the typed filter

**Files:**
- Modify: `crates/otto/src/plugin/builtin/command_palette/screen.rs`
- Modify: `crates/otto/src/plugin/effects.rs`

- [ ] Read `filtered`, `prompt_preview`, `render`, `on_key` and confirm the preview derives from screen-local state only.
- [ ] Rewrite the `prompt_preview_*` tests to the spec's success criteria so they fail first: opening shows `/`; typing `c` then `o` shows `/co` regardless of highlight; `Backspace` on a non-empty filter yields `/c`; a non-matching filter still yields `/<filter>`.
- [ ] Invert `up_emits_prefill_input` (`:699-708`) and `down_emits_prefill_input` (`:710-722`) to assert `effs.is_empty()`, rename them to say so, and rewrite their shared doc comment (`:668-669`) — it currently names `Up` and `Down` among the keys that emit `PrefillInput`. These two tests are the reason this step cannot be scoped to `prompt_preview_*` alone.
- [ ] Rewrite the seed assertion in `palette_open_screen_prefills_prompt_with_first_command` (`effects.rs:3083-3193`) to expect `/`, and rename the test accordingly. **Only** the seed assertion at `:3131-3147` changes — the second half presses `Esc`, applies the effects, and asserts the screen stack empties and the prompt clears. Keep that regression coverage intact.
- [ ] Run `cargo test -p otto` and confirm the new assertions fail before implementing.
- [ ] Replace `prompt_preview`'s three-way branch with `format!("/{}", self.filter)` and rewrite its doc comment — the stale doc describing "the currently highlighted command" is what would mislead the next reader.
- [ ] Change `Up` and `Down` to mutate `self.cursor` only and return `Ok(vec![])`, matching `connect/screen.rs:213-223`.
- [ ] Leave `Char` emitting `Effect::PrefillInput { text: self.prompt_preview() }`.
- [ ] Fix the two stale `effects.rs` comments that describe the old behavior without using the words `prompt_preview` or `highlighted`: the `"palette"` `open_screen` branch comment at `:662` and the guard test's doc comment at `:3195-3197`, both of which say the palette "mirrors its selection into the prompt". The `app.prefill_input(screen.prompt_preview())` call itself does not change — the behavior change is entirely upstream in `prompt_preview`; say so in the commit body.
- [ ] Re-read the empty-prompt guard above that call and confirm its reasoning still holds — the palette owns the draft for as long as it is open. Leave the guard alone.
- [ ] Verify the selection paths are untouched and their existing ordering tests stay green without edits: `Enter` no-arg → `Stack([CloseScreen, PrefillInput{""}, RunSlash])`; `Enter` `requires_arg` → `Stack([CloseScreen, PrefillInput{"/<cmd> "}])`; `Esc` and empty-result `Enter` → `[CloseScreen, PrefillInput{""}]`.
- [ ] Public-interface check: no SPP wire format, tool schema, plugin ABI, slash-command name, env var, or on-disk format change — internal TUI behavior only.
- [ ] `rg -n 'prompt_preview|highlighted|mirrors its selection|seeds the preview' crates/otto/src docs/ README.md` and fix any remaining stale claim that the prompt mirrors the highlighted command.
- [ ] Run `cargo test -p otto`; expect green.
- [ ] `cargo fmt --all` and commit: `git commit -m "otto: echo the typed filter in the palette prompt"`.

## Task 2: Close the palette on backspace past the leading slash

**Files:**
- Modify: `crates/otto/src/plugin/builtin/command_palette/screen.rs`

- [ ] Add a failing test: with an empty filter, `Backspace` emits `[CloseScreen, PrefillInput{""}]`; with filter `c`, it emits only `PrefillInput{"/"}` and does not close. Run `cargo test -p otto` and confirm failure.
- [ ] Change the `Backspace` arm so `self.filter.pop()` returning `None` produces `[CloseScreen, PrefillInput { text: String::new() }]`, and a successful pop keeps today's behavior (reset `cursor` to 0, emit the new preview).
- [ ] Cite the precedent in the code comment — `app.rs:1460-1462` documents closing the palette on backspace past the leading `/` — so this reads as established intent rather than a new invention.
- [ ] Run `cargo test -p otto`; expect green.
- [ ] `cargo fmt --all` and commit: `git commit -m "otto: close the palette on backspace past the slash"`.

## Task 3: Drop the filter header, add a no-match state, rebalance the window

**Files:**
- Modify: `crates/otto/src/plugin/builtin/command_palette/screen.rs`
- Modify: `crates/otto/locales/en.toml`, `crates/otto/locales/es.toml`, `crates/otto/locales/pt.toml`, `crates/otto/locales/hi.toml`

- [ ] Add failing render tests first:
      (a) no rendered line starts with `> `;
      (b) with `commands` non-empty and a filter matching nothing, the sheet renders the `no-matches` line and is not blank — this is the regression the header removal would otherwise introduce;
      (c) with a list longer than the region, sweep `region.height` over **`3..=14`** and, at cursor top/middle/bottom for each, assert the `▶` marker is present *and* the emitted line count never exceeds `height - 1` (the row `ui.rs::paint_screen` overpaints with `tips()`). The sweep starts at 3 deliberately: at `height ≤ 2` the `.max(1)` floor forces one row into a budget with no room, so `1 + capacity = 2 > height - 1` and the bound is false by construction. Assert `▶` presence across the full `1..=14` if you want the wider net — that half holds at every height, because the window always contains `self.cursor`.
      (d) the empty-`commands` path still renders `no-commands`.
      Run `cargo test -p otto` and confirm failure.
- [ ] Replace `cursor_row_never_lands_on_the_tips_row` (`:539-570`) with sweep (c) rather than keeping both — it pins a single `height: 12`, survives the capacity change unchanged, and would pass silently while (c) covers strictly more.
- [ ] Add `no-matches` to `crates/otto/locales/en.toml` under `[picker.command-palette]`, then `es.toml`, `pt.toml`, `hi.toml` — `crates/otto/tests/locales.rs::every_locale_has_the_same_key_set_as_en` fails if any is missed. Match each catalog's tone and the parenthesised style of its `no-commands` value.
- [ ] Remove the leading `StyledLine::plain(format!("> {}", self.filter))` from `render`.
- [ ] Add the no-match branch as an **early return**, placed and shaped exactly like the `commands.is_empty()` block at `:106-117` and taken before `name_col_width` and the windowing arithmetic. Pushing it after the row loop instead would leave the blank spacer at `:163-164` ahead of it, rendering `["", no-matches]`.
- [ ] Change `capacity` from `.saturating_sub(3).max(1)` to `.saturating_sub(2).max(1)` and rewrite the comment above it to name the two chrome rows that actually remain — the spacer/scroll-hint line and the sheet's last row, overpainted by `tips()`. Keep the explanation of *why* the last row is reserved (the window anchors the cursor on its last row, so a row hidden under `tips()` would be the selected one and `▶` would vanish for every scrolled list). Record that `.max(1)` is not panic-protection — capacity `0` yields a valid empty slice — but a floor that, at `height ≤ 2`, forces a row into a budget with no room; this change shrinks that broken range from `height ≤ 3` to `height ≤ 2`.
- [ ] Note in the comment that the blank spacer row stays unconditional because making it conditional on `hint` would be circular (`hint` derives from `capacity`).
- [ ] Update the three existing render tests deliberately rather than letting them pass or fail by accident:
      `overflowing_list_windows_around_cursor_with_scroll_hint` (`:486-530`) — at `height: 6` capacity goes 3 → 4, so `/cmd03` becomes visible and the hints become `↓16 more below` / `↑16 more above`;
      `list_exactly_filling_capacity_renders_every_row` (`:576-594`) — the boundary moves from 9 rows to 10; update its fixture, its `// capacity = height(12) - 3 = 9` comment, and its name if that encodes the number. It stays green either way, which is exactly why it needs a deliberate edit — a test passing for the wrong reason is worse than one failing.
      `long_list_fits_shows_no_scroll_hint` (`:459-478`) — stale capacity comment only.
- [ ] Confirm the `self.commands.is_empty()` early return still precedes the windowing arithmetic.
- [ ] Run `cargo test -p otto` and `cargo test -p otto --test locales`; expect green.
- [ ] `cargo fmt --all` and commit: `git commit -m "otto: drop the palette sheet's duplicate filter header"`.

## Task 4: Record the superseded assumption, changelog, full verification

**Files:**
- Modify: `docs/superpowers/specs/2026-09-08-issue-80-slash-command-input-design.md`
- Modify: `CHANGELOG.md`

- [ ] Annotate the assumption at `docs/superpowers/specs/2026-09-08-issue-80-slash-command-input-design.md:80` ("Showing the highlighted command name in the prompt means showing the full slash command path…") as superseded by `savvagent/otto#96`, pointing at this change's spec. Leave line 4's bare `Status: IMPLEMENTED` alone — that spec accurately records what #92 shipped, and it is not a blockquoted `> **Status:**` field despite what other plans in this directory imply.
- [ ] Add a `### Fixed` entry to `CHANGELOG.md` under the correct heading — `## [Unreleased]` (`:9`) or a new v0.27.1 section, per the Release line sequencing note. Leave the #92 entry under `## 0.26.4`'s `### Added` (`:34-44`) alone; the new entry supersedes it rather than rewriting history. Mention the behavior change, the backspace-closes affordance, and (#96).
- [ ] Run `cargo build` (bare — required for TUI work because `crates/otto` owns the `otto-tool-fs` binary the TUI spawns at runtime).
- [ ] Run `cargo test --workspace`; expect green.
- [ ] Run `cargo fmt --all --check` and `cargo clippy --workspace --all-targets` (CI uses `RUSTFLAGS=-D warnings`); expect clean.
- [ ] Launch `cargo run -p otto` and confirm by eye, because green tests do not cover the actual terminal render: pressing `/` puts `/` in the prompt; typing `co` leaves `/co` there while the list filters; arrows move `▶` without touching the prompt; no `> co` row appears above the prompt; a nonsense filter shows the no-matches line rather than a blank panel; backspacing past the `/` closes the palette; Enter on a no-arg command runs it and leaves the prompt empty.
- [ ] `cargo fmt --all` and commit: `git commit -m "docs: record the superseded #80 assumption and changelog #96"`.

## Deferred

- **Palette tips string.** `picker.command-palette.tips` reads "↑/↓ navigate · type to filter · Enter run · Esc cancel" (`crates/otto/locales/en.toml:216`) and is the palette's only affordance-discovery surface, so Task 2's new backspace-closes gesture is undiscoverable. Adding it means re-translating the tips line in all four catalogs to sell a convenience gesture that duplicates `Esc`; not worth the churn inside this fix. Worth a follow-up if the gesture proves useful.
- **Ghost completion** — see the spec's "The trade this accepts". A separate, independently-designable change.

# issue-118-palette-ghost-completion Implementation Plan

**Goal:** While the `/` command palette is open and the highlighted row's name is a valid prefix
completion of the typed filter, render the remainder of that name as dim, non-editable "ghost" text
immediately after the prompt's cursor — restoring the predictive signal `savvagent/otto#96`
traded away, without writing anything the user didn't type into the prompt's real, submittable
buffer. Per `savvagent/otto#118`.

**Architecture:** Add one new optional method to the plugin `Screen` trait
(`crates/otto-plugin`), `ghost_completion(&self) -> Option<String>`, defaulting to `None` — the
same shape as the existing `tips()` seam. `PaletteScreen` is the only implementer for now, and its
implementation is a pure function of existing state (`filter`, `cursor`, `commands` via
`filtered()`); no new state is added to the screen. `ui.rs::render` reads this through the screen
stack after rendering the prompt textarea and paints the result directly into the frame buffer at
the textarea's own interior rect, computed via a new small helper so the geometry can never drift
from the textarea's actual `Block`. This is a render-time-only addition: `on_key`, `Effect`
handling, and everything downstream of a keypress are untouched.

**Tech Stack:** Existing `crates/otto-plugin` and `crates/otto` modules only
(`otto-plugin/src/screen.rs`, `otto/src/plugin/builtin/command_palette/screen.rs`,
`otto/src/ui.rs`). No new dependencies. `ratatui::buffer::Buffer::set_stringn` (already a
transitive dependency via `ratatui`, used for the first time in this file) is the only "new" API
surface, not a new crate.

**Spec:** `docs/superpowers/specs/2026-09-10-issue-118-palette-ghost-completion-design.md` — read
it first, including "Why not put the ghost text in the textarea's buffer" and "Risks & Open
Questions". This plan implements it exactly.

**Release line:** v0.29.0 (MINOR — an additive feature to the plugin ABI, not a fix; see the
spec's "Public-interface changes"). Current `Unreleased` section of `CHANGELOG.md` is empty as of
this writing (last release was v0.28.1); check whether another PR's release has landed first before
opening the release PR (Task 4 / Phase 4 step 12) rather than assuming.

**Branch:** `palette/ghost-completion`

## File Map

**New files**
- None.

**Modified files**
- `crates/otto-plugin/src/screen.rs` — new `Screen::ghost_completion` default trait method.
- `crates/otto/src/plugin/builtin/command_palette/screen.rs` — `PaletteScreen` implements
  `ghost_completion`; new unit tests.
- `crates/otto/src/ui.rs` — new `prompt_inner_rect` helper; render-time ghost-text overlay in
  `render()`; new render tests.
- `CHANGELOG.md` — an `Added` entry under `## [Unreleased]`.
- `docs/superpowers/specs/2026-09-10-issue-118-palette-ghost-completion-design.md` — the design
  source of truth (already committed).
- `docs/superpowers/plans/2026-09-10-issue-118-palette-ghost-completion.md` — this plan.

**Explicitly not modified:** `crates/otto/src/plugin/effects.rs`, `crates/otto/src/app.rs`, and
`crates/otto/locales/*.toml`. Nothing about `Effect` handling, `PrefillInput`, or translated
strings changes — this is a pure render-time addition with no new user-visible text and no new
effect.

## Task 1: `Screen::ghost_completion` trait method + `PaletteScreen` implementation

**Files:**
- Modify: `crates/otto-plugin/src/screen.rs`
- Modify: `crates/otto/src/plugin/builtin/command_palette/screen.rs`

- [ ] In `crates/otto/src/plugin/builtin/command_palette/screen.rs`'s `#[cfg(test)] mod tests`,
      add failing tests for a not-yet-existing `PaletteScreen::ghost_completion` (call it as an
      inherent method during this step — it becomes a trait method once the trait gains it below,
      but the test bodies don't need to change). The existing fixture
      (`fixture()`, `:385-397`) has `clear`/`demo`/`exit`/`theme`/`zeta`:
      - `ghost_completion_shows_the_remainder_of_a_prefix_match`: type `c` (highlights `clear`,
        the only fixture row starting with `c`), assert
        `ghost_completion() == Some("lear".to_string())`.
      - `ghost_completion_with_empty_filter_shows_full_highlighted_name`: with an empty filter
        (cursor defaults to the first row, `clear`), assert
        `ghost_completion() == Some("clear".to_string())`.
      - `ghost_completion_is_none_for_substring_only_match`: build a palette with
        `[cmd("acl-export", false), cmd("clear", false)]` (mirrors the existing
        `tips_name_a_plugin_command_that_captures_the_highlight` fixture at
        `screen.rs:1043-1060`), type `cl` (highlights `acl-export`, a substring match but not a
        prefix match), assert `ghost_completion() == None`.
      - `ghost_completion_is_none_when_filter_exactly_matches_the_full_name`: type `clear` in full,
        assert `ghost_completion() == None` (empty-suffix suppression).
      - `ghost_completion_is_none_when_nothing_is_highlighted`: type a filter matching nothing
        (`xyz`), assert `ghost_completion() == None`.
      - `ghost_completion_follows_navigation`: type `e` (matches multiple fixture rows containing
        `e`), press `Down` to move the highlight, assert `ghost_completion()` changes to reflect
        the newly highlighted row's suffix (or `None` if that row isn't a prefix match — pick
        fixture rows where you know the answer; assert the concrete expected value, don't just
        assert it changed).
      Run `cargo test -p otto` and confirm these fail to compile (no such method yet) — that
      compile failure *is* the "red" of this step's TDD; there is no way to get a runtime red for a
      brand-new method.
- [ ] Add to `crates/otto-plugin/src/screen.rs`'s `Screen` trait, directly after `tips()`:
      ```rust
      /// Optional: the remainder of a predicted completion, rendered as dim
      /// "ghost" text immediately after the prompt's cursor. Returning `None`
      /// (the default) means: no ghost text. This is advisory only — it is
      /// never written into the prompt's editable buffer, so it can never be
      /// deleted, submitted, or otherwise treated as real input.
      fn ghost_completion(&self) -> Option<String> {
          None
      }
      ```
- [ ] In `crates/otto/src/plugin/builtin/command_palette/screen.rs`, inside the existing
      `impl Screen for PaletteScreen` block (after `tips`), add:
      ```rust
      fn ghost_completion(&self) -> Option<String> {
          let filtered = self.filtered();
          let (_, cmd) = filtered.get(self.cursor)?;
          let filter_lower = self.filter.to_ascii_lowercase();
          if !cmd.name.to_ascii_lowercase().starts_with(&filter_lower) {
              return None;
          }
          let suffix: String = cmd.name.chars().skip(self.filter.chars().count()).collect();
          (!suffix.is_empty()).then_some(suffix)
      }
      ```
      Give it a doc comment explaining the two narrowings (prefix-only; empty-suffix suppressed)
      and pointing at the spec's "Approach > 2" for the full rationale — the same density of
      comment this file already uses elsewhere (see `prompt_preview`'s doc comment for the house
      style).
- [ ] Run `cargo test -p otto`; the new tests should now compile and pass. If any fixture-derived
      expected value in the tests above was wrong (double check `clear`'s suffix after typing `c`
      is `"lear"`, not `"clear"`), fix the test, not the implementation, unless the implementation
      is actually wrong.
- [ ] Public-interface check: this step adds the trait method. Confirm in the commit body that it
      is additive (default `None`, no existing `impl Screen for ...` block requires changes) — cite
      `rg -n "impl Screen for" crates/` to show every implementer compiles unchanged (they will,
      since none of them override `ghost_completion` yet).
- [ ] `cargo fmt --all` and commit: `git commit -m "otto-plugin: add Screen::ghost_completion, palette implements it"`.

## Task 2: Render-time overlay in `ui.rs`

**Files:**
- Modify: `crates/otto/src/ui.rs`

- [ ] Read `ui.rs::render` fully from the textarea construction (`:241-258`) through the
      screen-stack paint (`:385-402`) and confirm the insertion point: directly after
      `frame.render_widget(&textarea, chunks[4]);` (`:345`) and before the
      `if let Some((top_screen, layout)) = app.screen_stack.top()` block that calls `paint_screen`
      (`:385`).
- [ ] Add a failing test first. `ui.rs` render tests in this file construct an `App`, populate
      `screen_stack` with a test double, call `render`, and assert on `frame.buffer()` /
      `terminal.backend().buffer()` content — follow the pattern of an existing test in this file
      that opens a screen and inspects rendered cells (e.g. the `paint_screen_*` tests around
      `:1944-2000` and the `bottom_sheet_*` tests) rather than inventing a new test harness shape.
      Write:
      - A test that opens a `PaletteScreen` (or a minimal test `Screen` impl overriding only
        `ghost_completion` to return a known string, whichever matches this file's existing test
        double conventions more closely — check for an existing mock `Screen` impl in this file's
        `#[cfg(test)] mod tests` before writing a new one) with a known prompt string and asserts
        the expected ghost characters appear in the buffer at
        `textarea's inner x + prompt.chars().count()`, styled with `palette.muted` as the fg.
      - A test that the overlay does **not** draw when `screen_stack` is empty (home screen, no
        palette open) — buffer at the expected cell is unchanged from what plain textarea rendering
        would produce.
      - A test that the overlay does not draw when the top screen's `ghost_completion()` returns
        `None` (e.g. a palette with an empty command list, or an existing screen type that doesn't
        override the trait method).
      Run `cargo test -p otto` and confirm these fail (compile error or wrong buffer content) before
      implementing.
- [ ] Factor the existing inline `Block::default().borders(Borders::ALL).border_style(...)
      .padding(Padding::horizontal(1))` builder at `ui.rs:247-252` into a `fn prompt_block(palette:
      Palette) -> Block` (or take the border style components it needs — match whatever the actual
      surrounding code makes cleanest) placed near `bottom_sheet_rect` (`:1191`). Replace the inline
      builder at `:247-252` with a call to it, so `textarea.set_block(prompt_block(palette))`
      produces the exact same `Block` as before (no behavior change to today's render — verify with
      `cargo test -p otto` before adding anything new). **This is a mandatory design fix from the
      spec's critique round, not optional:** a spec critique caught that an earlier draft proposed a
      *second*, independently-written `Block::borders(ALL).padding(horizontal(1))` literal for the
      ghost overlay's interior-rect computation — two sources of truth with nothing forcing them to
      stay identical. Do not reintroduce that shape (a standalone `prompt_inner_rect` helper with a
      duplicated `Block` literal); derive the overlay's interior rect from the same `Block` value
      via `.inner(chunks[4])` instead.
- [ ] Add the overlay block from the spec's revised "Approach > 3" directly after
      `frame.render_widget(&textarea, chunks[4]);`, computing `let inner =
      block.inner(chunks[4]);` from the `Block` value `prompt_block` returned (reused, not
      rebuilt), then using `app.screen_stack.top()`, `top_screen.ghost_completion()`,
      `textarea.cursor()`, `textarea.lines().len() == 1`, and `frame.buffer_mut().set_stringn(...)`
      exactly as specified. **Also mandatory from the critique round:** guard on `line_fits` —
      `textarea.lines().first().is_some_and(|l| l.chars().count() <= inner.width as usize)` — in
      addition to (not instead of) `cursor_row == 0 && textarea.lines().len() == 1`. The critique
      found that `cursor_row`/`lines().len()` alone report *logical* line/row and do not change when
      `WrapMode::WordOrGlyph` soft-wraps a long logical line across multiple *visual* rows — so that
      guard alone would silently paint the ghost text at the wrong screen position the moment a
      wrapped prompt reached it. `line_fits` proves no wrap occurred by checking the actual
      condition wrapping depends on (line width vs. available width), not a proxy for it. Style:
      `palette.base_style().fg(palette.muted)` — matches this file's existing muted-text
      convention (see the `no-matches`/`no-commands` styling and the Assumptions section of the
      spec for why no `Modifier::DIM`).
- [ ] Add at least one test exercising a filter long enough (relative to a narrow test-terminal
      width) that `line_fits` would be false if the check were only `cursor_row == 0 &&
      textarea.lines().len() == 1` — i.e. a regression test for the specific gap the critique found,
      not just the common short-filter case. Assert no ghost text renders in that case even though
      `ghost_completion()` itself returns `Some(...)`.
- [ ] Host-swap `RwLock` check (Non-Negotiable Rule 7 / Load-Bearing Invariant 3): confirm this
      function (`ui.rs::render`) does not hold `app`'s `Arc<RwLock<Option<Arc<Host>>>>` guard and
      does not `.await` anywhere in or near the new code — `render` is a synchronous ratatui draw
      callback, so this should be true by construction, but state it explicitly in the commit body
      per the plan format's requirement, since this file is one of the two named in the rule.
- [ ] Run `cargo test -p otto`; expect the new tests green and no existing test's buffer
      assertions disturbed (the overlay only draws when a screen with a non-`None`
      `ghost_completion()` is on top, which no currently-shipped screen other than the palette
      provides, so no other test's expected buffer content should change).
- [ ] `cargo fmt --all` and commit: `git commit -m "otto: render palette ghost-completion after the prompt cursor"`.

## Task 3: Changelog, full verification, manual terminal check

**Files:**
- Modify: `CHANGELOG.md`

- [ ] Add an `### Added` entry under `## [Unreleased]` in `CHANGELOG.md` (currently empty —
      confirm it is still empty before adding, in case another PR landed first) describing: the
      command palette now shows the remainder of the highlighted command as dim ghost text after
      the cursor when it's a valid completion of what's typed; references `#118`; notes it's an
      additive `Screen::ghost_completion` trait method for plugin authors who want the same
      behavior in their own screens.
- [ ] Run `cargo build` (bare — required even for this TUI-only change, since `crates/otto` owns
      the `otto-tool-fs` `[[bin]]` the TUI spawns at runtime).
- [ ] Run `cargo test --workspace`; expect green.
- [ ] Run `cargo fmt --all --check` and `cargo clippy --workspace --all-targets` (CI uses
      `RUSTFLAGS=-D warnings`); expect clean.
- [ ] Launch `cargo run -p otto` and confirm by eye, because green tests do not cover the actual
      terminal render (the otto-development skill's "green tests are not the same as work-done"
      rule, and this spec's own Risks section flags the lack of a visual-regression test):
      - Press `/` — the highlighted row's full name appears as dim ghost text after the cursor.
      - Type toward a command whose prefix is unambiguous (e.g. `co` toward `connect`) — ghost text
        shrinks correctly as more is typed and disappears when the filter equals the full name.
      - Type a filter that only substring-matches a lower-priority command (if any real builtin
        pair reproduces the `acl-export`/`clear` shape; otherwise reason about the nearest real
        case) — confirm no ghost text renders and `tips()` still names the actual highlighted
        command.
      - Arrow through the list — ghost text updates with the highlight; backspacing removes a real
        character and ghost text recomputes.
      - Confirm the ghost text is visually distinct (dimmer/muted) from the real prompt text in
        both the light and dark theme, if this repo's `/theme` command can switch themes in this
        session — otherwise note in the PR body that only the default theme was checked.
      - Confirm `Enter` still runs exactly the highlighted command and the ghost text never
        appears in the submitted/dispatched value.
- [ ] `cargo fmt --all` and commit: `git commit -m "docs: changelog issue-118 palette ghost-completion"`.

## Deferred

- **Wrap-aware / multi-line ghost text.** Out of scope per the spec; the overlay no-ops rather than
  computing a wrapped cursor's screen row. Worth a follow-up only if a future screen wants
  ghost-completion on a multi-line prompt.
- **Wide-character-safe ghost rendering.** Not reachable today (slash command names are ASCII); not
  designed for.
- **Extending `ghost_completion` to other screens** (`connect`'s picker, transcript picker). The
  seam is generic; no other screen is changed in this PR.

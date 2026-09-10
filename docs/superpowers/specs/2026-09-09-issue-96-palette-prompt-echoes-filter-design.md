# Palette prompt echoes the typed filter — design

Date: 2026-09-09
Status: DRAFT
Related: `savvagent/otto#96`. Reverses part of `savvagent/otto#80` (shipped as PR #92).

## Problem

PR #92 (closing #80) made the prompt input mirror the command palette's *highlighted row*. Opening the palette seeds the prompt with the first command, and every subsequent keystroke rewrites the prompt with the resolved name of whatever is currently highlighted (`crates/otto/src/plugin/builtin/command_palette/screen.rs:70-87`, `:214-262`; `crates/otto/src/plugin/effects.rs:696-700`).

The result is that the prompt stops echoing the user:

| Keys | Sheet header | Prompt input (today) |
|---|---|---|
| `/` | `> ` | `/clear` |
| `/c` | `> c` | `/changelog` |
| `/co` | `> co` | `/connect` |
| `↓` | `> co` | *(next match)* |
| `⌫` | `> c` | `/changelog` |

Two defects follow:

1. **The prompt contradicts the keystrokes that produced it.** The user typed `/co`; the prompt reads `/connect`. Backspace is the sharpest case — deleting one character replaces the whole word with a different command. The prompt is the one surface a terminal user expects to echo their input verbatim, and here it does not.
2. **An unmade choice is rendered as made.** `/clear` sits in the prompt before the user has expressed any intent beyond pressing `/`. The `▶` highlight in the list already says "Enter runs this"; restating it as literal prompt text reads as a staged command the user never confirmed.

There is also a presentation consequence. The palette is a `BottomSheet` anchored directly above the prompt (`crates/otto/src/ui.rs:1190-1216`), so its `> <filter>` header line renders one row above the prompt. Once the prompt echoes the filter, those two rows carry identical text.

### This is a reversal, not a narrowing

#80's body asked, verbatim, that "the input prompt should show the slash command **and the command currently highlighted in the command list**." #92 implemented exactly that, and it is what shipped in v0.26.4. #96 — filed by the same reporter one day later — asks for the opposite. This spec does not claim continuity with #80; it records that the reporter changed their mind after using the behavior, and that their latest word governs.

That framing matters for the next reader. Anyone who opens #80 will find an explicit acceptance criterion that this change deletes. The justification is not "#80 never asked for this" (it did) but the two defects above, which only became visible once the behavior was in a real terminal.

### The trade this accepts

Under #92 the prompt always held a runnable command, and `Enter` ran what the prompt displayed. This change breaks that correspondence, and the break is wider than it first looks because `filtered()` matches on `name.contains(filter)` — substring, not prefix, and case-folded on the filter side only (`screen.rs:62-68`). After this change the prompt can legitimately read `/nnec` (which matches `connect`), `/CO`, or `/xyz` (which matches nothing), while `Enter` dispatches the highlighted row regardless. The prompt becomes honest about keystrokes and silent about consequence.

The design that resolves both sides of this tension is a third one: echo the typed text and render the highlighted row's completion after the cursor as dim ghost text (`/co` + faint `nnect`), the way shell autosuggestion works. That keeps the prompt a faithful echo *and* keeps it predictive of `Enter`. It is deliberately out of scope here, for a concrete reason rather than a size hand-wave: the prompt is a `tui_textarea::TextArea` whose buffer is the user's actual editable text (`crates/otto/src/app.rs:1963-1974`), so ghost text cannot be part of that buffer without becoming deletable, submittable content. It needs a separate render-time overlay pass keyed off palette state — new plumbing between the palette screen and `ui.rs`, not a change to `prompt_preview`. That is a larger, independently-designable change; it should be filed as a follow-up if the substring/`Enter` mismatch proves confusing in practice.

## Approach

Narrow the preview from "the highlighted command" to "what the user typed", let the sheet stop drawing its own copy of the filter, and close the two gaps that only open up once the prompt is a literal echo — an undeletable leading `/`, and a sheet with nothing left to draw when the filter matches nothing.

### 1. `prompt_preview` becomes the literal filter

`PaletteScreen::prompt_preview()` returns `format!("/{}", self.filter)` unconditionally. The three-way branch on the highlighted row disappears; the empty-filter case (`/`) and the no-match case (`/<filter>`) both fall out of the single expression rather than being special-cased. The method keeps its name and signature — it still answers "what should the prompt show while this screen owns input" — but its doc comment is rewritten, because as written it describes mirroring the highlighted command.

### 2. Navigation stops touching the prompt

`Up` and `Down` mutate `self.cursor` only and return `Ok(vec![])`, matching the sibling picker (`crates/otto/src/plugin/builtin/connect/screen.rs:213-223`). Moving the highlight is not an edit to the user's text, so it must not produce a `PrefillInput`. This is safe for rendering: the TUI main loop calls `terminal.draw(...)` unconditionally at the top of every iteration and polls events on a 50 ms timeout (`crates/otto/src/main.rs:3375-3392`, `:3598`), so the `▶` highlight repaints at ≥20 Hz whether or not a key produced an effect.

`Char` continues to emit `Effect::PrefillInput { text: self.prompt_preview() }`, because that *is* an edit to the user's text.

### 3. Backspace past the leading `/` closes the palette

`Backspace` on a non-empty filter pops one char and emits the new preview, as today. On an **empty** filter it now emits `[CloseScreen, PrefillInput { text: "" }]` instead of re-emitting `/`.

This gap is created by change 1 and must be closed with it. Under #92 the prompt held a screen-owned string like `/clear`, so a no-op backspace was unremarkable. Once the spec promises the prompt is *what the user typed*, a leading `/` that cannot be deleted is a direct contradiction of that promise — and it leaves backspace as the only editing key with no effect, in a prompt the user believes they are editing.

Closing on backspace-past-the-start is this codebase's own established intent, not a new invention: the legacy palette helper documents exactly it (`crates/otto/src/app.rs:1460-1462` — *"Returns `false` if it was already empty (the caller can use this to close the palette on Backspace past the leading `/`)"*).

### 4. The sheet drops its filter header

`render` no longer pushes the leading `StyledLine::plain(format!("> {}", self.filter))`. The prompt one row below is the input surface; the sheet is the result list.

This frees a row in the sheet's fixed height, so the windowing budget drops from three reserved chrome rows to two (`capacity = region.height - 2`). The accounting: `ui.rs::paint_screen` renders the paragraph over `sheet` and *then* overpaints `tips()` at `sheet.y + sheet.height - 1` (`crates/otto/src/ui.rs:1331-1352`), so `render` must emit at most `height - 1` lines. Before: `1` header + `1` hint/spacer + `capacity = h - 3` = `h - 1`. After: `1` hint/spacer + `capacity = h - 2` = `h - 1`. The load-bearing property is unchanged — the window anchors so the cursor sits on its last visible row, at line index `1 + (capacity - 1) = h - 2`, which stays strictly above the overpainted `h - 1`. Only the constant moves.

The blank spacer row is kept unconditionally even though the header it used to separate is gone. Making it conditional on `hint` being empty is circular — `hint` is derived from `hidden_above`/`hidden_below`, which depend on `capacity`, which would then depend on `hint`. Reserving the row unconditionally is what keeps that arithmetic non-recursive, so it stays; with no header above it, the scroll hint simply becomes the sheet's first line.

### 5. A no-match empty state

With `commands` non-empty and `filtered()` empty, trace the current `render`: the `commands.is_empty()` early return is not taken, the window is `0..0`, `hidden_above`/`hidden_below` are both `0` so the hint is blank, and the row loop emits nothing. Today the sheet still shows something, because `lines[0]` is the `> xyz` header. Remove the header and that state renders as a blank rectangle above the prompt with only the host's `tips()` row painted.

So change 4 requires a matching empty state: when `filtered()` is empty but `commands` is not, render a muted `picker.command-palette.no-matches` line. This is an **early return**, placed and shaped exactly like the existing `commands.is_empty()` block (`screen.rs:106-117`) and taken before `name_col_width` and the windowing arithmetic. Pushing the line after the row loop instead would leave the blank spacer at `:163-164` ahead of it and render `["", no-matches]`. The key is added to all four catalogs (`crates/otto/locales/{en,es,pt,hi}.toml`) — `crates/otto/tests/locales.rs::every_locale_has_the_same_key_set_as_en` fails otherwise.

## Scope

**In:**
- `crates/otto/src/plugin/builtin/command_palette/screen.rs` — `prompt_preview` returns the literal filter; `Up`/`Down` stop emitting `PrefillInput`; `Backspace` on an empty filter closes the palette; `render` drops the `> <filter>` header, reserves two chrome rows instead of three, and gains a no-match empty state.
- `crates/otto/src/plugin/effects.rs` — the `"palette"` `open_screen` branch comment (`:662`) and the guard test's doc comment (`:3195-3197`), both of which say the palette "mirrors its selection into the prompt"; no logic change. Its test `palette_open_screen_prefills_prompt_with_first_command` (`:3083-3193`) hard-asserts the old seed and must be renamed, with the seed assertion at `:3131-3147` rewritten to expect `/`. Only that assertion changes — the second half of the test presses `Esc` and asserts the stack empties and the prompt clears, which is regression coverage worth keeping. This is the one cross-file test coupling.
- `crates/otto/locales/{en,es,pt,hi}.toml` — a new `picker.command-palette.no-matches` key in each.
- `docs/superpowers/specs/2026-09-08-issue-80-slash-command-input-design.md` — a superseded-by note on the one assumption #96 overturns. Its `> **Status:**` stays IMPLEMENTED; it accurately records what #92 shipped.
- `CHANGELOG.md` — a `Fixed` entry referencing #96. The existing v0.26.4 entry describing #92's behavior is left alone; the new entry supersedes it rather than rewriting history.
- Tests in `screen.rs`. `up_emits_prefill_input` (`:699-708`) and `down_emits_prefill_input` (`:710-722`) assert the behavior change 2 removes and must be inverted to assert no effects; their shared doc comment (`:668-669`) names `Up` and `Down` among the keys that emit `PrefillInput` and goes stale with them. Beyond that and the `prompt_preview_*` rewrite, three existing render tests are affected by the capacity change and must be updated deliberately rather than left to pass or fail by accident:
  - `overflowing_list_windows_around_cursor_with_scroll_hint` (`:486-530`) — at `height: 6`, capacity goes 3 → 4, so its `/cmd03` and `↓17 more below` assertions both break.
  - `list_exactly_filling_capacity_renders_every_row` (`:576-594`) — stays green but stops testing its boundary; the "exactly fills" case moves from 9 rows to 10. Its name, its `// capacity = height(12) - 3 = 9` comment, and its fixture all need updating, because silently passing for the wrong reason is worse than failing.
  - `long_list_fits_shows_no_scroll_hint` (`:459-478`) — stale capacity comment.

**Out:**
- Selection behavior. `Enter` on a `requires_arg` command still closes and prefills `/<cmd> `; `Enter` on a no-arg command still clears and dispatches `RunSlash`; `Esc` and empty-result `Enter` still close and clear. This spec changes only what appears *before* selection.
- How slash commands are discovered, filtered (`name.contains(filter)`), ordered, or dispatched. The substring-matching consequence is documented above, not changed.
- Inline ghost-completion rendering of the highlighted command — see "The trade this accepts". Worth a follow-up issue; not this change.
- The `/`-opens-only-on-an-empty-prompt rule (`main.rs:89-94` `should_route_home_keybinding`) and the empty-prompt guard in `open_screen` (`effects.rs:660-690`). Both still hold: the palette owns the draft for as long as it is open, so seeding `/` instead of `/clear` changes nothing about either one's reasoning.

## Public-interface changes

None. Internal TUI behavior using existing plugin effects. No SPP wire format, tool schema, `ProviderHandler`/`ProviderClient` method, plugin ABI surface, slash-command name, env var, or on-disk transcript/keyring format changes. The new locale key is additive.

## Premise corrections

- The issue as filed had an empty body and a title with two typos ("now" for "not", "util" for "until"). It has been rewritten in place against the reading taken here — that the resolved value of the highlighted command must not reach the prompt until selection. This is the only reading under which #96 describes a defect rather than restating what #92 shipped. **This reading contradicts #80's written acceptance criteria**; see "This is a reversal, not a narrowing" above.
- #92's spec recorded the opposite choice under Assumptions: *"Showing the highlighted command name in the prompt means showing the full slash command path (`/connect`, `/clear`, etc.), not just the raw typed filter."* That is the assumption #96 overturns.
- The `> <filter>` header and the three-row budget were **not** introduced by #92. Both trace to `e6b8e1c`, *"Render the command palette as an inline overlay above the prompt (#19)"* (2026-09-05), four days earlier. They are coupled to each other, but the coupling predates the prompt-preview work.
- The egui front-end referenced throughout #92's spec was removed in #94. Only the TUI path exists now.

## Assumptions

- Echoing the filter verbatim, with no completion hint, is acceptable for now. The `▶` marker and accent styling already communicate what `Enter` will run; the cost is that the prompt no longer predicts it (see "The trade this accepts").
- Dropping the sheet's `> <filter>` header belongs in this change rather than a separate one, because it becomes redundant *as a result of* this change — and because dropping it is what forces the no-match empty state, which would otherwise be a blank-panel regression.
- Keeping `PrefillInput` on `Char`/`Backspace` (rather than routing those keys to the textarea directly) preserves the existing screen-owns-input model. The palette screen still receives every key; it just reports a different string back.

## Goal & Success Criteria

While the command palette is open, the prompt input shows exactly what the user typed and nothing more; the resolved command value reaches the prompt only on selection.

- [ ] Pressing `/` opens the palette and puts `/` — not the first command — in the prompt.
- [ ] Typing `c`, then `o`, leaves `/co` in the prompt, regardless of which row is highlighted.
- [ ] `Up`/`Down` move the `▶` highlight, return no effects, and leave the prompt untouched.
- [ ] Backspace on a non-empty filter removes exactly one character (`/co` → `/c`).
- [ ] Backspace on an empty filter closes the palette and clears the prompt.
- [ ] A filter matching nothing shows `/<filter>` in the prompt and the `no-matches` line in the sheet — never a blank sheet.
- [ ] `Enter` on a no-argument command still clears the prompt and dispatches, in that effect order.
- [ ] `Enter` on an argument-taking command still closes the palette and leaves `/<cmd> ` in the prompt.
- [ ] `Esc` still closes the palette and clears the prompt.
- [ ] The sheet renders no `> <filter>` header.
- [ ] A command list longer than the sheet windows around the cursor with the `▶` row visible at every cursor position, across a sweep of region heights — not just one.

## Error Handling & Edge Cases

- **Empty command list.** The prompt shows `/` and the sheet shows the existing `no-commands` body. The early return in `render` precedes the windowing arithmetic, so the capacity change cannot affect it.
- **No filtered match.** The prompt shows `/<filter>`; the sheet shows the new `no-matches` body. `Enter` here still takes the empty-result path: close and clear.
- **Cursor at a list edge.** `Up` at the top and `Down` at the bottom are no-ops for both cursor and prompt.
- **A degenerate sheet height.** `capacity` is `.max(1)`. This is not panic-protection — a capacity of `0` would yield a valid empty slice, not a panic. What `.max(1)` does at `height ≤ 2` is force one command row into a budget with no room for it, so the host's `tips()` overpaints the `▶` row. That boundary is real but pre-existing, and this change *improves* it: the broken range shrinks from `height ≤ 3` to `height ≤ 2`. `bottom_sheet_rect` can legitimately produce such heights (see its own test `bottom_sheet_with_no_room_above_the_prompt_is_empty`, `ui.rs:2226-2233`), so the guard test sweeps heights rather than pinning one.

## Risks & Open Questions

- The windowing arithmetic in `render` is the delicate part of this change, not the preview logic. The reserved-row count is coupled to the number of chrome lines through `region.height`, and the comment explaining *why* the count is what it is must move with the constant, or the next reader will re-derive the old value. The guard is a test that sweeps region heights at several cursor positions and asserts the `▶` row is always present — the current test pins a single `height: 12` (`screen.rs:539-570`), which is not enough to catch an off-by-one at the boundary.
- The substring/`Enter` mismatch documented in "The trade this accepts" is a real usability cost being accepted knowingly. If it generates complaints, the ghost-completion design is the answer, and it is a new issue rather than a revert.
- **Investigated and deliberately not changed:** `App::prefill_input` sets the cursor column from `l.len()` — bytes, not chars (`crates/otto/src/app.rs:1963-1974`). Because this change feeds the prompt arbitrary typed characters on every keystroke rather than ASCII command names, that looked like a latent bug this change would make routine. It is not one: `CursorMove::Jump` clamps the column through `fit_col`, which is `min(col, line.chars().count())` (`tui-textarea-2-0.10.2/src/cursor.rs:271-273`, arm at `:373-377`), and byte length is always ≥ char count, so the cursor lands on exactly the char count today. The `l.len()` is therefore correct only by way of that clamp, which is worth knowing but is not this issue's business — changing it here would widen the diff for no behavior delta. Recorded so the next person does not re-derive it.

- `prompt_preview` keeps its name but no longer previews the *command*. Its doc comment must be rewritten in the same commit, or it will read as stale against #92's spec.
- This change reverses a behavior the reporter explicitly asked for one release earlier. If #96's intent was in fact something narrower, the whole change is misdirected — which is why the issue was rewritten in place and the reversal is called out at the top of this spec rather than buried.

# Ghost-completion in the palette prompt — design

Date: 2026-09-10
Status: DRAFT
Related: `savvagent/otto#118`. Follow-up to `savvagent/otto#96` (shipped in PR #114). Also
references `savvagent/otto#80` and `savvagent/otto#92`.

## Problem

`savvagent/otto#96` (PR #114) made the palette prompt echo exactly what the user typed — `/` plus
the raw filter — instead of the resolved name of the highlighted row
(`crates/otto/src/plugin/builtin/command_palette/screen.rs::prompt_preview`). That fixed the
defect where backspacing one character could replace the whole prompt word with a different
command, but it traded away prediction: the prompt no longer indicates what `Enter` will actually
run.

The gap is wider than it looks because `PaletteScreen::filtered()` matches on
`name.contains(filter)` — a substring, not a prefix, case-folded on the filter side only
(`screen.rs:62-68`). So the prompt can legitimately read `/nnec` while the highlighted row (and
what `Enter` dispatches) is `/connect`. PR #114 mitigated this by making `Screen::tips()` name the
command `Enter` would run (`screen.rs::tips`), but that puts the disambiguating information on a
different row than the text it disagrees with — a user watching the prompt, not the tips line one
row up, still has no in-line signal.

The design spec that shipped #96
(`docs/superpowers/specs/2026-09-09-issue-96-palette-prompt-echoes-filter-design.md`, "The trade
this accepts") named the fix and explicitly deferred it:

> [...] echo the typed text and render the highlighted row's completion after the cursor as dim
> ghost text (`/co` + faint `nnect`), the way shell autosuggestion works. [...] It needs a separate
> render-time overlay pass keyed off palette state — new plumbing between the palette screen and
> `ui.rs`, not a change to `prompt_preview`. [...] it should be filed as a follow-up if the
> substring/`Enter` mismatch proves confusing in practice.

`savvagent/otto#118` is that follow-up, filed to record the gap rather than because it has already
caused reported confusion (see the issue body: "Worth doing when the typed-vs-dispatched mismatch
actually confuses people in practice; `tips()` may already be enough mitigation"). This spec
proceeds on the reading that the job exists to close the gap now that the design is understood, and
records that judgment call explicitly (see Assumptions).

## Approach

Add a new optional seam on the plugin `Screen` trait — "the completion for the current
highlight" — and have `ui.rs` paint it as dim text after the textarea's cursor, without writing it
into the textarea's real, editable buffer.

### 1. `Screen::ghost_completion` — a new default trait method

`crates/otto-plugin/src/screen.rs` gains:

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

This mirrors the existing `tips()` seam (`screen.rs:38-43`): a default no-op method every screen
inherits, overridden only by screens that have something to say. Adding it does not touch `id`,
`render`, `on_key`, or `on_event` — no existing `impl Screen for ...` block breaks.

### 2. `PaletteScreen::ghost_completion`

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

Two deliberate narrowings relative to "the highlighted row's completion" read literally:

- **Prefix-only.** `filtered()` is a substring match, so the highlighted row is not always a valid
  completion of what was typed (`/cl` highlighting `/acl-export` — the exact case `tips()` exists
  for). Ghost text that does not literally continue the typed characters would misrepresent itself
  as a completion instead of a prediction; when the highlight is not a prefix match this returns
  `None` and the row falls back to relying on `tips()` alone, exactly as it does today. This means
  ghost text and `tips()` are not redundant: ghost text covers the common case (typing toward a
  visible prefix match) with an inline signal, `tips()` remains the only signal for the substring
  case.
- **Empty suffix suppressed.** When the typed filter already equals the highlighted name in full
  (`/clear` with cursor on `clear`), the suffix is empty; `then_some` turns that into `None` so
  nothing renders. An empty ghost span would be a no-op visually but is worth naming explicitly so
  a future reader doesn't wonder whether it was missed.

Char-based skip (not byte slicing) so a filter containing a multi-byte character can never land the
slice on a byte boundary that isn't a char boundary; command names are ASCII in practice, but the
filter is arbitrary user keystrokes.

### 3. `ui.rs` render-time overlay

`crates/otto/src/ui.rs::render` builds and renders the prompt `textarea` at `chunks[4]`
(`ui.rs:246-258, 345`), then separately paints the screen stack (`ui.rs:385-402`) via `paint_screen`,
which only ever draws into the `BottomSheet`/`Fullscreen` region *above* the prompt — it never
touches the prompt's own rect. Ghost text has to be painted into the prompt's rect, so it is a third,
new step, inserted directly after the textarea render call and before the screen-stack paint:

```rust
let block = prompt_block(&palette); // factored out of the inline builder below
textarea.set_block(block.clone());
// ... existing set_style / set_cursor_line_style / measure calls, unchanged ...
frame.render_widget(&textarea, chunks[4]);

if let Some((top_screen, _)) = app.screen_stack.top() {
    if let Some(ghost) = top_screen.ghost_completion() {
        let (cursor_row, cursor_col) = textarea.cursor();
        let single_line = textarea.lines().len() == 1;
        // `line_fits` proves the line cannot have wrapped onto a second
        // visual row: if the whole logical line's char count is within
        // the interior width, `WrapMode::WordOrGlyph` has nothing to wrap.
        // This is the actual no-wrap guarantee — `cursor_row == 0` and
        // `lines().len() == 1` alone do NOT detect wrapping, because both
        // report the *logical* line/row, unchanged by soft-wrapping a
        // long logical line across multiple *visual* rows. See Risks.
        let inner = block.inner(chunks[4]);
        let line_fits = textarea
            .lines()
            .first()
            .is_some_and(|l| l.chars().count() <= inner.width as usize);
        if cursor_row == 0 && single_line && line_fits {
            let x = inner.x.saturating_add(cursor_col as u16);
            if x < inner.x + inner.width {
                let max_width = (inner.x + inner.width - x) as usize;
                frame.buffer_mut().set_stringn(
                    x,
                    inner.y,
                    &ghost,
                    max_width,
                    palette.base_style().fg(palette.muted),
                );
            }
        }
    }
}
```

`prompt_block(&palette) -> Block` is factored out of the inline block-builder `render()` already
constructs (`borders(ALL)` + `padding(Padding::horizontal(1))` + border styling,
`ui.rs:247-252`) so there is exactly **one** place that defines the prompt's block geometry —
`textarea.set_block(...)` and the ghost overlay's `block.inner(chunks[4])` both derive from the
same `Block` value in the same call to `render()`, rather than two independently-written
`Block::borders(ALL).padding(Padding::horizontal(1))` literals that a later edit to one (a
conditional border, an added title) could silently leave the other out of sync with. An earlier
draft of this spec proposed a standalone `prompt_inner_rect` helper reproducing the geometry
independently; a design critique correctly flagged that as two sources of truth with nothing
forcing them to agree, so this revision derives both uses from one shared value instead.

`Buffer::set_stringn` (not `set_string`) is used because it takes an explicit max-width and clips
rather than panicking or overflowing the buffer if the ghost text would run past the block's right
border — the safety property that matters here, since the ghost text's length is unbounded (a
command name) and the available width shrinks as the user types.

This is the "new seam between the palette screen and the render layer" the #96 spec anticipated:
`ui.rs` asks the top screen a question ("what's the ghost completion right now?") through the same
kind of default-`None` optional method `tips()` already establishes, rather than `ui.rs`
special-casing `PaletteScreen` by downcasting or matching on `id()`. Any future screen (a rewritten
`connect` picker, say) can opt in the same way.

### Why not put the ghost text in the textarea's buffer

Considered and rejected, per the #96 spec's own reasoning (restated here because it is the load
-bearing reason this is a render-time overlay and not a buffer edit): `app.input_textarea` is a
`tui_textarea::TextArea` whose buffer is the user's real, editable, submittable text
(`app.rs:1963-1974`, `PrefillInput`'s handler). Any text appended to that buffer becomes real
content — deletable with one more backspace than the user expects, and submittable on `Enter`
if the palette's own `on_key` didn't intercept it first. Ghost text must be visually present but
functionally inert, which is exactly what a post-hoc buffer overwrite in the frame buffer gives and
a textarea edit does not.

## Scope

**In:**
- `crates/otto-plugin/src/screen.rs` — new `Screen::ghost_completion` default method.
- `crates/otto/src/plugin/builtin/command_palette/screen.rs` — `PaletteScreen` implements it.
- `crates/otto/src/ui.rs` — the render-time overlay in `render()`, plus the small
  `prompt_inner_rect` helper.
- `CHANGELOG.md` — an `Added` entry.
- Tests: unit tests for `PaletteScreen::ghost_completion` (prefix match, substring-only match,
  exact match, empty filter, no highlight); a `ui.rs` render test asserting the ghost text appears
  at the expected cell(s) for a representative case, and that it does *not* appear when no screen is
  open or the top screen returns `None`.

**Out:**
- Multi-line / wrapped prompt text. The palette always drives the prompt with a single line (`/` +
  append-only filter characters, no newlines — `PaletteScreen::on_key`'s `Char`/`Backspace` arms).
  The overlay's `line_fits` check (Approach > 3) proves the single logical line has not soft-wrapped
  onto a second visual row — checking `lines().len() == 1` alone is not sufficient, since
  `WrapMode::WordOrGlyph` wrapping is purely a rendering concern that leaves both `lines().len()`
  and `textarea.cursor()`'s reported row unchanged (a design critique caught this: see Risks). The
  overlay no-ops rather than attempting to compute a wrapped cursor's on-screen row.
- Wide (double-width, e.g. CJK) characters in the ghost text or the typed filter. Slash command
  names are ASCII by construction (`PaletteCommand::name` comes from `SlashSpec.name`, which the
  plugin manifest format constrains to command-token syntax); this is not a currently reachable
  case, not a designed-for one.
- Any change to what `Enter`, `Esc`, `Up`/`Down`, or selection do. This spec adds a read-only,
  advisory rendering signal; `on_key` is untouched.
- Any change to `tips()` or the substring-match filtering behavior in `filtered()`. Ghost text
  narrows to prefix matches specifically so it does not have to resolve or paper over the
  substring-match ambiguity `tips()` already covers; both signals coexist.
- Extending `ghost_completion` to any other screen (`connect`'s provider picker, transcript picker,
  etc.). The seam is generic so a future screen can adopt it, but no other screen is changed here.

## Public-interface changes

**Additive.** `Screen::ghost_completion` is a new trait method on the plugin ABI
(`crates/otto-plugin`) with a default implementation (`None`) — every existing native `impl Screen
for ...` block in this repo continues to compile and behave identically without any change.
(WASM plugins do not currently implement `Screen`'s `render`/`on_key`/`tips` surface across the
WASM boundary at all — `crates/otto-plugin-wasm` only has them declare a `ScreenSpec` id and
request `Effect::OpenScreen` — so there is nothing on that side for this addition to break either;
the claim above is scoped to native implementers because that's the only surface that exists
today.) No SPP wire format, tool schema, `ProviderHandler`/`ProviderClient` method, slash-command
name, env var, or on-disk transcript/keyring format changes.

Per the versioning convention (`CHANGELOG.md` header; pre-1.0 `0.MINOR.PATCH`), this is a **feature
addition to the plugin ABI**, which warrants a MINOR bump, not a PATCH — see the implementation
plan's Release line.

## Premise corrections

None. The issue's own body already corrects its context (clarifying it is "not a regression — a
known trade #96 accepted") and points at the exact design already sketched in the #96 spec; this
document formalizes and slightly narrows that sketch (the prefix-only rule in step 2 above), rather
than overturning any prior premise.

## Assumptions

- **This is worth doing now, not left as a filed-but-unbuilt follow-up.** The issue explicitly
  hedges ("worth doing when the typed-vs-dispatched mismatch actually confuses people in practice;
  `tips()` may already be enough mitigation") and offers no evidence of reported confusion. This
  spec proceeds anyway, because (a) the job was queued as ready work, (b) the design was already
  fully sketched by the #96 spec with an identified concrete seam, and (c) the change is additive,
  small, and low-risk (Non-Negotiable Rule 6 needs no special gate for it). If this judgment is
  wrong, the fix is trivial: `tips()` already covers the disambiguation need on its own, so
  reverting this change loses a convenience, not a correctness backstop.
- **Ghost text is prefix-only, not "the highlighted row's completion" unconditionally.** The issue's
  prose example (`/co▏nnect`) is itself a prefix case. Rendering ghost text for a substring-only
  match (`/cl` → ghost `acl-export`, which does not visually continue `/cl`) would look like a
  rendering bug, not a prediction — worse than the status quo of relying on `tips()` alone for that
  case. This narrowing keeps ghost text honest: what it shows always reads as a valid, literal
  completion of what is on the prompt line.
- **Single-line-only, with an explicit no-op fallback rather than wrap-aware cursor math.** The
  palette's prompt content is always `/` + filter with no newlines, so this is not a functional
  restriction in practice today; it is a defensive scope boundary against a future palette change
  (or a future screen adopting `ghost_completion`) that could make the prompt multi-line, in which
  case ghost text silently stops rendering rather than painting at a wrong row.
- **Styling matches the codebase's existing "muted" convention** (`palette.base_style().fg(palette
  .muted)`), not a separate `Modifier::DIM`. `rg 'Modifier::DIM'` in `crates/otto/src` returns no
  hits — every other "de-emphasized" text in this codebase (the `no-matches`/`no-commands` lines,
  scroll hints, footer separators) uses `fg(palette.muted)` alone. Ghost text follows the same
  convention rather than introducing a new one for a single call site.

## Goal & Success Criteria

While the command palette is open and the highlighted row's name starts with the typed filter, the
prompt shows the typed text followed by the remainder of that name rendered as dim, non-editable
"ghost" text — giving the prompt back its predictive signal from before #96, without reintroducing
#96's defect (the prompt's *real*, submittable content never includes anything the user didn't
type).

- [ ] Opening the palette with an empty filter and a highlighted row shows that row's full name as
      ghost text after the cursor.
- [ ] Typing `c`, then `o` toward `/connect` shows `/co` as real (cursor-editable) text with `nnect`
      rendered as dim ghost text immediately after it.
- [ ] Typing a filter that only substring-matches the highlighted row (`/cl` highlighting
      `acl-export`) shows no ghost text — `tips()` remains the only disambiguation signal for that
      case, unchanged from today.
- [ ] Typing a filter that exactly equals the highlighted row's full name shows no ghost text (empty
      suffix).
- [ ] `Up`/`Down` update the ghost text to match the newly highlighted row, without touching the
      prompt's real (submittable) content — consistent with #96, where navigation already emits no
      `PrefillInput`.
- [ ] Backspacing removes one real character and the ghost text recomputes against the shorter
      filter (which may now highlight a different row, per `filtered()`'s cursor-reset-to-0 rule).
- [ ] `Enter`, `Esc`, and selection behavior are pixel-for-pixel unchanged from pre-#118 behavior —
      ghost text is never part of what gets submitted, prefilled, or dispatched.
- [ ] No other screen's rendering changes: a palette not open, or any other screen on top of the
      stack, renders the prompt exactly as before.

## Error Handling & Edge Cases

- **No highlighted row** (`filtered()` empty). `ghost_completion` returns `None` via the `?` on
  `filtered.get(self.cursor)`; no overlay draws. Matches the existing no-match empty state from
  #96 (the sheet shows `no-matches`).
- **Ghost text longer than the remaining prompt width.** `set_stringn`'s explicit `max_width` clips
  it at the block's right border; no panic, no overflow into the border cell. A clipped ghost hint
  is still informative (it shows the *start* of the remaining characters), so clipping rather than
  suppressing entirely is the right degrade.
- **Cursor not at column matching `.chars().count()` of the prompt text.** Cannot currently happen —
  `PrefillInput`'s handler always does `move_cursor(CursorMove::Jump(row, col))` to the end of the
  freshly-set text (`app.rs:1963-1974`) — but the overlay reads the textarea's actual reported
  cursor position (`textarea.cursor()`) rather than recomputing it from `self.filter.len()`, so it
  stays correct even if that invariant ever changes.
- **A screen other than the palette is on top and does not override `ghost_completion`.** Returns
  `None` via the trait default; no overlay draws, and no other rendering changes — this is the same
  "additive, default-off" shape as `tips()`.

## Risks & Open Questions

- **Resolved during spec critique: the original wrap guard didn't detect wrapping.** An earlier
  draft guarded only on `cursor_row == 0 && textarea.lines().len() == 1`. Both are *logical*
  line/row properties; `WrapMode::WordOrGlyph` soft-wrapping a long logical line across multiple
  *visual* rows changes neither — so that guard would have passed identically whether the single
  logical line rendered on one visual row or several, and painted the ghost overlay at the wrong
  screen position the moment a palette-driven prompt (or a future `ghost_completion`-adopting
  screen with a longer, space-containing line) actually wrapped. The revised `line_fits` check
  (Approach > 3) closes this by proving the logical line's char count fits within the interior
  width — the actual condition under which `WordOrGlyph` cannot have wrapped it — rather than
  inferring non-wrapping from properties that don't imply it. No functional gap remains for the
  palette's own prompt shape (single line, no spaces, append-only); a future multi-line adopter is
  still out of scope (see Scope > Out) but now fails safe for the right reason.
- **The two block-geometry call sites are now unified, not merely documented as coupled.** An
  earlier draft had `prompt_inner_rect` reproduce the textarea's `Block` construction
  independently; the revision in Approach > 3 derives both `textarea.set_block(...)` and the
  overlay's `block.inner(chunks[4])` from one `prompt_block(&palette)` value per `render()` call, so
  there is no second literal that could drift out of sync. Still worth the architecture reviewer's
  attention: it's the one place this change reads two pieces of per-frame state
  (`textarea.cursor()` and `top_screen.ghost_completion()`) and assumes they describe the same
  frame's prompt content — true by construction since both are read within the same synchronous
  `render()` call, but worth an explicit second look given it's new plumbing.
- **This is new visual behavior with no dedicated visual regression test** — `cargo test` proves the
  cell(s) `set_stringn` wrote land where expected, not that the ghost text is visually distinguishable
  from real prompt text on every themed palette. The manual terminal check in the implementation
  plan's final task covers this; it is the same "green tests are not the same as work-done" gap the
  otto-development skill calls out for any TUI change.
- **Judgment call on doing this now at all** — see Assumptions. If it turns out `tips()` was already
  sufficient and this reads as visual noise in practice, the fix is a follow-up removing the overlay
  call in `ui.rs` and the trait method's only caller; the trait method itself can stay (harmless
  dead default) or be removed in the same pass.

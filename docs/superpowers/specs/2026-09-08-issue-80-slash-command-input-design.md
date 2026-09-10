# Slash command prompt preview — design

Date: 2026-09-08
Status: IMPLEMENTED
Related: `savvagent/otto#80`

## Problem

Pressing `/` from the home view opens the inline command palette, but the prompt input itself stays empty while the palette owns all subsequent key handling. The current behavior comes from two places:

- The command-palette plugin binds `/` on the home screen to `Effect::OpenScreen { id: "palette" }`. In the TUI path that binding is routed before the textarea without checking whether the prompt is empty, and the palette open path did not seed the prompt when the screen opened (`crates/otto/src/plugin/builtin/command_palette/mod.rs:34-58`, `crates/otto/src/main.rs:4047-4065`, `crates/otto/src/plugin/effects.rs:661-680`).
- The palette screen keeps its own `filter`/`cursor` state and renders `> {filter}` plus the highlighted rows, but its `on_key` handler only mutates the screen-local state or dispatches the chosen slash (`crates/otto/src/plugin/builtin/command_palette/screen.rs:35-63`, `:181-247`).

That leaves the inline list working, but the prompt gives no visible confirmation of which slash command is currently selected. Issue #80's acceptance criteria require both surfaces at once: the inline list must continue to work, and the prompt input must show `/` plus the currently highlighted command.

## Approach

Keep the palette interaction model exactly as-is and add a prompt-preview side effect that mirrors the palette's current selection into `App::input_textarea` via the existing `Effect::PrefillInput` / `App::prefill_input` path (`crates/otto/src/plugin/effects.rs:214`, `crates/otto/src/app.rs:1966-1984`).

### 1. Teach the palette screen how to derive its prompt preview

Add a small helper on `PaletteScreen` that returns the text the prompt should show while the palette is open:

- When the filtered list has a highlighted row: `/<highlighted command name>`.
- When the filtered list is empty but the user has typed a filter: `/<filter>`.
- When the filter is empty and there are no commands: `/`.

This keeps the prompt aligned with the highlighted row when a match exists, while still reflecting what the user typed when there is no current highlight.

### 2. Emit prompt-preview updates from palette key handling

After every palette state transition that can change the highlighted command or the visible filter, return `Effect::PrefillInput` with the new preview text in addition to the existing behavior:

- `Char`, `Backspace`, `Up`, `Down` update the preview in-place.
- `Enter` on an argument-taking command continues to prefill `"/<name> "` and close the screen.
- `Enter` on a no-argument command continues to execute immediately, but first clears the prompt so the preview text does not remain behind after the slash runs.
- `Esc` and the empty-results `Enter` path close the screen and clear the preview.

This preserves the palette's command execution semantics while making the prompt a live mirror of the current selection.

### 3. Seed the preview when the palette first opens

The first highlighted command should appear in the prompt immediately after `/` opens the palette, before any additional key press. In the palette-specific `open_screen` branch, construct the concrete `PaletteScreen`, derive its initial preview, call `app.prefill_input(...)`, and then push the screen onto the stack (`crates/otto/src/plugin/effects.rs:661-680`).

That keeps the inline list and the prompt synchronized from the first frame.

### 4. Preserve drafts and make the preview visible in both front-ends

Mirroring the palette into the prompt introduces two coupled frontend requirements:

- **TUI draft safety:** the `/` home keybinding must only open the palette when the prompt is empty. Otherwise opening and canceling the palette would overwrite an in-progress draft with the preview text and then clear it on close. The TUI should therefore match the egui rule already documented in `egui_app/view.rs`: a lone `/` at an otherwise-empty prompt opens the palette, while `/` inside an existing draft stays literal. This belongs in the `main.rs` home-keybinding routing path, not in the palette plugin itself.
- **egui visibility:** the egui prompt cannot stay hidden for bottom-sheet screens, because the prompt preview would then update off-screen. The egui shell should keep painting the prompt while a bottom-sheet palette is open, but make that prompt non-interactive so the palette still owns input. Centered/fullscreen screens should continue to hide the prompt.

## Scope

**In:**
- `crates/otto/src/plugin/builtin/command_palette/screen.rs` — add prompt-preview derivation and return `PrefillInput`/clear effects alongside the existing palette key actions.
- `crates/otto/src/plugin/effects.rs` — seed the prompt preview when the palette screen opens.
- `crates/otto/src/main.rs` — gate the TUI `/` home-keybinding to empty prompts so the preview cannot erase an in-progress draft.
- `crates/otto/src/egui_app/view.rs` — keep the prompt visible, but non-interactive, while a bottom-sheet palette is open so the preview is actually visible in egui.
- Targeted tests in those same files covering open, navigation, filtering, clear-on-close, immediate-run behavior, TUI draft safety, and egui prompt visibility.

**Out:**
- Changing how slash commands are discovered, filtered, or dispatched.
- Changing the host/tool/provider architecture or any `RwLock` / transport behavior.
- Replacing the inline palette with a different UI.
- Any change to slash-command syntax, README command docs, or persisted transcript/keyring formats.

## Public-interface changes

None. This is an internal TUI/GUI behavior change using existing plugin effects. It does not change the SPP wire format, any tool schema, the plugin ABI surface, slash-command names, env vars, or on-disk formats.

## Premise corrections

- The current palette is plugin-driven, not the old legacy `App::commands` / `palette_filter` state. The issue should therefore be fixed in the plugin command-palette flow (`command_palette/screen.rs` + `plugin/effects.rs`), not in the legacy helper methods in `app.rs`.
- This spec is committed in the issue worktree on branch `otto/issue-80-slash-command-input`; the repo-relative path above is the durable reference the plan and PR will cite.

## Assumptions

- ~~Showing the highlighted command name in the prompt means showing the full slash command path (`/connect`, `/clear`, etc.), not just the raw typed filter, because the acceptance criteria explicitly mention the currently highlighted command.~~ **Superseded by `savvagent/otto#96`.** This assumption was correct about #80's acceptance criteria — they did explicitly ask for the highlighted command — but the behavior it produced was reversed a day after shipping: the prompt now echoes the typed filter, and the resolved command reaches it only on selection. See `docs/superpowers/specs/2026-09-09-issue-96-palette-prompt-echoes-filter-design.md`. The rest of this spec still describes what #92 shipped and stays IMPLEMENTED.
- Clearing the prompt when the palette closes without selecting an argument-taking command is the least surprising behavior because today's prompt is effectively empty after the palette exits, and leaving the preview text behind would look like a staged command the user never confirmed.
- Using `Effect::PrefillInput` is preferable to new effect or trait surface area because it already updates both the in-memory textarea and the egui pending-prefill bridge.
- Matching egui's existing lone-`/` trigger rule in the TUI is preferable to storing/restoring arbitrary draft text, because it avoids a new preview-lifecycle state machine while preventing prompt data loss.

## Goal & Success Criteria

When a user opens the `/` command palette, the prompt input should live-preview the slash command currently highlighted in the inline list without regressing the existing palette behavior.

- [ ] Pressing `/` opens the inline palette and immediately shows the first highlighted slash command in the prompt input.
- [ ] Typing, backspacing, and moving the palette selection with arrow keys updates the prompt to match the currently highlighted command.
- [ ] If no command matches the typed filter, the prompt still shows `/<typed filter>` instead of going blank.
- [ ] Selecting a no-argument command still executes it immediately and does not leave stale preview text in the prompt afterward.
- [ ] Selecting an argument-taking command still closes the palette and leaves the prompt prefilled with `/<command> ` for further input.
- [ ] In the TUI, typing `/` when the prompt already contains other text does not open the palette or erase the draft; only a lone `/` at an otherwise-empty prompt triggers the palette.
- [ ] In egui, a bottom-sheet palette keeps the prompt visible so the preview can be seen, but the prompt stays non-interactive while the palette owns input.

## Error Handling & Edge Cases

- Empty command list: opening the palette still shows `/` in the prompt and the existing empty-state body in the sheet.
- No filtered match: preview falls back to `/<filter>` because there is no highlighted row to mirror.
- Arrow keys at the top/bottom of the list: preview remains stable because the cursor does not move.
- Esc / empty-result Enter: the palette closes and clears the preview so the prompt returns to its normal empty editing state.

## Risks & Open Questions

- The palette preview uses `PrefillInput`, which also updates the egui pending-prefill bridge. That is desired for parity, but tests should confirm the open/close behavior does not leave stale prompt text behind after immediate-run commands.
- The palette-specific prompt preview is now coupled to the order in which `open_screen` seeds the prompt and pushes the screen. A regression test should cover the initial open path directly, not only `PaletteScreen::on_key`.
- Effect ordering matters for prompt cleanup: tests should pin the `CloseScreen`/`PrefillInput`/`RunSlash` sequencing for `Esc`, empty-result `Enter`, and immediate-run `Enter` so stale preview text cannot regress silently.

# Status-line busy spinner — design

Date: 2026-09-05
Status: pending review
Related: `savvagent/savvagent-cli#27`

## Problem

The TUI footer currently renders the turn-state segment as static text from
`crates/savvagent/src/plugin/builtin/home_footer/mod.rs:100-115`: it shows the
localized `footer.idle` string while no turn is active and swaps to the
localized `footer.turn-working` string while a turn is active. That status is
then flattened into the footer's single ratatui line by
`crates/savvagent/src/ui.rs:301-321`.

Issue #27 asks for the working state in that status line to become an animated,
circular spinner sourced from `tui-spinner`, while preserving existing idle
behavior.

This is not a fast-path change under `savvagent-development`: it adds a new
crate dependency edge (`crates/savvagent` → `tui-spinner`), changes runtime TUI
behavior on an exercised code path, and touches more than two logical files.

## Approach

Add `tui-spinner` to the workspace after verifying version compatibility. The
workspace already pins `ratatui = "0.30"` in the root `Cargo.toml`, and
`tui-spinner 0.4.17` depends on `ratatui = "0.30"`, so the crate is compatible
without a ratatui upgrade.

Implement the change at the TUI render layer, not in the plugin ABI:

1. Keep `internal:home-footer` as the source of footer text semantics. Its
   center slot should continue to emit the existing idle/working text, so the
   GUI frontend and plugin-slot model stay stable.
2. Teach the ratatui footer renderer in `crates/savvagent/src/ui.rs` to replace
   the working-state text presentation with a spinner-backed variant only while
   a model turn is active. The busy gate should come from the TUI loop
   (`current_turn_id.is_some()` in `crates/savvagent/src/main.rs`), matching
   the `TurnStart`/`TurnEnd` semantics that `internal:home-footer` already uses
   for its text. This preserves the existing slot architecture (`StyledLine`
   plugin output → ratatui conversion), keeps `/bash`-only activity on the idle
   footer path, and avoids pushing ratatui widget types into
   `savvagent-plugin`.
3. Drive the spinner with a monotonic render tick passed from
   `crates/savvagent/src/main.rs`. The TUI already redraws once per loop before
   `event::poll(Duration::from_millis(50))`
   (`crates/savvagent/src/main.rs:2882-2884`, `3064-3066`), so the spinner can
   animate without introducing blocking work, new async state, or lock changes.
4. Use a radius-1 `tui_spinner::CircleSpinner`, which renders as a one-row,
   two-column braille ring that fits the existing single-line footer. Style the
   arc with the footer accent color and the dim ring with the footer muted
   color so it matches current theme conventions.
5. Preserve idle behavior exactly: when no turn is active / the app is not
   loading, render the existing footer line unchanged.

## Scope

**In:**
- TUI footer/status-line rendering in `crates/savvagent/src/ui.rs`
- TUI draw-loop tick plumbing in `crates/savvagent/src/main.rs`
- Workspace and crate manifest updates needed to add `tui-spinner`
- Tests covering the busy/idle footer rendering behavior
- Changelog/spec/plan/PR/release records for the shipped visual fix

**Out:**
- Any change to the host turn loop in `savvagent-host`
- Any change to the provider transport split or host-swap locking rules
- Any change to plugin ABI, slash-command surface, tool schemas, env vars, or
  on-disk transcript/keyring formats
- Reworking the egui footer into an animated spinner (the issue targets the
  ratatui status line, and `tui-spinner` is a ratatui widget crate)
- Broader redraw-loop optimization (dirty-flag/event-stream refactors remain a
  separate concern)

## Public-interface changes

None. This is a visual TUI behavior change only.

- **SPP wire format:** unchanged
- **Tool MCP schemas:** unchanged
- **Plugin ABI:** unchanged
- **Slash commands / env vars / on-disk formats:** unchanged

The new dependency is internal to the `savvagent` crate and does not create a
new user-facing or agent-facing interface.

## Premise corrections

- The issue title says to swap the status line's `idle` / working state for a
  circular spinner, but the existing implementation does not have a spinner at
  all; it is static localized text produced by `internal:home-footer`.
- The most maintainable integration point is `ui.rs`, not the footer plugin,
  because plugin slots intentionally traffic in `StyledLine` text rather than
  ratatui widgets.
- The circular spinner does fit the current one-line footer: `CircleSpinner`
  with `radius(1)` renders to a single text row, so no footer layout expansion
  is required.

## Assumptions

- **The spinner should appear only in the ratatui TUI, not the egui frontend.**
  Rationale: the requested crate is ratatui-specific and the issue explicitly
  names the TUI widget crate.
- **The working label should remain alongside the spinner rather than being
  replaced by a glyph-only indicator.** Rationale: this preserves the existing
  turn-id/status text, improves accessibility, and still satisfies the request
  to swap the busy-state indicator to a spinner-backed presentation.
- **The spinner should track active model turns, not every `is_loading` case.**
  Rationale: the existing footer state comes from `TurnStart`/`TurnEnd`; `/bash`
  direct execution can set `is_loading` without making the footer's center text non-idle, so the spinner gate should match `current_turn_id` /
  footer turn semantics rather than all loading states.
- **The unconditional 50 ms ratatui redraw loop is sufficient animation cadence
  for this change.** Rationale: it already exists; this task should reuse it,
  not redesign it.
- **This ships as a PATCH release.** Rationale: per `CHANGELOG.md`, pre-1.0
  PATCH covers fixes; this is a UI/status-line fix with no interface expansion
  or breaking behavior.

## Goal & Success Criteria

Replace the TUI footer's busy-state text-only presentation with a themed,
animated circular spinner from `tui-spinner`, while leaving the idle footer
unchanged and preserving existing architecture boundaries.

- [ ] `crates/savvagent` depends on `tui-spinner`, with ratatui-version
      compatibility documented by the spec/plan.
- [ ] While a model turn is active, the ratatui footer renders a circular
      spinner in the center status segment using the current theme's accent and
      muted colors.
- [ ] While idle, the footer still renders the existing localized `footer.idle`
      text with no spinner.
- [ ] `cargo build --workspace --all-targets`, `cargo test --workspace`,
      `cargo clippy --workspace --all-targets`, and `cargo fmt --all --check`
      pass.
- [ ] `CHANGELOG.md` documents the fix under `[Unreleased]` for the follow-on
      release PR.

## Error Handling & Edge Cases

- If the footer center slot is empty for any reason, the TUI should not invent
  new text; it should render the remaining footer groups exactly as today.
- Non-turn busy states (for example a direct `/bash` invocation that toggles
  `is_loading` without a `TurnStart`) should stay on the idle footer path; this
  change is scoped to the model-turn working indicator.
- If the spinner output ever resolves to an empty line (unexpected for
  `CircleSpinner::radius(1)`), the footer should fall back to the existing text
  path rather than rendering a blank busy segment.
- Small terminals must continue to degrade safely. The busy footer remains a
  single-line render and must not assume extra height.
- The draw-loop tick plumbing must stay synchronous and allocation-light; no
  `.await` belongs in `ui::render` or inside the draw closure.

## Risks & Open Questions

- The TUI currently redraws unconditionally every loop iteration. Reusing that
  behavior makes the spinner easy to animate, but it means this change should
  avoid introducing expensive per-frame allocations beyond the tiny footer
  spinner/text composition.
- Runtime verification matters more than usual for this change because the core
  value is visible animation rather than pure data transformation; Phase 5
  should include a manual `cargo run -p savvagent` check if the environment can
  launch the TUI.
- Because the footer data model is shared with egui, the implementation must
  keep the spinner injection TUI-local; otherwise it would leak ratatui-specific
  concerns into the shared render model.
- `tui-spinner` is a third-party dependency. Compatibility is verified now
  against ratatui 0.30, but any future ratatui bump will need the usual crate
  compatibility check.

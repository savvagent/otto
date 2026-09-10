# issue-117-styled-line-constructors Implementation Plan

**Goal:** Give `otto-plugin`'s styled-text types the constructors their callers keep hand-rolling — `StyledSpan::{plain, colored, muted}` and `StyledLine::{colored, muted}` — then rewrite the 51 hand-built literals and delete the six file-local reinventions of the same function, per `savvagent/otto#117`.

**Architecture:** Purely additive to the plugin ABI. `StyledLine`/`StyledSpan` keep their public fields and their struct-literal form; the constructors are a shorthand for the one shape that dominates (`bg: None`, `modifiers: TextMods::default()`). `StyledLine::colored` delegates to `StyledSpan::colored`, `muted` delegates to `colored`, and `StyledLine::plain` is re-expressed through `StyledSpan::plain` without changing its signature or output. The WIT surface is untouched — `styled-span`/`styled-line` are data records with no associated functions.

**Tech Stack:** Existing workspace crates only (`otto-plugin`, `otto`). No new dependencies, no new files outside `docs/`.

**Spec:** `docs/superpowers/specs/2026-09-10-issue-117-styled-line-constructors-design.md` — read it first, including "The pattern has already been reinvented six times" and "How criterion 6 is actually enforced". This plan implements it exactly.

**Commit discipline:** The rewrite is only safe if the colors are pinned first. Task 2 lands the color-pinning tests **against unmodified production code** — they must pass on the pre-rewrite tree, which is what makes them evidence rather than a rubber stamp. Tasks 3 and 4 must not edit an assertion in a test written by Task 2; if one fails, the rewrite is wrong, not the test. Every task ends with `cargo test --workspace` green.

**Release line:** v0.28.0 (MINOR — new public API on the plugin ABI, per the repo's pre-1.0 SemVer convention). No release PR is open at the time of writing (`gh pr list --state open` is empty), so this is the next release off `main`.

**Branch:** `plugin/styled-line-constructors`

## File Map

**New files**
- `docs/superpowers/specs/2026-09-10-issue-117-styled-line-constructors-design.md` — the design source of truth.
- `docs/superpowers/plans/2026-09-10-issue-117-styled-line-constructors.md` — this plan.

**Modified files**
- `crates/otto-plugin/src/styled.rs` — five new constructors, `StyledLine::plain` re-expressed, `push` helper in `json_spans` folded into `StyledSpan::colored`, new unit tests.
- Shape A rewrite (single-span `StyledLine` literals), under `crates/otto/src/`:
  `plugin/builtin/command_palette/screen.rs` (3), `plugin/builtin/themes/screen.rs` (3), `plugin/builtin/plugins_manager/screen.rs` (2), `plugin/builtin/connect/screen.rs`, `plugin/builtin/language/screen.rs`, `plugin/builtin/model/screen.rs`, `plugin/builtin/migration_picker/screen.rs`, `plugin/builtin/resume/screen.rs`, `plugin/builtin/user_slash_commands/trust_modal.rs`, `plugin/builtin/home_tips/mod.rs`, `plugin/builtin/home_footer/mod.rs`, `plugin/builtin/self_update/mod.rs` (2), `plugin/builtin/changelog/screen.rs`, `plugin/convert.rs`, `ui.rs` (5, four of them test fixtures).
- Shape B rewrite (spans with siblings) — same tree:
  `plugin/builtin/connect/screen.rs` (4), `plugin/builtin/home_footer/mod.rs` (2, the local closures), `plugin/builtin/plugins_manager/screen.rs` (2), `plugin/builtin/command_palette/screen.rs`, `plugin/builtin/keybindings_view.rs`, `plugin/builtin/language/screen.rs`, `plugin/builtin/model/screen.rs`, `plugin/builtin/themes/screen.rs`, `plugin/builtin/mcp/screen.rs`, `plugin/tool_summaries.rs` (2), `ui.rs` (3).
- Local-reinvention deletions: `plugin/builtin/tool_bash_summary/mod.rs`, `tool_fs_summary/mod.rs`, `tool_grep_summary/mod.rs`, `tool_task_summary/mod.rs`, `tool_web_summary/mod.rs` — each drops its private `fn span` and points its call sites at `StyledSpan::colored`.
- `CHANGELOG.md` — an `Added` entry referencing #117.
- `Cargo.toml` — workspace `version` to `0.28.0` (release PR, Phase 4 step 12).

**Explicitly not modified:**
- `crates/otto-plugin-wit/wit/shared.wit` and `crates/otto-plugin-wasm/src/convert.rs`. The WIT records have no associated functions and the conversion layer builds both sides of the boundary explicitly; a constructor would hide which side is being built. Leave them as literals.
- The 19 literals that set a background or non-default modifiers. They keep the struct literal — see the spec's "What is deliberately not added".
- `crates/otto/src/plugin/builtin/splash/screen.rs:68` — its `fg` and `modifiers` are both variables; it is not the shape being collapsed.

## Task 1: Add the constructors

**Files:**
- Modify: `crates/otto-plugin/src/styled.rs`

- [ ] Read the existing `impl StyledLine` block (`:133-146`) and the `push` helper used by `json_spans` (`:~248`), and confirm both build the same four-field shape.
- [ ] Write the unit tests first, in `styled.rs`'s existing `mod tests`, so they fail to compile before the impl exists:
  - `styled_span_plain_leaves_fg_none` — `StyledSpan::plain("x")` has `text == "x"`, `fg == None`, `bg == None`, `modifiers == TextMods::default()`.
  - `styled_span_colored_sets_only_fg` — `StyledSpan::colored("x", ThemeColor::Warning)` has `fg == Some(ThemeColor::Warning)`, `bg == None`, default modifiers.
  - `styled_span_muted_is_colored_with_muted` — asserts the two are `==`, which is what licenses `muted` being a second name for the same thing.
  - `styled_line_constructors_wrap_a_single_span` — for each of `plain`/`muted`/`colored`, the line has exactly one span equal to the matching `StyledSpan` constructor's output.
  - `styled_line_plain_is_unchanged` — pins `StyledLine::plain("x")` against a hand-written literal, so Task 1's re-expression of its body cannot drift.
- [ ] Add `impl StyledSpan { plain, colored, muted }` above the existing `impl StyledLine`. Each sets `bg: None` and `modifiers: TextMods::default()`; `muted` is `Self::colored(text, ThemeColor::Muted)`.
- [ ] Add `colored` and `muted` to `impl StyledLine`, each wrapping the `StyledSpan` counterpart in a one-element `spans` vec. `muted` is `Self::colored(text, ThemeColor::Muted)`.
- [ ] Re-express `StyledLine::plain`'s body as `Self { spans: vec![StyledSpan::plain(text)] }`. Do not change its signature or its doc comment's meaning.
- [ ] Write `///` doc comments on all five new functions stating explicitly that they set `bg: None` and default modifiers, and pointing at the struct literal for anything else. On `muted`, say why it exists alongside `colored` (secondary text is the default case, 20 of 41 colored spans) so the next reader does not delete it as redundant.
- [ ] Fold `json_spans`' private `push` helper into `StyledSpan::colored` — it is the sixth reinvention and lives in this very file.
- [ ] Confirm no re-export change is needed: `crates/otto-plugin/src/lib.rs:33` already re-exports `StyledLine` and `StyledSpan`, and inherent methods travel with the type.
- [ ] Public-interface check: additive only. Five new associated functions on two existing public structs; no signature, field, or variant changes; no WIT change. Record this in the commit body.
- [ ] `cargo test -p otto-plugin` green, then `cargo test --workspace` green.
- [ ] Commit: `otto-plugin: add StyledSpan/StyledLine color constructors (#117)`.

## Task 2: Pin the colors before touching a call site

**Files:**
- Modify: the test modules of the screens rewritten in Tasks 3–4.

The point of this task is that it changes **no production code**. Everything it adds must pass against the tree as Task 1 leaves it.

- [ ] For each file in the File Map's rewrite lists that has a `render`/`render_slot` producing colored spans, find the existing test module and check whether any test asserts `.fg`. Record which files have coverage and which do not — the spec's "How criterion 6 is actually enforced" claims none do, so any that does is a pleasant surprise, not a reason to skip.
- [ ] Add a color-pinning test per uncovered screen, asserting the `fg` of the spans its `render` emits for the state being rewritten. Concretely, at minimum:
  - `connect/screen.rs` — the `candidates.is_empty()` line is `Warning`; the `Search: ` prefix is `Muted`; the `No providers match` pair is `Muted` + `Accent`.
  - `command_palette/screen.rs` — both empty states (`no-commands`, `no-matches`) are `Muted`.
  - `themes/screen.rs` — the `Warning` empty state and the two `Muted` lines.
  - `language/screen.rs`, `model/screen.rs`, `plugins_manager/screen.rs`, `resume/screen.rs`, `migration_picker/screen.rs`, `user_slash_commands/trust_modal.rs`, `home_tips/mod.rs` — the empty-state / hint line's color.
  - `home_footer/mod.rs` — `home.footer.center` is `Accent`; the `home.footer.right` row is `Muted` labels with an `Accent` version.
  - `tool_bash_summary`, `tool_fs_summary`, `tool_grep_summary`, `tool_task_summary`, `tool_web_summary` — at least one summary per plugin, asserting the `fg` of every span it emits. These five are the highest-risk rewrite (a whole-file `span(` → `StyledSpan::colored(` substitution) and the spec calls them out by name.
- [ ] Where a screen's existing test already builds the state, extend it with `fg` assertions rather than duplicating the setup.
- [ ] Run `cargo test --workspace`. **Every new test must pass now**, before any rewrite. A test that needs the rewrite to pass is testing the wrong thing — rewrite it.
- [ ] Commit: `otto: pin span colors in builtin screen tests ahead of the #117 rewrite (#117)`.

## Task 3: Rewrite the whole-line literals (shape A)

**Files:**
- Modify: the 26 shape-A sites listed in the File Map.

- [ ] Work file by file. For each literal: `fg: None` → `StyledLine::plain(text)`; `fg: Some(ThemeColor::Muted)` → `StyledLine::muted(text)`; any other concrete color → `StyledLine::colored(text, <color>)`. The `text` expression is copied across verbatim — including its `.to_string()` / `.into()` — and nothing else changes.
- [ ] `ui.rs:1433` and the other `otto_plugin::`-qualified literals keep their qualification (`otto_plugin::StyledLine::colored(…)`); do not add imports to shorten them, that is a separate cleanup.
- [ ] `ui.rs`'s four footer-test fixtures (`:2166`, `:2202`, `:2228`, `:2266`) are test *input*, not assertions. Rewriting them is safe and in scope; leave the assertions around them alone.
- [ ] After each file, re-read the diff hunk and confirm the color is character-identical to what it replaced. This is the "mechanical equivalence review" the spec requires.
- [ ] `cargo test --workspace` green — specifically, no test from Task 2 fails.
- [ ] Commit: `otto: build single-span styled lines through the new constructors (#117)`.

## Task 4: Rewrite the sibling spans and delete the six reinventions (shape B)

**Files:**
- Modify: the 25 shape-B sites, plus the five `tool_*_summary/mod.rs` and `home_footer/mod.rs`.

- [ ] Rewrite the plain shape-B literals the same way as Task 3, with `StyledSpan::{plain, muted, colored}`.
- [ ] `connect/screen.rs:159` has a conditional color. It becomes `StyledSpan::colored(text_expr, if self.query.is_empty() { ThemeColor::Muted } else { ThemeColor::Fg })` — the conditional moves into the argument unchanged. Same for `mcp/screen.rs:630`'s `row.state_color`.
- [ ] For each of the five `tool_*_summary/mod.rs`: delete the private `fn span`, replace every `span(x, c)` call with `StyledSpan::colored(x, c)`, and fix the now-unused `TextMods` import if the file has one. Confirm each file's remaining imports still resolve.
- [ ] `home_footer/mod.rs:120-134`: delete the local `muted` and `accent` closures and call `StyledSpan::muted` / `StyledSpan::colored(_, ThemeColor::Accent)` at their call sites.
- [ ] Re-run the brace-matching sweep from the spec and confirm what remains is only the 19 background/modifier literals, `styled.rs`'s own constructor bodies, and the deliberately-excluded WIT conversion layer.
- [ ] `cargo test --workspace` green — again, with no Task 2 assertion edited.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean (a deleted helper often leaves an unused import behind; this is where that surfaces).
- [ ] Commit: `otto: build styled spans through the new constructors and drop six local copies (#117)`.

## Task 5: Docs and close-out

**Files:**
- Modify: `CHANGELOG.md`, the spec's `Status:` line, this plan's checkboxes.

- [ ] Add a `### Added` entry under `## [Unreleased]` describing the five constructors as plugin-ABI surface third-party plugins can use, and noting that the builtins now go through them. Do not describe it as a user-visible change — nothing renders differently.
- [ ] Flip the spec's `Status: DRAFT` to `Status: IMPLEMENTED`.
- [ ] Tick this plan's checkboxes in place.
- [ ] `cargo build && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` all clean one final time.
- [ ] Commit: `docs: record #117 styled-text constructors as shipped (#117)`.

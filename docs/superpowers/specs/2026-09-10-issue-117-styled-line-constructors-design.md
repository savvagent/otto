# Styled-text constructors in `otto-plugin` — design

Date: 2026-09-10
Status: IMPLEMENTED
Related: `savvagent/otto#117`. Noted during review of `savvagent/otto#96` (shipped as PR #114).

## Problem

`otto-plugin`'s styled-text types are the currency of every screen and slot in the TUI: `Plugin::render_slot` and `Screen::render` both return `Vec<StyledLine>`, and a `StyledLine` is a `Vec<StyledSpan>` where each span carries `text`, `fg`, `bg` and `modifiers` (`crates/otto-plugin/src/styled.rs:6-24`).

The module ships exactly one constructor — `StyledLine::plain(text)` (`styled.rs:133-146`) — which builds a one-span line with no colors and default modifiers. Anything that is not plain is hand-built as a struct literal. The overwhelmingly common case is *one span, a semantic foreground color, no background, no modifiers*, and it costs seven lines every time:

```rust
StyledLine {
    spans: vec![StyledSpan {
        text: rust_i18n::t!("picker.command-palette.no-matches").to_string(),
        fg: Some(ThemeColor::Muted),
        bg: None,
        modifiers: TextMods::default(),
    }],
}
```

`ThemeColor::Muted` appears 60 times across the workspace, 41 of them under `crates/otto/src/plugin/builtin/`. That number counts *spans*, not lines, which matters for scoping: it is not one repeated shape but two.

### The two shapes, counted

Measured against `origin/main` at `e72d355` by a brace-matching sweep of every `StyledSpan { … }` and `StyledLine { … }` literal under `crates/otto/src/` and `crates/otto-plugin/src/`, keeping only literals with `bg: None` **and** `modifiers: TextMods::default()` (a regex sweep undercounts here — it misses `otto_plugin::`-qualified literals and multi-line field values, and it cannot tell a solo span from a span with siblings):

| Shape | Count | Collapsed by |
|---|---|---|
| **A** — whole line: `StyledLine { spans: vec![StyledSpan { … }] }`, exactly one span | 26 | a `StyledLine` constructor |
| **B** — a `StyledSpan { … }` that is not the sole span of a `StyledLine` literal | 25 | a `StyledSpan` constructor |

Of those 51: 4 have `fg: None` (they want `plain`), 6 are the bodies of local helper functions that take an `fg` parameter (see below), and **41 name a concrete color** — of which **20 are `Muted`**, 8 `Warning`, 9 `Accent`, and one each `Green`, `Fg`, `row.state_color`, and a `Muted`/`Fg` conditional.

A further 19 `StyledSpan` literals set a background or non-default modifiers. They are out of scope and keep the struct literal.

Shape A is concentrated in picker empty states — `command_palette/screen.rs` (3), `themes/screen.rs` (3), `plugins_manager/screen.rs` (2), `connect/screen.rs`, `language/screen.rs`, `model/screen.rs`, `migration_picker/screen.rs`, `resume/screen.rs`, `user_slash_commands/trust_modal.rs`, `home_tips/mod.rs`, `home_footer/mod.rs`, `self_update/mod.rs` — plus `plugin/convert.rs` and five in `ui.rs` (four of them test fixtures).

Shape B is concentrated in label+value rows: `connect/screen.rs`'s `Search: ` prefix and its `No providers match <query>` line, `keybindings_view.rs`, the `themes`/`language`/`model`/`plugins_manager`/`mcp` footers, `plugin/tool_summaries.rs`, and `ui.rs`'s footer separator.

### The pattern has already been reinvented eight times

Eight call sites do not hand-build the literal — they define a **private, file-local copy of the constructor this spec proposes**:

- `tool_bash_summary/mod.rs:31`, `tool_fs_summary/mod.rs:34`, `tool_grep_summary/mod.rs:28`, `tool_task_summary/mod.rs:41`, `tool_web_summary/mod.rs:28` each define, character-for-character, `fn span(text: impl Into<String>, fg: ThemeColor) -> StyledSpan`.
- `home_footer/mod.rs:120-134` defines local `muted` and `accent` closures that are exactly `StyledSpan::muted` and `StyledSpan::colored(_, Accent)`.

- `styled.rs`'s own `json_spans` helper `fn push(out, text, fg)` — the constructor reinvented inside the very module that should have exported it.

Five identical copies of a two-line function in five sibling plugins is the strongest evidence available that the constructor belongs upstream in `otto-plugin`. This is not a hypothesis about what callers would want; it is what five callers already wrote.

> **Found during implementation:** two more, bringing the total to eight. `self_update/mod.rs:139`'s `fn note_effect(text)` wraps `StyledLine::plain` inside an `Effect::PushNote`, and its doc comment states the motive outright — *"Centralised so each call site doesn't repeat the `StyledSpan` scaffolding."* `ui.rs`'s test module defines its own `fn span(text: &str)`. Both are folded into the new constructors; `note_effect` survives as a one-liner because the `Effect::PushNote` wrapper is still worth a name.

### Why this is worth changing

The cost is not keystrokes — it is that the noise hides the signal. In the `connect` empty state the only thing that differs between adjacent literals is one enum variant, and it takes six lines of identical boilerplate to say so. Reviewers of #96/#114 hit exactly this: adding one more empty-state block made the shape conspicuous.

There is a second-order cost. Because there is no constructor to reach for, `bg` and `modifiers` are re-spelled by hand at every site, so `TextMods::default()` is load-bearing punctuation rather than a stated intent. A site that *does* want a modifier looks identical at a glance to one that does not.

## Approach

Add constructors for both shapes, as siblings of `plain`, in `crates/otto-plugin/src/styled.rs`:

```rust
impl StyledSpan {
    pub fn plain(text: impl Into<String>) -> Self;
    pub fn colored(text: impl Into<String>, fg: ThemeColor) -> Self;
    pub fn muted(text: impl Into<String>) -> Self;
}

impl StyledLine {
    pub fn plain(text: impl Into<String>) -> Self;   // unchanged signature
    pub fn colored(text: impl Into<String>, fg: ThemeColor) -> Self;
    pub fn muted(text: impl Into<String>) -> Self;
}
```

All six set `bg: None` and `modifiers: TextMods::default()`. `muted` is `colored(text, ThemeColor::Muted)`; each `StyledLine` constructor is a one-span line wrapping its `StyledSpan` counterpart; `StyledLine::plain`'s body becomes `Self { spans: vec![StyledSpan::plain(text)] }`, which is field-for-field what it builds today, keeping its signature and behavior unchanged.

Then rewrite the 51 measured call sites and delete the local reinventions.

### Why `muted` earns a name of its own when `colored` already covers it

`StyledLine::muted(t)` is exactly `StyledLine::colored(t, ThemeColor::Muted)`, so it is redundant by construction. It is worth the ABI surface because Muted is not one color among several here — it is **20 of the 41** concretely-colored spans in the tree, and it is the *default* for secondary text. The named form states the intent ("this is secondary text") where the general form states a mechanism ("this is the Muted slot"). The same argument does not carry for `Warning` (8) / `Accent` (9): those are chosen per-site against a real alternative, so `colored(t, ThemeColor::Warning)` reads correctly and a `warning()` sibling would just be a second spelling.

This is the same asymmetry the issue proposes, and it is the reason the proposal is two functions rather than one or eight.

### Why span constructors are in scope

The issue names only the `StyledLine` pair. Taken literally that collapses shape A and none of shape B — leaving the file the issue points at (`connect/screen.rs`) still carrying hand-built Muted spans directly beneath the ones it collapsed, and leaving all five `fn span` reinventions in place. The issue's own justification ("`muted` alone collapses most of the 41") is only true on the span reading of that number, since 41 counts spans. The span constructors are what makes the stated goal true, they are the same additive, no-signature-change decision, and `StyledLine::colored` is naturally written in terms of `StyledSpan::colored` anyway. They are in scope.

### What is deliberately not added

- **No modifier-carrying constructors** (`bold`, `dim`, …) and no builder chain. The 19 sites that set modifiers set them conditionally or several at once (`TextMods { bold: is_cursor, ..Default::default() }`) and are better served by the struct literal, which stays available. Adding a builder now would be designing for a caller that does not exist.
- **No `bg` parameter.** Exactly one literal in the whole workspace sets a background, and it is a WASM round-trip test fixture. A constructor taking `Option<ThemeColor>` for `bg` would be worse at every call site than the literal it replaces.
- **No `From`/`Into` impls** (e.g. `impl From<&str> for StyledLine`). Inference-driven conversions make the color decision invisible at the call site, which is the opposite of what this change is for.
### Who this actually helps

Worth stating precisely, because it is easy to get wrong: **WASM plugin authors get nothing from this change.** They depend on `otto-plugin-wit`, not `otto-plugin` (see `crates/otto-plugin-wasm/tests/fixtures-src/*/Cargo.toml`), and they build the *generated* `wit::StyledSpan` — a different type whose `fg` is a `ThemeColor` with a `Reset` sentinel rather than an `Option`. `otto-plugin` is also `publish = false` (`release-plz.toml:3`), so it is not consumable from crates.io either.

The beneficiaries are `crates/otto` and any future in-tree Rust consumer of these types. The guest-side gap is real and unaddressed: `crates/otto-plugin-wasm/tests/fixtures-src/interactive/src/lib.rs:113` is a *ninth* reinvention of this helper, written against the WIT bindings, spelling out all five `TextMods` fields. If that is worth closing, the seam is a documented snippet in `docs/plugins/authoring.md` or a small guest-side helper crate — **not** `otto-plugin`, and emphatically not by promoting the WIT `record`s to `resource`s, which would be a breaking change for a cosmetic gain.

- **No change to the WIT surface.** `crates/otto-plugin-wit/wit/shared.wit:171-180` declares `styled-span` / `styled-line` as pure data records with no associated functions; constructors are native-Rust ergonomics for `otto-plugin` consumers. `otto-plugin-wasm/src/convert.rs` is untouched.

## Public-interface impact

`otto-plugin` is plugin ABI surface (Non-Negotiable Rule 6). This change is **additive**: five new associated functions, no existing signature altered, no field added or removed, no type made or unmade `#[non_exhaustive]`. `StyledLine` and `StyledSpan` are plain public structs with public fields (only `ThemeColor` is `#[non_exhaustive]`), so third-party plugins that build them as literals are unaffected, and inherent methods take precedence over trait methods in Rust's resolution rules, so a new `plain`/`muted`/`colored` cannot shadow anything a downstream crate relies on. No `fn plain|muted|colored` on these types exists anywhere in the workspace today.

Per this repo's pre-1.0 SemVer convention (MINOR = features/breaking changes, PATCH = fixes), new public API is a MINOR bump: **v0.28.0**.

## Success criteria

1. `StyledSpan::{plain, colored, muted}` and `StyledLine::{colored, muted}` exist in `crates/otto-plugin/src/styled.rs`, are reachable through `otto_plugin`'s existing root re-export of `StyledLine`/`StyledSpan`, and each carries a `///` doc comment naming what it sets.
2. `StyledLine::plain`'s signature and output are unchanged — an existing caller sees no difference.
3. All 51 measured literals are rewritten to the constructors, and the six local reinventions (`fn span` × 5, `home_footer`'s `muted`/`accent` closures) are deleted with their call sites pointed at the new constructors. A re-run of the brace-matching sweep reports only the 19 background/modifier-setting literals plus `styled.rs`'s own constructor bodies.
4. Unit tests in `styled.rs` pin each constructor's four field values, including that `muted(t) == colored(t, ThemeColor::Muted)`, that `plain` leaves `fg` as `None`, and that the `StyledLine` forms produce exactly one span equal to their `StyledSpan` counterpart.
5. `cargo build`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings` are clean.
6. No rendered output changes.

### How criterion 6 is actually enforced

The existing suite does **not** enforce it, and this spec does not pretend otherwise. Every render-adjacent test in the files being rewritten asserts on `.text` only: `connect/screen.rs`'s `short_list_hides_query_until_user_types` joins span text; `command_palette/screen.rs`'s tests assert on `prompt_preview()`; `plugin/convert.rs`'s `styled_line_to_ratatui_produces_spans` — whose own subject literal is being rewritten — checks `spans.len()` and `spans[0].content` and never `.fg`. A rewrite that swapped `Warning` for `Muted` would compile and pass all of them.

Two mechanisms close that gap, and both are required:

- **A color-pinning test per rewritten screen.** Each screen whose `render` produces colored spans gets (or extends) a test asserting the `fg` of the spans it emits, written **before** the rewrite so it passes against the current code and would fail if the rewrite changed a color. This is the only real regression net and it is where most of this change's new test lines go.
- **A mechanical equivalence review of the diff.** Every rewritten hunk must be a literal-for-constructor substitution with the same `text` expression and the same color; anything else is a behavior change and belongs in a different commit.

## Risks

- **A mechanical rewrite silently changing a color.** This is the main risk and the reason for the color-pinning tests above. Writing them first, against unmodified code, is what makes them evidence rather than a rubber stamp.
- **Deleting the five `fn span` helpers changes more than it looks.** Each is a whole-file find-and-replace of `span(x, c)` → `StyledSpan::colored(x, c)`, and each of those five plugins has existing summary tests. Those tests assert on text, so they do not pin the color — the color-pinning requirement applies to these five files too.
- **Overlap with in-flight work.** `crates/otto/src/ui.rs` and `crates/otto/src/plugin/builtin/command_palette/screen.rs` are also touched by `savvagent/otto#116`, worked concurrently. The overlap is disjoint by region — #116 changes `paint_screen`'s region arithmetic and the palette's `capacity` constant; this change touches only styled-literal construction. Conflicts, if any, are textual and local.
- **Constructor proliferation.** Named color helpers are a slope: `warning()`, `error()`, `accent()` are each one PR away. The "what is deliberately not added" section above is the standing answer — `muted` is justified by frequency (20 of 41) and by being the default for secondary text, not by symmetry, and that argument does not generalize.

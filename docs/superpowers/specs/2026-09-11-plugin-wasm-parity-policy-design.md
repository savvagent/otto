# Record a native-vs-WASM plugin ABI parity policy and enumerate the current gap — design

Date: 2026-09-11
Status: IMPLEMENTED
Source: savvagent/otto#140
Related: v0.28.0 (#117, `StyledSpan`/`StyledLine` constructors), v0.29.0 (#118,
`Screen::ghost_completion`) — the two "for now" changelog entries this issue is about. Surfaced by
the architecture pass on the v0.29.0 release PR (#135).

## Problem

Two consecutive releases shipped an `otto-plugin` addition that native Rust plugin authors can use
and WASM plugin authors cannot, each recorded in `CHANGELOG.md` with a "for now" and no tracking
issue:

- v0.28.0 (#117) — `StyledSpan::plain/colored/muted`, `StyledLine::colored/muted`
  (`crates/otto-plugin/src/styled.rs`). Changelog: "they bind against `otto-plugin-wit`, where
  `styled-span` / `styled-line` are WIT `record`s that cannot carry associated functions at all …
  Guest-side ergonomics remain an open gap."
- v0.29.0 (#118) — `Screen::ghost_completion` (`crates/otto-plugin/src/screen.rs`). Changelog: "not
  yet exposed through the WASM plugin ABI's WIT interface, so third-party WASM plugins inherit the
  default (no ghost text) for now."

Both statements are accurate: `grep -rn ghost crates/otto-plugin-wit/ crates/otto-plugin-wasm/`
returns nothing, and `impl Screen for WasmScreen`
(`crates/otto-plugin-wasm/src/adapter/interactive.rs:518-654`) does not override
`ghost_completion`. Neither gap is tracked anywhere, which is how "for now" becomes permanent.

The divergence is structural: `otto-plugin` is a plain Rust crate and can grow constructors,
defaulted trait methods, and other ergonomics freely. `otto-plugin-wit` is a WIT interface —
records cannot carry associated functions, and every new guest-callable capability needs an
explicit interface addition plus a regenerated guest binding
(`crates/otto-plugin-wasm`'s `wasmtime::component::bindgen!`-generated host side, and each guest's
own `wit_bindgen::generate!`). So the native surface drifts ahead of the WASM surface by default,
and each individual step is defensible in isolation — but CLAUDE.md's `otto-development` skill
names the plugin ABI (`otto-plugin`/`otto-plugin-wit`/`otto-plugin-wasm`) as one interface under
Non-Negotiable Rule 6. Left unaddressed, "the plugin ABI" stops being one thing, and a third-party
WASM plugin becomes second-class by accumulation rather than by decision.

Two questions, asked once here instead of repeated per-release:

1. **Is parity the goal?** If yes, each `otto-plugin` addition should land with either a WIT
   counterpart or a recorded reason it cannot have one.
2. **What is the current gap?** Nobody has enumerated it; a WASM plugin author cannot tell from the
   docs which parts of `otto-plugin` reach them.

## Approach

This is primarily a documentation/policy change with three small, separately-scoped follow-up
issues — not a WIT/runtime change in this PR. The reasoning for that scope split is itself part of
the design (see "Why the follow-ups are issues, not code" below).

### 1. Enumerate the current gap (this document + CLAUDE.md)

Surveyed every public trait/type in `crates/otto-plugin/src/*.rs` against the four `.wit` files in
`crates/otto-plugin-wit/wit/` (`shared.wit`, `plugin-static.wit`, `plugin-interactive.wit`,
`plugin-provider.wit`) and `crates/otto-plugin-wasm/src/adapter/*.rs`'s actual trait impls. Four
concrete gaps, in order of size:

| Native surface | otto-plugin-wit counterpart | Gap | Tracked |
|---|---|---|---|
| `Screen::ghost_completion` (`screen.rs:69`) | none | `screen-instance` resource (`plugin-interactive.wit`) has `on-key`/`on-event`/`render`/`tips` only. `impl Screen for WasmScreen` doesn't override it, so every WASM screen gets the native default (`None`). | #165 |
| `StyledSpan::plain/colored/muted`, `StyledLine::plain/colored/muted` (`styled.rs:134-201`) | `styled-span`/`styled-line` records exist (`shared.wit`), no constructors | WIT records cannot carry associated functions, so this was never closeable by a WIT edit. Every WASM plugin builds the struct literal by hand (`examples/plugin-hello-interactive/src/lib.rs`'s local `styled_line` helper is the same 10 lines every example/fixture re-implements). | #166 |
| `ContentRenderer` (`content.rs` — `id`/`render`/`dispatch`/`freeze`/`thaw`/`focusable_elements`/`focused_index`/`set_focus`/`snapshot_state`/`restore_state`) and `Plugin::create_renderer` (`plugin.rs:115-123`) | **none at all** | There is no `plugin-canvas.wit` (or equivalent) alongside `plugin-static.wit`/`plugin-interactive.wit`/`plugin-provider.wit`. A WASM plugin cannot register an inline content-block renderer today — not a partial gap like the other two, a whole missing capability family. | #167 |
| `Plugin::summarize_tool_call`/`summarize_tool_result` (`plugin.rs:80-106`) and `Contributions::tool_summaries: Vec<ToolSummarySpec>` (`manifest.rs:65,180`) | none | `shared.wit`'s `contributions` record has no `tool-summaries` field, and no `.wit` file exports anything shaped like `summarize-tool-call`/`summarize-tool-result`. A WASM plugin can never register for or render a tool-call/tool-result summary. | #170 |

Checked and found **not** a gap (host-side validation helpers a WASM guest never needs to call
across the ABI, because the guest never constructs these native types itself): `PluginId::new`
(`types.rs:17`), `ChordPortable::new` (`types.rs:161`), `ThemePalette::new` (`types.rs:202`),
`ScreenArgs::screen_id()` (`types.rs:347`). These operate on values the *host* constructs from data
the guest supplies as plain strings/records (e.g. a WASM plugin's manifest carries `id: string`,
not a `PluginId`); there's nothing for a WASM author to be missing here.

### 2. Record the position (CLAUDE.md)

**Parity is the goal for the plugin ABI's guest-callable behavioral surface** (trait methods a
plugin implements to receive host callbacks — `Screen`, `Plugin`, hooks) **and for constructor
ergonomics on WIT-backed value types, wherever the guest side can express them in ordinary Rust
without a WIT change.** Concretely: every `otto-plugin` addition to a surface already mirrored in
`otto-plugin-wit` lands its WIT/guest-ergonomics counterpart in the same change, or records an
issue number and the reason it doesn't (yet) — never a bare "for now." A capability family with
*no* WIT interface at all (this issue found one: `ContentRenderer`) is named explicitly rather than
silently left to keep growing on the native side alone.

This is the position CLAUDE.md gets: a new "Plugin ABI (native vs. WASM parity)" subsection under
Architecture, plus the three plugin crates added to the Workspace map table (currently absent
entirely — an existing documentation gap this task's edit happens to touch in passing, not
something to widen further).

### 3. File the two required tracking issues, plus one more the enumeration surfaced

Acceptance criteria requires `ghost_completion` and the `StyledSpan`/`StyledLine` constructors each
get "an issue or a WIT addition." Both get **issues** (not WIT/code changes) in this PR — see "Why
the follow-ups are issues, not code" below — filed against `savvagent/otto`:

- **#165** — `Screen::ghost_completion` WIT/WASM counterpart.
- **#166** — `StyledSpan`/`StyledLine` guest-side constructor ergonomics.
- **#167** — `ContentRenderer`/canvas content blocks (no WIT interface at all) — filed because the
  enumeration step surfaced it, not because the acceptance criteria named it, but the recorded
  CLAUDE.md position ("a capability family with no WIT interface gets a recorded reason") requires
  it not be left as a silent omission.

### 4. Establish the "reference an issue number" convention going forward

CLAUDE.md's new subsection states the rule directly: a future changelog entry citing a WASM gap
must cite the tracking issue number, never bare "for now." This PR's own diff does not touch
`CHANGELOG.md` — that entry is deferred to release-cut time (per Non-Negotiable Rule 8, in the
dedicated release PR — see the plan's Task 2), not added in this feature PR. When it lands there,
it will demonstrate the convention by citing #165/#166/#167/#170 rather than repeating "for now" a
third time.

### Why the follow-ups are issues, not code

`ghost_completion`: `WasmScreen`'s `render`/`tips` are `&self` + sync (matching the native trait)
but the underlying wasm calls are async, so `WasmScreen` only calls into the guest from
`on_key`/`on_event` (both already async) and caches the result (`CachedRender`, `refresh_cache` in
`crates/otto-plugin-wasm/src/adapter/interactive.rs`); the sync trait methods just read the cache.
`ghost_completion(&self, prompt: &str)` doesn't fit that model: the host calls it every render tick
with the *live* prompt-line text (`crates/otto/src/ui.rs`'s `paint_ghost_completion` call site),
which can change without this screen ever receiving a matching `on_key`. A WIT addition shaped like
`tips()` (read from the on_key/on_event-refreshed cache) would silently serve stale ghost text
between refreshes — worse than today's "no ghost text." Closing this gap correctly needs a
cache-invalidation design keyed on the actual `prompt` argument, not a one-line WIT edit, and —
because `wasmtime::component::bindgen!`'s typed export requirements make a new required guest
export a **breaking change** for already-compiled third-party components (Non-Negotiable Rule 6) —
it also needs a MINOR bump, an explicit breaking-change CHANGELOG note, and every in-tree
fixture/example updated in the same change. That's a full plan of its own, not a task inside this
one.

`StyledSpan`/`StyledLine` ergonomics: each WASM plugin crate invokes `wit_bindgen::generate!` itself
inside its own crate (see `mod bindings` in `examples/plugin-hello-interactive/src/lib.rs`), so
Rust's orphan rules already permit that crate to add its own
`impl StyledSpan { pub fn plain(...) -> Self { .. } }` today — nothing blocks this except that no
plugin does it, and there's nowhere shared to put it. The real fix is a small shared guest-side
support crate (or a documented reference snippet) that every example/fixture plugin can depend on
instead of re-implementing the same helper locally — a new crate with its own build/test/CI
surface, not a `.wit` file edit, and out of scope for a policy-recording PR.

`ContentRenderer`: needs an entire new WIT world (pixel-frame rendering, input dispatch, focus
traversal across the sandbox boundary) — its own sub-project, not a task here.

## Scope

**In:**

- `CLAUDE.md` — new "Plugin ABI (native vs. WASM parity)" subsection (position + gap table +
  pointer to the three tracking issues); Workspace map table gains rows for `otto-plugin`,
  `otto-plugin-wit`, `otto-plugin-wasm`.
- Three GitHub issues filed against `savvagent/otto` (#165, #166, #167 — already created; this spec
  documents them, it doesn't create them again).
- `CHANGELOG.md` — a `Changed`/`Added` entry recording the policy decision, added at release-cut
  time (Task 2 of the plan), citing #165/#166/#167/#170 instead of "for now."
- This design spec and its matching plan under `docs/superpowers/`.

**Out:**

- Any `.wit` file change (`crates/otto-plugin-wit/wit/*`, `crates/otto-plugin-wasm/wit/*`) — see
  "Why the follow-ups are issues, not code."
- Any change to `crates/otto-plugin-wasm/src/adapter/*.rs`, `crates/otto-plugin/src/*.rs`, or any
  example/fixture plugin — no runtime behavior changes anywhere in this PR.
- Rewriting the historical v0.28.0/v0.29.0 `CHANGELOG.md` entries — Keep a Changelog entries are a
  point-in-time record of what shipped; the "for now" wording in those two entries stays as
  written; the new convention governs entries written from here forward.
- Re-litigating whether parity actually is the right goal — the issue's own framing offers both
  answers as legitimate positions; this spec picks one and states it, per the issue's own
  instruction ("say so once ... and stop repeating 'for now'").

## Public-interface changes

**None** in this PR. `CLAUDE.md` and `CHANGELOG.md` are documentation, not the SPP wire format, a
tool schema, the plugin ABI itself, a slash command, an env var, or the on-disk transcript/keyring
format — Non-Negotiable Rule 6 is not engaged by this PR. (It is very much engaged by the *content*
of the decision this PR records, and by issue #165 specifically, whose eventual implementation
would be a breaking plugin-ABI change per the analysis above — that's for #165's own future PR to
flag when it lands.)

## Assumptions

- **Parity is recorded as the goal**, not "WASM guests get the stable subset." The issue frames
  both as legitimate; parity is chosen because (a) CLAUDE.md's `otto-development` skill already
  treats `otto-plugin`/`otto-plugin-wit`/`otto-plugin-wasm` as one interface under Non-Negotiable
  Rule 6, and a deliberately-permanent-subset position would contradict that framing without
  amending Rule 6 too (out of scope here), and (b) two of the three gaps found
  (`ghost_completion`, the `StyledSpan`/`StyledLine` ergonomics) are individually closeable without
  a fundamental redesign — a "stable subset" position would have to explain why *these two*
  specifically are permanently excluded, which the codebase gives no reason to believe.
- **The two acceptance-criteria gaps get issues, not WIT/code changes**, per "Why the follow-ups
  are issues, not code" above — the acceptance criteria explicitly permits either, and both
  turned out, on inspection, to need real design work (a cache-invalidation strategy for
  `ghost_completion`, a new shared crate for the styled-type ergonomics) that doesn't fit safely
  inside a policy-recording PR without risking a rushed, undertested plugin-ABI change.
- **The `ContentRenderer` gap is filed as a third issue** even though the acceptance criteria only
  names two, because the enumeration step (acceptance criterion 1) surfaced it and the recorded
  CLAUDE.md position requires every gap get either a WIT counterpart or a recorded reason — leaving
  it out would make the enumeration incomplete by the position's own logic.
- **`PluginId::new`/`ChordPortable::new`/`ThemePalette::new`/`ScreenArgs::screen_id()` are excluded
  from the gap table** because they're host-side validation/convenience helpers over data the
  *host* constructs from guest-supplied plain values (strings, records) — not a surface a WASM
  guest is missing, since the guest never constructs these native Rust types across the ABI in the
  first place.
- **This is not fast-pathed**, despite touching only two prose files in this PR's own diff: the
  acceptance criteria's "position ... recorded" and "gap ... enumerated" bullets are architectural
  judgment calls (is parity the goal?), not a one-sentence AC, and the work also includes filing
  three GitHub issues with real technical analysis in each — matching the otto-development skill's
  explicit instruction to use the skill's full spec → plan → implement → PR → review → merge →
  release → verify → close lifecycle for this job.

## Goal & Success Criteria

A future contributor adding a new `otto-plugin` capability, or a reviewer checking a PR against
Non-Negotiable Rule 6, can read CLAUDE.md and know: (a) parity is the stated goal, (b) exactly which
four surfaces currently diverge and why, and (c) that citing a WASM gap in a changelog entry from
here forward means citing an issue number.

- CLAUDE.md states the parity-is-the-goal position in prose, not implied by omission.
- CLAUDE.md enumerates the four current gaps (`ghost_completion`, `StyledSpan`/`StyledLine`
  ergonomics, `ContentRenderer`, tool-summary rendering) with a one-line reason each and the
  tracking issue number.
- `#165`, `#166`, `#167` exist on `savvagent/otto`, each with enough technical detail (as filed
  above) that whoever picks them up next doesn't have to re-derive the caching-model conflict or
  the orphan-rule ergonomics insight from scratch; `#170` (the tool-summary gap surfaced by review)
  exists alongside them.
- `CHANGELOG.md` gains an entry (at release-cut time) recording the decision and citing all four
  issue numbers.
- `cargo build && cargo test --workspace` and `bacon clippy-all` remain clean (vacuously — no Rust
  is touched by this PR).

## Error Handling & Edge Cases

- **Scope creep into actually implementing #165/#166** was the main risk while writing this spec —
  mitigated by the explicit "Why the follow-ups are issues, not code" section above, reasoned
  through before any code was touched, rather than discovered mid-implementation.
- **CLAUDE.md's new subsection contradicting the existing Workspace map's omission of the plugin
  crates** — resolved by adding the three plugin crates to that table in the same change, rather
  than describing a parity policy for crates the document doesn't otherwise mention.
- **Issue numbers going stale if this PR's number changes before merge** — not applicable; the
  three issues are already filed and numbered before this spec was written to reference them.

## Risks & Open Questions

- **None identified for this PR's own scope.** The follow-up issues (#165, #166, #167) each carry
  their own open design questions (documented in their bodies), which is the point — they're
  tracked there now instead of resurfacing as a third "for now" changelog entry.

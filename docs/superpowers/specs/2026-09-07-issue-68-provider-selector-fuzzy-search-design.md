# /connect provider selector fuzzy search — design

Date: 2026-09-07
Status: pending review
Related: `savvagent/otto#68`

## Problem

The built-in `/connect` flow currently opens a modal list driven directly from
`crates/otto/src/app.rs:1719-1788`, `crates/otto/src/main.rs:3869-3918`, and
`crates/otto/src/ui.rs:397-436`. The dialog is keyboard-only, but it only
supports moving a single cursor through the entire `effective_providers()`
catalog. As the built-in `PROVIDERS` registry and runtime-installed external
providers grow, the selector becomes a long scroll-only list with no in-place
filtering.

Issue #68 asks for typed fuzzy filtering inside the existing `/connect` dialog,
while preserving the current API-key capture behavior: providers that require a
key should only prompt when no keyring credential is already stored, and keyless
providers should continue to connect without any key prompt. The UI should also
degrade gracefully when the list is short so the dialog does not gain permanent
search-box clutter for only a handful of entries.

This is not fast-path work under `otto-development`: it changes an exercised TUI
interaction path, touches multiple runtime/state-machine files, and adds new
selection behavior rather than a trivial bug fix.

## Approach

Keep the existing `/connect` command and modal structure, but teach the modal to
manage a lightweight query string and a filtered view over
`crate::providers::effective_providers()`. The change stays entirely in the TUI
shell (`crates/otto`) and does not alter `otto-host`, provider transport, or
keyring persistence rules. Because the work touches `crates/otto/src/app.rs`
and `crates/otto/src/main.rs`, the implementation must preserve the host-swap
`RwLock` rule by avoiding any new `.await` while holding host read guards.

1. **App-owned selector state.** Extend `App` in `crates/otto/src/app.rs` with
   provider-selector query state and helper methods that derive the currently
   visible provider slice from the full provider catalog. `open_provider_selector`
   will reset the query, seed the cursor to the active provider when visible, and
   otherwise fall back to the first filtered match. Selection will resolve the
   currently highlighted filtered provider directly, instead of indexing the
   full unfiltered catalog. This keeps selector behavior deterministic even as
   the filtered subset changes.
2. **Manual fuzzy subsequence matching.** No fuzzy-matching crate is currently a
   workspace dependency (`Cargo.toml` has no `nucleo`, `skim`, `fuzzy-matcher`,
   etc.), so implement a small local matcher in `app.rs`/`providers.rs` that
   checks a case-insensitive subsequence match against a provider's
   `display_name` and `id`. Matching should preserve the existing provider order
   from `effective_providers()` rather than re-ranking results, so the list stays
   stable and predictable for keyboard use.
3. **Keyboard-driven filtering.** In the `InputMode::SelectingProvider` handler
   in `crates/otto/src/main.rs:3869-3918`, printable characters will append to
   the selector query, Backspace will delete, Up/Down will move within the
   filtered results, Enter will select the highlighted filtered provider, and Esc
   will first clear an active query before closing the dialog when the query is
   already empty. This keeps the flow keyboard-driven and consistent with other
   otto pickers.
4. **Graceful short-list UI.** In `crates/otto/src/ui.rs:397-436`, render a
   compact filter row only when it is useful: either the total provider catalog has more than five entries (so
   discoverability matters) or the user has
   already typed a query. For short catalogs with an empty query, keep the
   current uncluttered list-only modal. When a query yields no matches, render an
   explicit empty-state row and keep the typed query visible so the user can edit
   it in place.
5. **Preserve API-key gating.** The selection path will continue to branch on
   `ProviderSpec::api_key_required` in `crates/otto/src/main.rs` and on stored
   keyring state via `crate::creds::load(spec.id)`. The only behavioral change is
   how the selected `ProviderSpec` is chosen (filtered list instead of raw index).
   The rules remain: keyless providers skip prompting; keyed providers reuse a
   stored credential when present; the API-key modal opens only when required and
   no stored credential exists (or keyring lookup errors and the user needs to
   paste a key).
6. **Focused tests.** Add unit tests around the new selector-state helpers in
   `app.rs`, rendering tests in `ui.rs` for the search-row/empty-state behavior,
   and an event-loop test in `main.rs` for the Enter-selection gating if that is
   the narrowest place to pin the preserved credential flow. Also keep provider
   registry behavior stable by using `effective_providers()` as the single source
   of truth.

## Scope

**In:**
- `/connect` selector state, filtering, and selection logic in `crates/otto/src/app.rs`
- `/connect` modal rendering in `crates/otto/src/ui.rs`
- `InputMode::SelectingProvider` keyboard handling in `crates/otto/src/main.rs`
- Small provider-catalog helpers in `crates/otto/src/providers.rs` if useful for
  searchable text or short-list heuristics
- Unit tests covering fuzzy filtering, empty states, and preserved API-key gating

**Out:**
- New slash commands or alternate provider-picker flows
- Changes to `otto-host`, provider crates, tool registry, or transport boundaries
- Changes to keyring storage format, transcript format, or any on-disk schema
- Touching `crates/otto/src/splash.rs`
- Reordering or adding providers in `PROVIDERS` beyond helper metadata needed for
  the selector behavior

## Public-interface changes

None.

- **SPP wire format:** unchanged
- **Tool MCP schemas:** unchanged
- **Plugin ABI:** unchanged
- **Slash commands / env vars / on-disk formats:** unchanged

This is a TUI interaction improvement within the existing `/connect` command.

## Premise corrections

- The issue text refers to `requires_api_key`; the current `ProviderSpec` field
  is named `api_key_required` in `crates/otto/src/providers.rs:48-50`. The
  implementation should preserve that existing field and behavior rather than
  introducing a renamed duplicate.
- The current `/connect` flow already has the desired credential branching in
  `crates/otto/src/main.rs:3876-3914` and `crates/otto/src/app.rs:1740-1788`;
  this work preserves that logic while changing how a provider is located.
- Built-in providers are no longer just the original three/four hosted options;
  the selector now includes at least five built-ins plus any external provider
  plugins surfaced through `effective_providers()`. The scaling problem is about
  the combined runtime catalog, not just the compile-time built-in slice.

## Assumptions

- **A small manual subsequence matcher is preferable to a new dependency.**
  Rationale: no fuzzy-match crate is already present in the workspace, and the
  issue does not justify expanding the dependency graph for a tiny selector.
- **The filter should match both provider display name and stable id.**
  Rationale: users may think in branded names ("OpenAI") or ids used elsewhere
  in otto (`openai`, `deepseek`).
- **Short-list degradation means no always-visible search row when the provider
  catalog has five or fewer entries.** Rationale: the issue explicitly asks to
  avoid search-box clutter; hiding the row at today's five built-ins keeps the
  existing uncluttered modal while still allowing typing to reveal filtering.
- **Esc should clear the active query before dismissing the dialog.**
  Rationale: this matches common picker behavior and lets keyboard users recover
  from an over-restrictive query without reopening `/connect`.
- **This ships as a MINOR release (`v0.26.0`) once merged.**
  Rationale: the change adds a new interactive user-facing capability within a
  pre-1.0 release line, which this repo treats as a feature.

## Goal & Success Criteria

Enhance the existing `/connect` modal so provider selection scales to a growing
provider catalog through typed fuzzy filtering, without regressing the existing
keyring-based API-key capture rules.

- [ ] While the selector is open, typing narrows the visible provider list in
      place via case-insensitive fuzzy matching on provider name/id.
- [ ] Up/Down/Enter operate on the filtered list, and an empty result set keeps
      the dialog open with an editable query and a clear empty-state message.
- [ ] Providers with `api_key_required = false` continue to connect immediately
      with no stored-key lookup or API-key prompt.
- [ ] Providers with `api_key_required = true` continue to open the API-key
      modal only when no stored credential exists; stored credentials still
      bypass the prompt.
- [ ] For short provider catalogs, the selector remains uncluttered until a
      query is actually needed, while catalogs with six or more entries show a
      discoverable filter affordance.

## Error Handling & Edge Cases

- A query with zero matches must not panic or implicitly fall back to the full
  list; it should render an empty state and suppress Enter-selection until a
  real match exists.
- Changing the query should clamp or reset the cursor into the filtered result
  range so stale indexes from the unfiltered list cannot address the wrong
  provider.
- Clearing the query should restore the active provider selection when possible,
  or the first provider otherwise, so the dialog never lands on an invalid row.
- Keyring read errors should keep the current behavior: surface a note and allow
  manual API-key entry for keyed providers, while keyless providers continue to
  bypass the keyring entirely.
- The modal should continue to function when external plugin providers are
  installed through `install_external_providers`; filtering must read the
  combined `effective_providers()` list rather than the built-in `PROVIDERS`
  slice alone.

## Risks & Open Questions

- The fuzzy matcher is intentionally simple. If users later want ranking or more
  sophisticated typo tolerance, that can be a follow-up once there is evidence a
  small subsequence filter is insufficient.
- Rendering a hidden-until-needed search row for short catalogs requires care so
  popup layout and footer help text remain visually balanced in small terminals.
- The selector currently resolves stored credentials directly inside the main
  input-event loop. Tests need to pin the preserved branching carefully so the
  new filtered-selection indirection does not accidentally bypass or duplicate
  keyring checks.
- External providers can expand the runtime catalog at startup. The short-list
  threshold should therefore be based on `effective_providers().len()` at dialog
  open/render time, not a hardcoded assumption about today's built-ins.

# /connect picks a provider — remove the provider-specific connect commands

Date: 2026-09-09
Status: pending review
Related: `savvagent/otto#82`

## Problem

Connecting a provider currently has two paths: `/connect` alone (opens the provider picker) and the
provider-specific `/connect <provider>` commands (`/connect anthropic`, `/connect gemini`,
`/connect openai`, `/connect deepseek`, `/connect local`). Issue #82 asks to collapse these: the
picker is the one way to select a provider, and the provider-specific *user-facing* commands go away.

The provider-specific commands are not just discoverability clutter — they are registered as named
slash entries (`connect anthropic`, …) by each provider plugin, so they also appear as tappable
entries in the `/` command palette, duplicating the picker's job and accounting for the long-command
collisions the palette's current layout workaround exists for (`command_palette/screen.rs:121-122`).

## Approach

Keep the internals that make the picker (and its companion flows) work, and remove only the
*user-visible* command surface:

### 1. `/connect` opens the picker unconditionally

`ConnectPlugin::handle_slash` (`crates/otto/src/plugin/builtin/connect/mod.rs:81-109`) currently
routes one arg to `Effect::RunSlash { name: "connect {provider}" }` (after validating the provider-id
character set). Change it to ignore all args and always return `Effect::OpenScreen { id:
"connect.picker", args: ScreenArgs::ConnectPicker }`. The arg-validation error path and the
`args_hint: Some("[provider]")` in `manifest()` are removed with it. This makes `/connect <anything>`
(literal or typed into the prompt) open the picker, which is what the issue asks for.

### 2. Keep the internal `connect <id>` slashes, but hide them from the palette

The provider plugins' `connect <id>` slash registrations stay **unchanged** as internal plumbing.
Three existing flows depend on them and none of them is a user-typed command:

- The picker's `Enter`/`Alt+Enter` actions dispatch
  `Effect::RunSlash { name: "connect {pid}"[, args: ["--rekey"]] }`
  (`crates/otto/src/plugin/builtin/connect/screen.rs:247` and `:410`). The picker was always the
  source of these dispatch strings, not the user.
- The silent stored-key reconnect and `--rekey` flows inside each provider plugin are triggered by
  that same slash name (`provider_*/mod.rs` `handle_slash`).
- `build_connect_candidates` (`crates/otto/src/plugin/effects.rs:907-920`) filters the picker's
  candidate list by whether `connect {id}` is currently routable in the slash index.

So the internal names become a **reserved, private namespace** (`connect <id>`) owned by the
`internal:connect` plugin. `build_palette_commands` (`crates/otto/src/plugin/effects.rs:798-833`)
gains a filter that drops any slash whose name begins with `connect ` from the `/` command palette,
mirroring the existing enable/disable filtering. The predicate lives on the connect plugin
(`connect::is_internal_connect_namespace(name)`) since that plugin owns the namespace.

Consequence, documented under Assumptions: a user-defined slash command *also* named `connect <x>`
becomes unreachable. This is already effectively true on the typed path after section 1 (any typed
`/connect x` opens the picker), so hiding those entries from the palette is consistent rather than
surprising: the palette must never offer a command that can no longer run.

### 3. Update the strings/docs that instruct `/connect <id>` or `--rekey`

The following user-facing strings tell the user to type a provider-specific command and must be
rewritten to point at the picker (and its `Alt+Enter` re-key action). Same keys across all four
locales (`en`, `es`, `pt`, `hi`):

- `notes.connect-rejected-keyed` — "Run `/connect %{id} --rekey` …" → "run `/connect` and pick `%{id}`
  with Alt+Enter to try a different key"
- `notes.connect-already` — today ends with "…or pass `--rekey` to `/connect`" → point at
  `/disconnect` + the picker instead
- `notes.turn-auth-failed-hint` — "Run `/connect %{id} --rekey` to enter a different API key for
  `%{name}`" → "/connect, pick `%{name}`, Alt+Enter to enter a different key"
- `notes.startup-build-failed`, `notes.startup-timeout` — "Run `/connect %{id}` to retry" → "run
  `/connect` and pick `%{name}`"
- `notes.use-not-connected` — "Use `/connect %{name}` first" → "Use `/connect` first to add `%{name}`"

Corresponding source references to refresh in `crates/otto/src/providers.rs` (`turn_auth_hint` doc +
its test's `/connect gemini --rekey` assertion), the five provider plugins' module docs/comments that
frame `connect <id>` as a user-runnable command, the `command_palette/screen.rs` stale-collision
comment, and the `/connect` row in `README.md`.

### 4. No protocol/plugin-ABI change

`otto-plugin` (SlashSpec/Effect), the `.wit` interface, SPP wire format, keyring/transcript formats,
and the plugin runtime are untouched. `RegisterProvider`, `PromptApiKey`, `OpenScreen`, `RunSlash`,
and `ProviderRegistered` continue to do exactly what they do today. The provider-id validation logic
moves out of the connect plugin's slash handler (no longer reachable) — merely removed, not relocated.

## Scope

**In:**
- `crates/otto/src/plugin/builtin/connect/mod.rs` — `/connect` ignores args, always opens the picker;
  drop `args_hint` and the arg-validation path; add `connect::is_internal_connect_namespace`; update
  module docs; rewrite the now-outdated `handle_slash` tests.
- `crates/otto/src/plugin/effects.rs` — filter the `connect <id>` namespace out of
  `build_palette_commands`; targeted palette test.
- `crates/otto/src/providers.rs` — `turn_auth_hint` doc/test refresh.
- The five provider plugins' module docs/comments referencing `/connect <id>` as a user command —
  comments and test doc-strings only, no behavior change, registrations kept.
- `crates/otto/src/plugin/builtin/command_palette/screen.rs` — stale-collision comment refresh.
- Locale files `crates/otto/locales/{en,es,pt,hi}.toml` — the five keys above.
- `README.md` — the `/connect` command row.

**Out:**
- Removing or renaming the provider plugins' internal `connect <id>` slashes (the picker, silent
  stored-key reconnect, `--rekey`, and `build_connect_candidates` all route through them).
- Any change to `otto-plugin` SlashSpec/Effect, the `.wit` interface, the SPP wire format, env vars,
  or on-disk/keyring formats.
- Touching the TUI's legacy `App::refresh_commands` select-provider path or the
  host-swap/`RwLock`/`ProgressDispatcher` machinery — none of this change reaches those.
- Making `/connect <provider>` an alias, a deprecation warning, or a sub-command of `/mcp`.
- Restoring a direct-connect shortcut of any kind; the picker is the one entry point.

## Public-interface changes

**Breaking (user-facing):** the slash-command surface loses the documented provider-specific
commands (`/connect anthropic`, `/connect gemini`, `/connect openai`, `/connect deepseek`,
`/connect local`) — typing any of them now opens the picker instead of connecting directly. Executing
one still works (pick in the picker, Enter), so no stored-key / keyring functionality regresses, but
this is a behavior change to documented commands. Per the repo's versioning policy (pre-1.0:
MINOR captures features + breaking boundary changes) this ships as part of the next MINOR release
(v0.27.0), with a `CHANGELOG.md` entry. The internal slash names are unchanged and remain
routable/dispatchable by effect.

Everything else (SPP wire format, tool schemas, plugin ABI, env vars, on-disk formats) is
**unchanged**.

## Premise corrections

- The conflict this removes is real and already visible in the palette: the provider-specific
  `connect <id>` entries appear alongside `/connect` in the `/` palette today — see the layout
  workaround comment in `command_palette/screen.rs:121-122`. The palette filter in section 2 removes
  the duplicate entries at the source.
- The provider-specific commands are dispatched as *named* slashes, so they cannot be collapsed
  "for free" by only changing `ConnectPlugin::handle_slash`; the palette also needs the namespace
  filter. Both halves are needed.
- `/connect anthropic` today is *not* handled by `ConnectPlugin`'s manifest directly — it routes arg
  → `RunSlash { name: "connect anthropic" }` → the `provider-anthropic` plugin's `handle_slash`. The
  picker already uses exactly that same hop (`connect {pid}`, optional `["--rekey"]`), so this design
  keeps one routing mechanism for all paths.

## Assumptions

- "Remove the provider-specific commands" means *remove them from the user-facing surface* — typed
  input and the command palette — without breaking the picker's internal dispatch, the silent
  stored-key reconnect, or the `Alt+Enter` re-key flow. Those are documented features whose plumbing
  is the same named slash.
- A user-defined slash command named `connect <x>` becomes unreachable and is hidden from the
  palette along with the built-ins, because `/connect <anything>` now opens the picker by definition.
  This matches the issue's intent ("`/connect` and nothing else") and is strictly more honest than
  listing a command that can no longer execute.
- Every connect-related recovery string should point at the picker (with `Alt+Enter` for re-keying)
  rather than at a literal `/connect <id> [--rekey]`, so no locale string instructs a removed
  command.
- Keeping the `connect <id>` namespace filter as a string-prefix convention (rather than a new
  "hidden" field on `SlashSpec`, or an explicit ownership check) is the right trade: the namespace is
  reserved by the connect plugin that owns it; a `SlashSpec` field would ripple through 38 literal
  constructions across the workspace and the published `otto-plugin` ABI for no additional
  capability.

## Goal & Success Criteria

When a user wants to connect a provider, the provider picker is the one command.

- [ ] Typing `/connect` (with or without any arguments) opens the provider picker.
- [ ] The `/` command palette lists `/connect` but **not** `/connect anthropic`, `/connect gemini`,
      `/connect openai`, `/connect deepseek`, or `/connect local` (and no other `connect <id>` from
      enabled provider plugins).
- [ ] In the picker, Enter still connects the highlighted provider and `Alt+Enter` still opens the
      API-key modal for it; the silent stored-key reconnect still happens without a modal.
- [ ] All connect-related error/recovery notes (`connect-rejected-keyed`, `connect-already`,
      `turn-auth-failed-hint`, `startup-build-failed`, `startup-timeout`, `use-not-connected`) point
      at `/connect` + the picker, not a literal `/connect <id>` — in all four locales.
- [ ] README's `/connect` row documents the picker as the only connect path.
- [ ] No `otto-plugin`, `.wit`, SPP, env-var, or on-disk-format change; `cargo test --workspace` and
      `cargo clippy --workspace --all-targets` stay clean; `cargo fmt --all` clean.

## Error Handling & Edge Cases

- `/connect <junk>` (any args): opens the picker. No more `InvalidArgs` error path — the validation
  is simply unreachable and is removed.
- Provider not routable (externally disabled, not built): unchanged — `build_connect_candidates`
  still filters the picker by slash-index routability, and the palette filter runs on the same
  rebuilt index.
- Empty provider catalog: unchanged — the picker already renders its empty state and
  `open-plugins-hint`.
- User-defined slash named `connect <x>`: hidden from the palette (consistent with its
  unreachability); registered as before at index-build time.
- Re-keying an already-connected provider from the picker: unchanged behavior (pre-existing
  `connect-already` boundary), only the note's wording changes.

## Risks & Open Questions

- External/WASM provider plugins that register a `connect <id>` slash get their palette entries
  hidden too. That is the intended convention, but the palette filter is the one behavioral surface
  that reaches beyond the bundled five providers — worth an explicit line in the PR description.
- The `note.connect-already` wording change drops an explicit `--rekey` affordance in favor of
  `/disconnect` + re-pick (`Alt+Enter`). Re-picking from `/connect` after an already-connected
  failure hits the pre-existing re-key boundary described above; the wording keeps the `/disconnect`
  first-step so the guidance remains actionable.
- None of this changes turn-loop, tool, transport, or provider-registry behavior, so the
  host-swap `RwLock`, `ProgressDispatcher` forwarder-abort, and keyring-plaintext invariants are
  untouched (verified by review, not by code reach).
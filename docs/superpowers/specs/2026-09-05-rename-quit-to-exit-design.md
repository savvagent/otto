# Rename `/quit` to `/exit` — design

Date: 2026-09-05
Status: IMPLEMENTED
Related: `savvagent/otto#20`

## Problem

Otto's slash-command surface still uses `/quit` for session termination, while the issue's
acceptance criteria require `/exit` to match the convention used by comparable agentic coding CLIs.
The existing implementation is a built-in core plugin that registers `/quit`
(`crates/otto/src/plugin/builtin/quit/mod.rs:1`), is advertised in the home command list
(`crates/otto/src/app.rs:1427`), appears in the command-palette regression fixture
(`crates/otto/src/plugin/builtin/command_palette/screen.rs:284`), and is documented in the
README (`README.md:160`). Because the README-documented slash-command surface is a public interface
under the repo's workflow rules, renaming this command is a deliberate breaking change that cannot
fast-path.

## Goal & Success Criteria

Rename the built-in quit command surface from `/quit` to `/exit` everywhere users can invoke,
discover, or read about it, while keeping the underlying shutdown behavior identical: the plugin
must still emit `Effect::Quit`, `apply_effects` must still map that effect to
`App::request_quit`, and Ctrl-C / Ctrl-D quit behavior must remain unchanged.

Success criteria:
- `/exit` is the only built-in slash command exposed for session termination; `/quit` is removed,
  not kept as an alias.
- The built-in manifest, command list, command palette coverage, locale strings, and README all use
  `/exit` consistently.
- Existing shutdown behavior is unchanged: selecting `/exit` still results in `Effect::Quit` and a
  `should_quit = true` transition.
- The breaking slash-command rename is called out in spec, plan, PR, CHANGELOG, and the release's
  MINOR version bump.

## Approach

Rename the command at the built-in plugin boundary first, then update every user-facing discovery
surface and test that hard-codes the old spelling.

1. In `crates/otto/src/plugin/builtin/quit/mod.rs:8-81`, keep the module path and `Effect::Quit`
   behavior but rename the user-facing command from `quit` to `exit`, retitle the plugin metadata,
   and rename the core plugin id from `internal:quit` to `internal:exit` so the plugin manager and
   builtin-manifest tests stay semantically aligned with the new command name.
2. Update builtin registration expectations in `crates/otto/src/plugin/mod.rs:237` and the
   home command list in `crates/otto/src/app.rs:1427` to advertise `/exit` instead of `/quit`.
3. Update command-palette fixture/test coverage in
   `crates/otto/src/plugin/builtin/command_palette/screen.rs:284-352` so the regression test
   still proves the termination command is discoverable and dispatches `Effect::RunSlash { name:
   "exit", .. }`.
4. Keep `Effect::Quit` unchanged in `crates/otto/src/plugin/effects.rs:215` and its regression
   coverage in `crates/otto/src/plugin/effects.rs:1201-1219`; only update comments/test prose
   that name `/quit`, because the effect is the stable internal shutdown action while the slash name
   is the public surface being renamed.
5. Update locale keys and README docs so user-facing text no longer mentions `/quit`. The key names
   may stay `quit-summary` / `quit-description` if that avoids unnecessary churn, but the rendered
   strings must say "Exit" / "/exit". If renaming the locale keys proves low-risk, do it in the same
   change for consistency.
6. Guard the more-common `/exit` spelling against user-defined command collisions at startup. A
   `commands/exit.md` file should no longer panic the app during `Indexes::build`; instead, the
   built-in `internal:exit` command keeps ownership and the conflicting discovered user command is
   skipped, matching the existing `/reload-commands` reindex behavior. The static built-in
   `/reload-commands` contribution itself remains a hard conflict if a user tries to shadow it.

## Scope

**In:**
- The built-in exit command plugin implementation and tests.
- Builtin plugin registration expectations.
- Home command-list and command-palette references to the old spelling.
- User-facing locale strings and README command documentation.
- CHANGELOG entry documenting the breaking slash-command rename.

**Out:**
- Ctrl-C / Ctrl-D keyboard quit behavior.
- `Effect::Quit` naming or semantics.
- Provider/host/tool transport logic, keyring behavior, transcript format, or any other public
  interface outside the slash-command surface.
- Adding a deprecated `/quit` alias.

## Public-interface changes

**Breaking slash-command-surface change.** `/quit` is removed and replaced with `/exit` on the
README-documented slash-command surface. Per the repo's pre-1.0 versioning convention, this must
ship in a **MINOR** release and be called out in `CHANGELOG.md` as a breaking rename.

The internal `Effect::Quit` variant is intentionally unchanged, so there is no SPP wire-format,
provider-trait, tool-schema, plugin-ABI, or on-disk-format change in this work. If the built-in
plugin id is renamed from `internal:quit` to `internal:exit`, treat that as an internal consistency
change for a core built-in plugin, not as a separately versioned public ABI surface.

## Premise corrections

- The issue's affected-file list is incomplete for the current codebase: locale catalogs,
  `crates/otto/src/plugin/mod.rs`, and command-palette tests also hard-code `/quit` and must be
  updated for a coherent rename.
- `crates/otto/src/tui.rs` does not implement the slash command; it only contains a generic
  restore comment about quitting from the alternate screen. Functional changes are not expected
  there unless wording cleanup is warranted.
- The rename itself does not need new runtime shutdown behavior in `crates/otto/src/main.rs`,
  because quitting is already expressed through `Effect::Quit`; however, startup regression coverage
  and user-command collision handling may still need small `main.rs` / `plugin/manifests.rs` changes
  once `/exit` becomes the built-in spelling.

## Assumptions

- Remove `/quit` outright instead of keeping a deprecated alias, because this is a pre-1.0 product,
  the workflow treats slash-command renames as deliberate breaking changes, and the issue asks for a
  rename rather than compatibility layering.
- Rename the built-in plugin id to `internal:exit` for consistency with the command name, because it
  is a core built-in identifier rather than a documented external ABI and keeping `internal:quit`
  would be confusing in plugin-manager/debug output.
- Keep the module path `plugin/builtin/quit/` for this issue unless implementation friction is low,
  because the behavioral goal is the slash-surface rename, not filesystem churn.
- Keep `Effect::Quit` and `App::request_quit` naming unchanged, because the runtime concept remains
  "request application shutdown" and renaming internal effect names would broaden scope without user
  value.

## Error Handling & Edge Cases

- Selecting `/exit` from the command palette must still dispatch cleanly through `Effect::RunSlash`
  to `Effect::Quit`; the palette regression test should cover that exact path.
- Any code comment or test that still claims `/quit` is the public command would be a documentation
  regression even if behavior is correct.
- A project-local or user-wide `commands/exit.md` must not crash startup now that `/exit` is the
  built-in command; the builtin should win and the conflicting discovered command should be skipped.
- A user-defined `reload-commands.md` must still fail hard, because that collides with the
  `internal:user-slash-commands` plugin's own built-in static slash rather than with a discovered
  command that can be safely skipped.
- If locale-key renaming would force wide churn across all translations for no runtime benefit,
  keeping the key names while updating their rendered strings is acceptable; consistency of rendered
  output matters more than internal i18n key names.

## Risks & Open Questions

- Renaming the built-in plugin id could leave a missed test or hard-coded expectation outside the
  initially listed files; grep-based verification is required before implementation closes.
- Historical CHANGELOG entries mentioning `/quit` should remain untouched; only `[Unreleased]` and
  the new release entry should describe the new breaking rename.
- The review steps should explicitly verify that no `.await`/lock discipline or transport boundary
  is touched; this change should remain a surface rename only.

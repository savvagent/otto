# Expose discovered skills via /skills — design

Date: 2026-09-08
Status: pending review
Related: `savvagent/otto#83`

## Problem

Otto's public slash-command surface documents tools, providers, agents, hooks,
and user-defined commands, but it has no way to show the repository skills that
contributors are expected to use. The gap is visible in two places:

- `README.md:141-160` lists built-in slash commands such as `/tools`, but no
  `/skills` entry exists.
- `CLAUDE.md:87-95` establishes two committed skill locations for this repo —
  `.github/skills/` for Copilot CLI skills and `.claude/skills/` for
  Claude-Code-compatible skills — but that information is only readable by
  opening files manually.

Issue #83's acceptance criterion is specifically user-visible: after the prior
skills work, users need a `/skills` command so they can see the available
skills.

## Premise corrections

- Otto already ships **user-defined agents** discovery for the runtime `task`
  tool (`README.md:277-317`,
  `crates/otto/src/plugin/builtin/user_agents/discovery.rs:12-18`), but that is
  a different surface from repository skills. Agents are discovered from
  `.otto/.claude/agents`; skills are committed under `.github/skills/` and
  `.claude/skills/`.
- There is no existing runtime skill-discovery plugin today. Built-in plugin
  registration in `crates/otto/src/plugin/mod.rs:87-144` includes
  `internal:user-agents` and `internal:user-slash-commands`, but nothing for
  skills. `/skills` therefore needs its own discovery path rather than trying to
  read the agent index or the `task` tool schema.

## Goal & Success Criteria

Add an additive `/skills` slash command that reports the skills Otto can
discover for the current repository so users no longer need to inspect the tree
manually.

Success means:

- `/skills` is a built-in slash command visible in Otto's command palette.
- Invoking `/skills` prints the discovered skills' **name**, **description**,
  and **location**.
- The command reflects the actual committed skill files under this repo's
  supported skill roots instead of a hardcoded list.
- README slash-command documentation includes `/skills` and explains what it
  lists.
- The change remains additive to the slash-command surface and does not alter
  the `task` tool, agent discovery, or any on-disk format.

## Approach

Add a new built-in plugin, `internal:user-skills`, under
`crates/otto/src/plugin/builtin/user_skills/`, and register it from
`crates/otto/src/plugin/mod.rs` alongside the other built-ins.

The plugin will contribute a `/skills` slash command and perform best-effort
skill discovery directly from the repository's documented skill roots:

1. `<project>/.github/skills/*/SKILL.md`
2. `<project>/.claude/skills/*/SKILL.md`

Each discovered `SKILL.md` file will be parsed for YAML frontmatter,
extracting:

- `name` — required for display and for surfacing mismatches against the
  containing directory slug.
- `description` — required for user-facing output.
- derived `location` — sourced from the root path (`.github/skills` vs
  `.claude/skills`) so the listing tells users which compatibility surface owns
  the skill.

The command will rediscover skills when invoked so its output matches the files
on disk without needing a separate `/reload-skills` command. Results will be
sorted deterministically (location, then name) and emitted as note lines in the
same lightweight style `/tools` uses (`crates/otto/src/main.rs:1283-1310`): a
header with the total count, followed by one line per skill.

Malformed or incomplete `SKILL.md` files should not crash the command or the
TUI. Discovery is best-effort: invalid files are skipped with a tracing warning,
and `/skills` reports the valid discovered set. If nothing is found, the command
returns a clear "no skills discovered" note instead of silent success.

## Scope

### In

- New built-in `/skills` slash command.
- Runtime discovery of committed repo skill files from `.github/skills/` and
  `.claude/skills/`.
- User-facing note output containing skill name, description, and location.
- Unit coverage for discovery/parsing and slash output behavior.
- README documentation for the new slash command and repo-skill listing.

### Out

- Changing how `task` subagents work or how agents are discovered.
- Adding execution/loading of skill bodies into the model prompt.
- Discovering personal home-directory skills outside the project tree.
- Adding a new MCP tool, screen, modal, or on-disk configuration format.
- Changing `CHANGELOG.md` in the feature PR; the required release PR will carry
  the changelog entry per `RELEASING.md` and `otto-development`.

## Public-interface changes

This is an **additive** slash-command-surface change: Otto gains `/skills`.
There is no breaking change to the SPP wire format, provider/tool traits,
plugin ABI, tool schemas, or transcript/keyring formats.

## Assumptions

- Users asking for `/skills` want the skills that are actually committed for the
  current repository, not personal home-directory skills, because the repo's
  conventions only document committed `.github/skills/` and `.claude/skills/`
  roots.
- Listing the source root as the skill "location" is sufficient and more useful
  than printing raw absolute paths, because the distinction users need is
  Copilot-skill vs Claude-compatible skill provenance.
- `/skills` can be note-based instead of screen-based because the issue asks for
  discoverability, not a new interactive management UI.

## Error Handling & Edge Cases

- Missing skill directories: `/skills` returns a "no skills discovered" note.
- Malformed frontmatter or missing `name`/`description`: skip the file, log a
  warning, continue.
- Frontmatter `name` disagrees with the containing directory: the directory slug
  wins for display, and a warning is logged.
- Duplicate names across `.github/skills` and `.claude/skills`: both entries are
  shown because the location field disambiguates them and the repo conventions
  allow complementary skills across both roots.

## Risks & Open Questions

- The repo currently stores one Copilot skill helper file,
  `.github/skills/otto-development/agent-prompts.md`, beside `SKILL.md`. The new
  discovery must only consider `SKILL.md`, not sibling docs, or the listing will
  over-report.
- Future work may want personal-skill discovery if Otto grows a documented
  end-user skill story beyond repo-authored skills. This issue does not require
  that broader surface.

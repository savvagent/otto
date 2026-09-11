# Claude Code compatibility (sub-project E) — commands, skills, plugins, hooks — design

Date: 2026-09-09
Status: E1 reviewed and approved for implementation (corrections from spec critique folded in: tool registration mechanism, five-tier discovery, level-1 catalog sync/async bridge, trust-prompt reachability, dropped picker Screen in favor of an enriched listing); E2-E4 drafted, awaiting review before their own implementation passes
Issue: `savvagent/otto#83`
Supersedes: nothing
Related:
- `docs/superpowers/specs/2026-05-21-user-slash-commands-design.md` — sub-project A; four-path discovery, trust-file pattern
- `docs/superpowers/specs/2026-05-22-user-hooks-design.md` — sub-project B; hook stdin contract, hook kinds, `PreToolUseGate`
- `docs/superpowers/specs/2026-05-23-user-agents-design.md` — sub-project C; subagent dispatch via the built-in `task` tool, `SubagentStop`
- `docs/superpowers/specs/2026-05-25-external-plugins-design.md` — sub-project D; WASM plugin runtime, three WIT worlds, SHA-256 trust gating

## Context: the sub-project split

| Order | Sub-project | Status |
|-------|-------------|--------|
| A | User slash commands | shipped (v0.17.0) |
| B | User-defined hooks | shipped (v0.17.0) |
| C | Agents (subagents via the built-in `task` tool) | shipped (v0.17.0) |
| D | External plugins (WASM, three WIT worlds) | shipped (v0.18.0) |
| **E** | **Claude Code compatibility — commands, skills, plugins, hooks** | *this spec* |

A/B/C/D each established that otto reads a `.claude/` path *alongside* its
own `.otto/` path. That was always framed as a courtesy — "Claude Code
compatible" in the README — never as a contract anyone verified against a
real Claude Code installation.

This spec makes it a contract.

## Problem

Issue #83 was filed as "`/skills` command doesn't exist". Investigating it
surfaced that the premise behind it — "in a previous issue, we supported
Claude Code skills" — is false, and that the gap is wider than one missing
slash command.

Issues #23 / #24 / #55 formalized how **this repository's own contributors**
use Claude Code skills (`.claude/skills/`, `.github/skills/`). They changed
nothing about what **otto's users** get. `grep -ri skill crates/` returns
zero hits: otto has no skill discovery, no `SKILL.md` parsing, no skill
invocation path, and no `/skills` command.

Widening the check to all four extension surfaces a Claude Code user
actually owns:

| Surface | otto today | Verdict |
|---|---|---|
| **Commands** | `user_slash_commands` reads all four dirs incl. `.claude/commands/`; frontmatter, `$ARGUMENTS`/`$N`, `@path`, `!cmd`, trust prompt, `/reload-commands` | **works**, with `allowed-tools` parsed-but-unenforced and no plugin namespacing |
| **Skills** | nothing | **absent** |
| **Plugins** | `otto-plugin-wasm` scans `~/.claude/plugins/<id>/plugin.toml` for otto's own WASM format | **broken by construction** — see below |
| **Hooks** | `user_hooks` reads `settings.json` from all four dirs; 6 events; blocking protocol; `/reload-hooks` | **works**, missing 3 events + a `PostToolUse` matcher restriction |

### The plugin path collision

Sub-project D chose the same four-path convention A/B/C used and applied it
to plugins, so `otto-plugin-wasm/src/discovery.rs` walks:

```
<project>/.otto/plugins/<id>/plugin.toml
<project>/.claude/plugins/<id>/plugin.toml
~/.otto/plugins/<id>/plugin.toml
~/.claude/plugins/<id>/plugin.toml
```

But `~/.claude/plugins/` is not a directory of otto plugin folders. It is
Claude Code's own plugin state root, and its real shape is:

```
~/.claude/plugins/
  config.json                     # { "repositories": {} }
  installed_plugins.json          # { "version": 2, "plugins": { "<name>@<marketplace>": [ { scope, installPath, version, ... } ] } }
  known_marketplaces.json
  marketplaces/<marketplace-id>/  # cloned marketplace repo, with .claude-plugin/marketplace.json
  cache/<marketplace>/<plugin>/<version>/   # the installed plugin tree
  repos/
```

and an installed plugin tree looks like:

```
<installPath>/
  .claude-plugin/plugin.json      # { name, description, author, version? }
  commands/*.md
  agents/*.md
  skills/<name>/SKILL.md
  hooks/hooks.json                # optional
  .mcp.json                       # optional — MCP server definitions
```

The failure is silent, not loud: `cache/`, `marketplaces/`, and `repos/`
contain no `plugin.toml`, so discovery skips them and reports nothing. A
user with sixteen Claude Code plugins installed sees an empty `/plugins`
list and no diagnostic. Worse, otto has staked a claim on a directory whose
format it does not own and cannot control.

### What a user actually expects

> I already run Claude Code. I have `~/.claude/skills/`, a handful of
> plugins from the official marketplace, project hooks in
> `.claude/settings.json`, and team commands in `.claude/commands/`.
> I point otto at the same repo and my setup comes with me.

Everything below serves that sentence.

## Goals

1. A Claude Code user's **skills** (personal, project, and plugin-bundled)
   are discovered, listed by a new `/skills` command, injected under
   progressive disclosure, and invocable by the model.
2. A Claude Code user's **plugins** — as actually installed on disk by
   `claude plugin install` — are read, and every asset they bundle
   (commands, agents, skills, hooks, MCP servers) lands in otto's
   corresponding index, namespaced.
3. **Commands** and **hooks** close their remaining parity gaps.
4. otto's own WASM plugin format stops squatting on `.claude/plugins/`.
5. Nothing a Claude Code user has on disk needs editing, moving, or
   duplicating for otto to consume it.

## Non-goals

- **Marketplace install/update from inside otto.** v1 consumes what
  `claude plugin install` has already put on disk. `/plugins install
  <marketplace>` for the Claude Code format is a follow-up.
- **Executing Claude Code's own tool names.** A skill or command whose body
  says "use the Read tool" is prose to a model that sees `tool-fs:read_file`.
  We translate discovery and packaging, not tool vocabulary. A translation
  shim is out of scope; §"Tool-name divergence" documents the seam.
- **`settings.json` keys beyond `hooks`.** `permissions`, `env`,
  `statusLine`, `model`, `enabledPlugins` and the rest stay unread in v1.
- **`PreCompact` hooks.** otto has no compaction pass to hang them off
  (`grep -i compact crates/` is empty). Deferred until it does.
- **Writing to `~/.claude/`.** otto reads that tree; it never mutates it.
  All otto-owned state stays under `~/.otto/`.
- **Skills as a plugin-authoring surface.** Skills are markdown+assets, not
  WASM. Sub-project D's `Plugin` trait is untouched.

## Approach

Four work-streams, delivered as four PRs and four releases, in dependency
order. E1 is the load-bearing one; E2/E3/E4 are narrower.

| Stream | Scope | Release |
|---|---|---|
| **E1** | Skills subsystem: discovery, index, `skill` tool, `/skills`, `/reload-skills`, progressive disclosure | v0.27.0 |
| **E2** | Claude Code plugin loader; move otto's WASM discovery off `.claude/plugins/` | v0.28.0 |
| **E3** | Hook parity: `SessionEnd`, `Notification`, plugin-supplied `hooks.json`, `PostToolUse` matcher lift, `${CLAUDE_PLUGIN_ROOT}` | v0.28.1 |
| **E4** | Command parity: enforce `allowed-tools`, plugin namespacing, `disable-model-invocation` | v0.28.2 |

### E1 — Skills

Mirrors sub-project C (`user_agents`) almost module-for-module, because a
skill and an agent are the same shape of artifact: a discovered markdown
file with YAML frontmatter, indexed by slug, surfaced to the model through
a built-in MCP tool.

**Discovery.** Five paths as actually shipped in Task 1 (`SkillScope`),
existing precedence (project beats user; within project, `.otto/` beats
`.claude/` beats `.github/`), first-wins dedup by directory slug:

```
<project>/.otto/skills/<name>/SKILL.md
<project>/.claude/skills/<name>/SKILL.md
<project>/.github/skills/<name>/SKILL.md
~/.otto/skills/<name>/SKILL.md
~/.claude/skills/<name>/SKILL.md
```

`<project>/.github/skills/` is Copilot CLI's location rather than a Claude
Code one; otto reads it (as `SkillScope::ProjectGithub`) so repos that
already keep skills there — this one included — work without moving them.
It has no user-scope counterpart, since Copilot CLI only defines it inside
a repository, and it ranks below `.claude/` project scope. For the level-3
trust gate (below) it counts as project scope like the other two: those
skills arrive with the checkout.

Plus, once E2 lands, every enabled Claude Code plugin's `skills/<name>/SKILL.md`,
registered under the namespaced slug `<plugin>:<name>` (`SkillScope::Plugin`).
Plugin skills rank below all five filesystem tiers, so a user can shadow a
plugin skill by slug.

**Format.** Claude Code's, unchanged:

```markdown
---
name: rust-engineer
description: Use when building Rust systems where memory safety, ownership
  patterns, and zero-cost abstractions matter.
allowed-tools: tool-fs:read_file, tool-grep:search
---

# Rust engineer

...instructions...
```

`name` and `description` are required; a missing `description` is the one
hard parse error (it is the only thing the model sees at level 1). `name`
defaults to the directory slug and warns on mismatch, matching
`user_agents::frontmatter`. Unknown keys warn and are ignored — Claude Code
ships `license` and `metadata` in the wild and neither should be fatal.

**Progressive disclosure.** Three levels, matching Claude Code:

1. **Level 1 — always resident.** Name + description for every discovered
   skill, emitted as a single `SystemPromptSegment` with id
   `internal:user-skills:catalog` via the existing `otto-plugin::prompt`
   seam. Bounded: descriptions truncate at 1024 chars, the catalog caps at
   200 skills, and the segment is omitted entirely when no skills exist so
   a user without skills pays zero tokens.

   `Plugin::manifest()` — which is where `prompt_segments` are read — is
   synchronous, while `SkillIndex::catalog()` awaits a `tokio::sync::RwLock`
   read. `UserSkillsPlugin` therefore caches the last-rendered catalog string
   in a plain, synchronously-readable `Arc<std::sync::RwLock<Option<String>>>`
   (no new dependency — this is a single cached string, not a case for
   `arc_swap`), written by `on_event(HostStarting)` and
   `handle_slash("reload-skills")` right after `SkillIndex::replace(...)`,
   and read without an `.await` inside `manifest()`. `None` in the cache
   means no segment, matching `catalog()`'s own contract.

   Separately: today `Host::set_prompt_segments` is called exactly once, at
   TUI startup (`main.rs`, fed by `PluginRegistry::active_prompt_segments()`)
   — no existing `Effect` re-pushes segments into a *running* session, and
   `Effect::ReindexPlugin` (what `/reload-commands` etc. use) only
   recomputes slash/screen/keybinding indexes, never prompt segments. For
   `/reload-skills` to actually refresh what a live session's system prompt
   contains (not just the tool's enum and the index), this stream adds a
   small drain mirroring the existing `apply_pending_routing_reload`
   pattern (`main.rs`): a `pending_prompt_segments_reload` flag set from the
   effect handler, drained at the same points `apply_pending_routing_reload`
   is (outside any `RwLock` guard), which re-reads
   `registry.active_prompt_segments()` and calls
   `host.set_prompt_segments(...)` on the current host. This is new,
   general plumbing (not skill-specific) that happens to be needed here
   first, since no other built-in plugin has shipped a dynamically-changing
   prompt segment yet.
2. **Level 2 — on invocation.** The `skill` tool returns the full `SKILL.md`
   body as the tool result. Bodies never enter the system prompt.
3. **Level 3 — on demand.** Bundled `scripts/`, `references/`, `assets/`
   are ordinary files under the skill directory; the model reaches them
   with `tool-fs:read_file` / `tool-bash`. The `skill` tool result includes
   the absolute skill root so relative references resolve.

**Invocation.** Two entry points onto one code path:

- A built-in `skill` MCP tool — same shape as C's `task` tool. Registered
  the way `task` actually is: `UserSkillsPlugin` builds a `ToolDef` +
  `InProcessToolHandlerArc` and emits `Effect::RegisterInProcessTool`
  (mirroring `user_agents::register_task_tool_effects`), gated on the index
  being non-empty. **Not** `HostConfig::with_tool` — that path is for
  out-of-process stdio tool servers (`otto-tool-fs` and siblings) wired
  once in `main.rs`; an in-process tool goes through `ToolRegistry` via the
  `Effect`, exactly as Load-Bearing Invariant 5 requires. Takes
  `{ name: string }`, returns the body + skill root. The model calls it
  when a description matches the work at hand.
- `/skills` (no argument) — an enriched listing built from
  `SkillIndex::sorted_snapshot()`: name, truncated description, source
  path, and scope badge, sorted scope-then-name. `/skills <name>` looks the
  name up in the index and injects the full body + root directly (same
  payload shape as the tool), with near-match suggestions on an unknown
  name. This is what #83 asked for. A full interactive picker `Screen` is
  explicitly **not** built for this stream: the mirror target,
  `user_agents`, has no picker either, and neither AC1 nor AC2 requires
  interactivity — an enriched listing plus direct-injection-by-name
  satisfies both. Revisit as a deliberate scope addition later if wanted.
- `/reload-skills` — rescans all tiers without restart, mirroring
  `/reload-commands` / `/reload-hooks` / `/reload-agents`, and re-emits
  both the `skill` tool's `Effect::RegisterInProcessTool` (so its name enum
  stays live) and the level-1 catalog cache (so the system prompt segment
  stays live, per the plumbing described above).

**Tool scoping.** `allowed-tools` in a skill's frontmatter is advisory in
E1 and enforced in E4 by the same `ScopedToolRegistry` that already backs
subagent `tools:` scoping — one enforcement mechanism, three consumers
(agents, commands, skills).

**Trust.** A `SKILL.md` is inert markdown; loading one executes nothing, so
level 1 and level 2 need no gate. Level 3 is different: a project-local
skill that bundles `scripts/setup.sh` is asking the model to run code from
the repo. Reuse sub-project A's trust prompt and
`~/.otto/trusted-projects.json` — the first invocation of a *project-local*
skill whose directory contains any executable file or `scripts/` subdir
prompts for project trust. User-scope skills (`~/.claude`, `~/.otto`) are
trusted implicitly, exactly as user-scope commands are.

The two invocation surfaces are not equivalent here. The `skill` MCP tool
handler is a plain `InProcessToolHandler::call` returning
`Result<Value, String>` straight to the model — it has no `Effect` channel,
so on `NeedsConsent` it can only refuse and name the remedy (point the user
at `/skills <name>`), never show an interactive prompt. The actual
interactive consent flow (`Effect::OpenScreen{id: "trust.modal"}` +
`Effect::StashPendingSlash` + `Effect::SetTrustLevel`) only exists on the
slash-command dispatch path (`handle_slash`), so it is reachable **only**
from `/skills <name>`, reusing `user_slash_commands`'s existing trust modal
plumbing. A user who only ever calls the tool never sees a prompt for a
gated project-local skill — a refusal directing them to `/skills <name>` is
the correct behavior, not a bug.

**Module layout.** New built-in plugin `internal:user-skills` at
`crates/otto/src/plugin/builtin/user_skills/`:

| File | Role | Mirrors |
|---|---|---|
| `discovery.rs` | five-tier walk, precedence, dedup | `user_agents/discovery.rs` |
| `frontmatter.rs` | YAML parse, required/optional keys, warnings | `user_agents/frontmatter.rs` |
| `spec.rs` | `SkillSpec { name, description, allowed_tools, root, source, scope, body, has_bundled_executables }` | `user_agents/spec.rs` |
| `index.rs` | slug → spec map, catalog rendering, reload | `user_agents/index.rs` |
| `skill_tool.rs` | the built-in `skill` MCP tool | `user_agents/task_tool.rs` |
| `trust.rs` | level-3 trust gate, reusing `user_slash_commands::trust` | — |
| `mod.rs` | `Plugin` impl, slash specs, catalog cache, `/skills` listing + direct-injection, `/reload-skills`, hooks | `user_agents/mod.rs` |

(No `body.rs` — body extraction landed inside `frontmatter.rs` as shipped.
No `screen.rs` — see Invocation above.)

### E2 — Claude Code plugins

**Two plugin kinds, one registry.** otto's WASM plugins and Claude Code's
asset-bundle plugins are different artifacts serving different needs; they
coexist rather than converge.

**Breaking change (Non-Negotiable Rule 6).** otto's WASM discovery drops
its `.claude/plugins/` tiers and reads only:

```
<project>/.otto/plugins/<id>/plugin.toml
~/.otto/plugins/<id>/plugin.toml
```

The removed tiers never worked — a `plugin.toml` there was only ever
reachable by a user who hand-built the layout — so real-world breakage is
near-zero. It is still a documented public-surface change: named in this
spec, called out in `CHANGELOG.md`, flagged to the architecture reviewer,
and MINOR-bumped per the pre-1.0 SemVer convention in `RELEASING.md`.

**New loader** at `crates/otto/src/plugin/claude_code/`:

1. Read `~/.claude/plugins/installed_plugins.json`. Honor `version: 2`;
   an unrecognized `version` logs one warning and loads nothing (fail
   closed — the file is Claude Code's, and guessing at a future schema is
   how you load the wrong tree).
2. For each entry, take the newest install by `lastUpdated`, resolve
   `installPath`, and require it to exist and contain
   `.claude-plugin/plugin.json`. Missing or malformed entries warn and are
   skipped; one bad plugin never aborts the walk (D's rule).
3. Also walk `<project>/.claude/plugins/` and `<project>/.otto/plugins/`
   for *directory-shaped* Claude Code plugins (a `.claude-plugin/plugin.json`
   present), so a repo can vendor a plugin without a marketplace.
4. Parse `plugin.json` → `{ name, description, author, version }`.
5. Contribute each bundled asset directory into the index that already
   owns it, namespaced `<plugin>:<slug>`:

| Bundle dir | Consumer | Namespaced form |
|---|---|---|
| `commands/*.md` | sub-project A index | `/<plugin>:<command>` |
| `agents/*.md` | sub-project C index | `<plugin>:<agent>` as a `task` target |
| `skills/<n>/SKILL.md` | E1 index | `<plugin>:<skill>` |
| `hooks/hooks.json` | sub-project B per-event index | merged, lowest precedence |
| `.mcp.json` | `ToolRegistry` via the existing `/mcp` path | server name prefixed `<plugin>:` |

**Trust.** A Claude Code plugin can carry shell hooks and MCP server specs
— it executes code. Gate it exactly as D gates WASM: SHA-256 over the
install tree, a trust-prompt modal showing name/description/author/source
path/tree hash, and a record in `~/.otto/plugin-trust.toml`. Untrusted
plugins are discovered, listed in `/plugins` with an `untrusted` badge, and
contribute nothing. A tree-hash change re-arms the prompt.

**Surfacing.** The existing `/plugins` manager screen gains a `kind` column
(`wasm` / `claude-code`) and lists both. `/plugins trust|revoke|disable|enable
<id>` operate on either kind. `/plugins install <toml-url>` stays
WASM-only in v1 and says so when handed a marketplace URL.

### E3 — Hook parity

- **`SessionEnd`** — fires on clean TUI exit, before the `ToolRegistry`
  reaps stdio children. Cannot block.
- **`Notification`** — fires when otto surfaces a `PushNote`. Cannot block.
  Payload carries `message`.
- **`PostToolUse` matcher lift** — the v1 restriction to a `"*"` matcher
  goes away; `PostToolUse` gets the same glob matching `PreToolUse` has.
  This is the one hook gap with a real behavioral cost: a user's
  `"matcher": "Write"`-style rule silently does nothing today.
- **Plugin-supplied hooks** — E2's `hooks/hooks.json` merges into the same
  per-event index, ranked below all four `settings.json` tiers.
- **`${CLAUDE_PLUGIN_ROOT}`** — expanded in hook `command` strings to the
  owning plugin's install path, matching Claude Code. `${OTTO_PLUGIN_ROOT}`
  is accepted as a synonym. Unset for non-plugin hooks; an unexpanded
  occurrence there warns and is left literal.
- **`PreCompact`** — parsed and warned-on-use ("otto has no compaction
  pass; this hook will not fire") rather than silently dropped.

### E4 — Command parity

- **Enforce `allowed-tools`.** Wrap the turn's registry in
  `ScopedToolRegistry` for the duration of a command whose frontmatter sets
  it. Removes the README's standing "parsed but not yet enforced" caveat.
- **Plugin command namespacing.** E2's `commands/*.md` register as
  `/<plugin>:<command>`, matching Claude Code and matching A's existing
  subdirectory namespacing (`commands/team/lint.md` → `/team:lint`).
- **`disable-model-invocation`.** Honored as a frontmatter boolean; a
  command that sets it is user-invocable but never offered to the model.

## Tool-name divergence

The one place perfect compatibility is impossible. Claude Code's assets
name Claude Code's tools (`Read`, `Bash`, `Grep`, `Edit`); otto's model
sees `tool-fs:read_file`, `tool-bash:run`, `tool-grep:search`.

Three cases, three treatments:

1. **Prose in a body** ("use the Read tool to check…") — left alone. A
   capable model maps intent to otto's available tools. Translating prose
   is out of scope and would corrupt bodies.
2. **`allowed-tools` / `tools:` frontmatter** — a name that matches no
   registered otto tool is dropped from the allowlist with a warning at
   load time, not at dispatch time, so the user learns about it once. A
   frontmatter list that ends up empty after filtering is treated as
   "unset" rather than "deny everything" — failing open here matches what
   the author meant and matches C's existing behavior.
3. **Hook `matcher` globs** — matched against otto's names. A
   `"matcher": "Write"` rule simply never fires. E3's `/reload-hooks` output
   gains a line naming matchers that match no registered tool, so the
   mismatch is visible rather than mysterious.

A `[compat] tool_aliases` map in `~/.otto/config.toml` is the obvious
follow-up and is explicitly deferred.

## Alternatives considered

**Translate `.claude/` assets into `.otto/` on first run.** Rejected: it
forks the user's config. Two copies drift, and otto becomes responsible for
a migration it cannot keep current as Claude Code evolves.

**Implement Claude Code plugins as WASM plugins.** Rejected: they are
markdown and JSON, not code. Wrapping them in a WIT world buys nothing and
would force every marketplace plugin through a compile step.

**Skills as a system-prompt dump.** Rejected: a user with forty skills would
pay tens of thousands of tokens per turn. Progressive disclosure is the
whole design of the format.

**One mega-PR.** Rejected: E1 is a new subsystem with its own trust and
prompt-budget questions; E2 carries a breaking discovery change. Bundling
them makes the security review (Non-Negotiable Rule 5) unreviewable.

## Risks

| Risk | Mitigation |
|---|---|
| Level-1 catalog inflates every turn's prompt | Hard caps (200 skills, 1024-char descriptions), segment omitted when empty, token cost asserted in a test |
| `installed_plugins.json` schema drifts | Version-gated; unknown version loads nothing and warns once |
| Claude Code plugin hooks execute untrusted shell | Same SHA-256 + consent gate as D; untrusted plugins contribute nothing |
| Skill/agent/command slug collisions across tiers | One documented precedence chain, first-wins, collisions warn at load |
| Dropping `.claude/plugins/` from WASM discovery breaks someone | Near-zero real usage (path never worked); CHANGELOG + MINOR bump |
| Four streams drift from each other | One spec, one plan, sequential releases, each stream's PR references #83 |

## Acceptance criteria

1. `/skills` lists every skill found under all four tiers plus enabled
   plugins, with name, description, source, and scope.
2. `/skills <name>` and the model-invoked `skill` tool both inject the full
   `SKILL.md` body and expose the skill root for level-3 reads.
3. A user with zero skills sees no skills segment in the system prompt.
4. `/reload-skills`, `/reload-commands`, `/reload-hooks`, `/reload-agents`
   all rescan without restart.
5. With Claude Code plugins installed at `~/.claude/plugins/`, `/plugins`
   lists them with `kind = claude-code` and a trust badge; trusting one
   registers its commands, agents, skills, hooks, and MCP servers under
   `<plugin>:` namespaces.
6. otto's WASM plugin discovery no longer reads `.claude/plugins/`.
7. `PostToolUse` honors non-`*` matchers; `SessionEnd` and `Notification`
   fire; `PreCompact` warns instead of silently doing nothing.
8. A command with `allowed-tools` cannot dispatch outside its allowlist.
9. `README.md` documents skills, the Claude Code plugin format, the revised
   plugin discovery paths, and the tool-name divergence caveat.
10. `cargo test --workspace` green; `cargo clippy --workspace --all-targets`
    clean; a manual run confirms a real `~/.claude/` tree loads.

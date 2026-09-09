# issue-83-claude-code-compat Implementation Plan

**Goal:** Make an otto user's existing Claude Code setup work in otto unchanged — skills, plugins, commands, and hooks read from the same `~/.claude/` and `<project>/.claude/` trees Claude Code itself uses, per `savvagent/otto#83`.

**Architecture:** Four sequential work-streams, four PRs, four releases. E1 adds a skills subsystem modeled module-for-module on sub-project C's `user_agents` (four-tier discovery → slug index → built-in MCP tool → slash surface), with progressive disclosure through the existing `otto-plugin::prompt::SystemPromptSegment` seam. E2 adds a Claude Code plugin loader that fans a plugin's bundled `commands/`/`agents/`/`skills/`/`hooks/`/`.mcp.json` into the indexes A/B/C/E1 already own, and simultaneously drops otto's WASM discovery off `.claude/plugins/` (a documented breaking change). E3 and E4 close the remaining hook and command parity gaps. No new crates; everything lands in `crates/otto/src/plugin/` plus small reuse of `otto-host`'s `ScopedToolRegistry`.

**Tech Stack:** Existing workspace only — `crates/otto` (plugin registry, builtins, screens), `crates/otto-host` (`ScopedToolRegistry`, `ToolRegistry`, `HostConfig::with_tool`), `crates/otto-plugin` (`Plugin`, `Effect`, `SystemPromptSegment`), `crates/otto-plugin-wasm` (discovery paths only). `serde`/`serde_json`/`serde_yaml` and `sha2` are already workspace dependencies. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-09-issue-83-claude-code-compat-design.md` — read it first. This plan implements it exactly.

**Release line:** E1 → v0.27.0 · E2 → v0.28.0 · E3 → v0.28.1 · E4 → v0.28.2

**Branches:** `otto/issue-83-e1-skills` · `otto/issue-83-e2-claude-code-plugins` · `otto/issue-83-e3-hook-parity` · `otto/issue-83-e4-command-parity`

## File Map

**New files**
- `crates/otto/src/plugin/builtin/user_skills/mod.rs` — `internal:user-skills` plugin: `Plugin` impl, `/skills` + `/reload-skills` slash specs, level-1 prompt segment.
- `crates/otto/src/plugin/builtin/user_skills/discovery.rs` — four-tier walk, precedence, first-wins dedup by slug.
- `crates/otto/src/plugin/builtin/user_skills/frontmatter.rs` — YAML parse; `name`/`description` required-ish, `allowed-tools` optional, unknown keys warn.
- `crates/otto/src/plugin/builtin/user_skills/body.rs` — body extraction after the frontmatter fence.
- `crates/otto/src/plugin/builtin/user_skills/spec.rs` — `SkillSpec { name, description, allowed_tools, root, source, scope }`.
- `crates/otto/src/plugin/builtin/user_skills/index.rs` — slug → spec map, level-1 catalog rendering with caps, reload.
- `crates/otto/src/plugin/builtin/user_skills/skill_tool.rs` — the built-in `skill` MCP tool.
- `crates/otto/src/plugin/builtin/user_skills/screen.rs` — the `/skills` picker screen.
- `crates/otto/src/plugin/builtin/user_skills/trust.rs` — level-3 project-trust gate for skills bundling executables.
- `crates/otto/src/plugin/claude_code/mod.rs` — Claude Code plugin loader entry point.
- `crates/otto/src/plugin/claude_code/installed.rs` — `~/.claude/plugins/installed_plugins.json` v2 parsing, newest-install selection.
- `crates/otto/src/plugin/claude_code/manifest.rs` — `.claude-plugin/plugin.json` parsing.
- `crates/otto/src/plugin/claude_code/bundle.rs` — fan `commands/`/`agents/`/`skills/`/`hooks/hooks.json`/`.mcp.json` into their owning indexes, namespaced.
- `crates/otto/src/plugin/claude_code/trust.rs` — SHA-256 tree hash + `~/.otto/plugin-trust.toml` reuse.
- `docs/superpowers/specs/2026-09-09-issue-83-claude-code-compat-design.md` — design source of truth.
- `docs/superpowers/plans/2026-09-09-issue-83-claude-code-compat.md` — this plan.

**Modified files**
- `crates/otto/src/plugin/builtin/mod.rs` — register `internal:user-skills`.
- `crates/otto/src/main.rs` — register the built-in `skill` tool via `HostConfig::with_tool`.
- `crates/otto/src/plugin/manifests.rs` — index the new slash specs and prompt segment.
- `crates/otto-plugin-wasm/src/discovery.rs` — drop the two `.claude/plugins/` tiers.
- `crates/otto-plugin-wasm/src/register.rs` — update doc comments and tests for the two-tier walk.
- `crates/otto/src/plugin/external.rs` — same, plus load Claude Code plugins alongside WASM ones.
- `crates/otto/src/plugin/builtin/plugins_manager/` — `kind` column, trust/revoke across both plugin kinds.
- `crates/otto/src/plugin/builtin/user_hooks/config.rs` — accept `SessionEnd`, `Notification`, `PreCompact`.
- `crates/otto/src/plugin/builtin/user_hooks/mod.rs` — dispatch `SessionEnd` / `Notification`; lift the `PostToolUse` `*`-only restriction.
- `crates/otto/src/plugin/builtin/user_hooks/runner.rs` — `${CLAUDE_PLUGIN_ROOT}` / `${OTTO_PLUGIN_ROOT}` expansion.
- `crates/otto/src/plugin/builtin/user_hooks/discovery.rs` — merge plugin-supplied `hooks.json` at lowest precedence.
- `crates/otto/src/plugin/builtin/user_slash_commands/frontmatter.rs` — `disable-model-invocation`.
- `crates/otto/src/plugin/builtin/user_slash_commands/mod.rs` — enforce `allowed-tools` via `ScopedToolRegistry`; plugin namespacing.
- `crates/otto/src/plugin/builtin/user_agents/discovery.rs` — accept plugin-supplied agents.
- `README.md` — skills section, Claude Code plugin format, revised discovery paths, tool-name divergence caveat, new on-disk paths.
- `PRD.md` — sub-project E in the roadmap.
- `CHANGELOG.md` — per-release entries, including the E2 breaking change.

---

## Stream E1 — Skills (v0.27.0, branch `otto/issue-83-e1-skills`)

### Task 1: Skill discovery, frontmatter, and index

**Files:**
- Create: `crates/otto/src/plugin/builtin/user_skills/discovery.rs`, `frontmatter.rs`, `body.rs`, `spec.rs`, `index.rs`

- [ ] Read `crates/otto/src/plugin/builtin/user_agents/{discovery,frontmatter,body,spec,index}.rs` end to end and confirm the four-tier precedence helper, the warning-collection pattern, and the first-wins dedup are reusable shapes rather than shared code worth extracting on this pass.
- [ ] Add failing tests for discovery: all four tiers found; project `.otto/` beats project `.claude/` beats user `.otto/` beats user `.claude/`; a directory without `SKILL.md` is skipped silently; a slug appearing in two tiers resolves to the higher tier and emits one collision warning. Run `cargo test -p otto user_skills::discovery -- --nocapture` and expect failures.
- [ ] Add failing tests for frontmatter: `name` + `description` parse; missing `description` is a hard error naming the file; missing `name` defaults to the directory slug with a warning; `name` mismatching the slug warns but keeps the slug as the index key; `allowed-tools` parses as a comma-separated list; unknown keys (`license`, `metadata`) warn and are ignored, never fatal. Run `cargo test -p otto user_skills::frontmatter -- --nocapture` and expect failures.
- [ ] Implement `SkillSpec`, the frontmatter parser, the body extractor, and the four-tier discovery walk. Malformed skills warn and are skipped — one bad skill never aborts the walk (sub-project D's rule).
- [ ] Add failing tests for the level-1 catalog in `index.rs`: descriptions truncate at 1024 chars; the catalog caps at 200 skills and warns when it truncates; an empty index renders `None` rather than an empty segment. Implement `SkillIndex` and the catalog renderer. Run `cargo test -p otto user_skills::index -- --nocapture` and expect them to pass.
- [ ] Record the public-interface check in code review notes: this task adds no SPP wire-format, tool-schema, plugin-ABI, slash-command, env-var, or on-disk-format change — discovery and parsing only.
- [ ] Run `cargo fmt --all`, `cargo test -p otto user_skills -- --nocapture`, and commit with `git commit -m "otto: discover and index user skills"`.

### Task 2: The built-in `skill` tool and progressive disclosure

**Files:**
- Create: `crates/otto/src/plugin/builtin/user_skills/skill_tool.rs`, `trust.rs`
- Modify: `crates/otto/src/main.rs`

- [ ] Read `crates/otto/src/plugin/builtin/user_agents/task_tool.rs` and `crates/otto-host/src/config.rs`'s `with_tool` path, and confirm an in-process tool handler is the right registration shape for `skill` (it is what `task` uses).
- [ ] Add failing tests: `skill` reports an input schema of `{ name: string }`; invoking a known slug returns the full `SKILL.md` body plus the absolute skill root; invoking an unknown slug returns a tool error naming the closest matching slugs rather than a bare "not found"; the body is returned as a tool result and never appended to the system prompt. Run `cargo test -p otto user_skills::skill_tool -- --nocapture` and expect failures.
- [ ] Implement the `skill` tool and register it in `crates/otto/src/main.rs` via `HostConfig::with_tool`.
- [ ] Add failing tests for the level-3 trust gate in `trust.rs`: a project-local skill whose directory contains an executable file or a `scripts/` subdir prompts for project trust on first invocation; a user-scope skill never prompts; a stored "trust always" decision in `~/.otto/trusted-projects.json` suppresses the prompt. Implement it by reusing sub-project A's `user_slash_commands::trust` helpers rather than a second trust file.
- [ ] Verify explicitly that the tool handler holds no host-swap `RwLock` guard across an `.await`, and note it in review — the handler reads the index and touches the filesystem, so it must clone what it needs and drop any guard first.
- [ ] Record the public-interface check: this task ADDS a built-in tool name (`skill`) to the surface the model sees. Additive, no gate needed under Non-Negotiable Rule 6, but it must appear in `README.md` and `CHANGELOG.md`.
- [ ] Run `cargo fmt --all`, `cargo build`, `cargo test -p otto user_skills -- --nocapture`, and commit with `git commit -m "otto: add built-in skill tool with progressive disclosure"`.

### Task 3: `/skills`, `/reload-skills`, and the picker screen

**Files:**
- Create: `crates/otto/src/plugin/builtin/user_skills/screen.rs`, `mod.rs`
- Modify: `crates/otto/src/plugin/builtin/mod.rs`, `crates/otto/src/plugin/manifests.rs`

- [ ] Read `crates/otto/src/plugin/builtin/command_palette/screen.rs` and `crates/otto/src/plugin/builtin/user_agents/mod.rs` to confirm the `Screen` + `SlashSpec` shapes and how a builtin contributes a `SystemPromptSegment`.
- [ ] Add failing tests: `/skills` with no argument opens the picker; the picker lists name, truncated description, source path, and scope badge, sorted by scope then name; `/skills <name>` bypasses the picker and injects directly; `/skills <unknown>` surfaces a `PushNote` naming near-matches; `/reload-skills` rescans and reports the new count. Run `cargo test -p otto user_skills::screen -- --nocapture` and expect failures.
- [ ] Add a failing test asserting the plugin emits a `SystemPromptSegment` with id `internal:user-skills:catalog` when skills exist and emits **no** segment when the index is empty, so a user with zero skills pays zero tokens (spec acceptance criterion 3).
- [ ] Implement `screen.rs` and `mod.rs`, register `internal:user-skills` in `builtin/mod.rs`, and index its slash specs in `manifests.rs`.
- [ ] Run `cargo build` (required — the TUI spawns `otto-tool-fs` at runtime) and launch `cargo run -p otto` against a scratch `HOME` containing a synthetic `~/.claude/skills/` tree; confirm `/skills` lists them, one injects, and `/reload-skills` picks up a file added mid-session.
- [ ] Record the public-interface check: ADDS `/skills` and `/reload-skills` to the documented slash-command surface. Additive; document in `README.md`.
- [ ] Run `cargo fmt --all`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, and commit with `git commit -m "otto: add /skills picker and reload"`.

### Task 4: Documentation and E1 close-out

**Files:**
- Modify: `README.md`, `PRD.md`

- [ ] Add a "User-defined skills" section to `README.md` mirroring the existing commands/hooks/agents sections: the four discovery paths, the `SKILL.md` format with a worked example, the three disclosure levels, `/skills` / `/reload-skills`, the level-3 trust prompt, and the new `~/.otto/skills/` + `.otto/skills/` rows in the on-disk paths table.
- [ ] Add the tool-name divergence caveat to `README.md` — Claude Code assets name Claude Code tools; prose is left alone, frontmatter tool names that match nothing are dropped with a load-time warning, and a `[compat] tool_aliases` map is a deferred follow-up.
- [ ] Add sub-project E to `PRD.md`'s roadmap with E1 marked shipped and E2–E4 pending.
- [ ] Flip the spec's `Status:` line for the E1 portion and tick this stream's checkboxes in place — no archive directory, per house style.
- [ ] Open the PR against `main` with a body referencing `savvagent/otto#83`, and run the three required review passes (Rust-expert, architecture, and an independent diff-only `security-review` that receives **only** the PR diff — no spec, no plan, no summary).
- [ ] After merge, cut the dedicated v0.27.0 release PR per `RELEASING.md` (version bump across `workspace.package.version` and every internal `workspace.dependencies` entry, `CHANGELOG.md`, tag, GitHub Release).

---

## Stream E2 — Claude Code plugins (v0.28.0, branch `otto/issue-83-e2-claude-code-plugins`)

### Task 5: Move otto's WASM discovery off `.claude/plugins/`

**Files:**
- Modify: `crates/otto-plugin-wasm/src/discovery.rs`, `crates/otto-plugin-wasm/src/register.rs`, `crates/otto/src/plugin/external.rs`

- [ ] Read `crates/otto-plugin-wasm/src/discovery.rs` and confirm the two `.claude/plugins/` tiers are only ever reachable by a hand-built layout — Claude Code's own `~/.claude/plugins/` holds `config.json`, `installed_plugins.json`, `marketplaces/`, `cache/`, and `repos/`, none of which contain a `plugin.toml`.
- [ ] Add failing tests asserting the walk covers exactly `<project>/.otto/plugins/` and `~/.otto/plugins/`, and that a `plugin.toml` placed under `.claude/plugins/` is **not** picked up by the WASM loader. Run `cargo test -p otto-plugin-wasm discovery -- --nocapture` and expect failures.
- [ ] Remove the two tiers, update the module doc comments in `discovery.rs` / `register.rs` / `external.rs`, and fix the tests that assumed four tiers.
- [ ] Record the public-interface check: this is a **BREAKING** change to the documented plugin discovery surface under Non-Negotiable Rule 6. It must be named explicitly in the PR body, flagged to the architecture reviewer, written into `CHANGELOG.md` under a "Breaking" heading, and carried by the MINOR bump this repo's pre-1.0 SemVer convention requires.
- [ ] Run `cargo fmt --all`, `cargo test -p otto-plugin-wasm`, and commit with `git commit -m "otto-plugin-wasm: scope wasm plugin discovery to .otto/plugins"`.

### Task 6: Read installed Claude Code plugins

**Files:**
- Create: `crates/otto/src/plugin/claude_code/{mod.rs,installed.rs,manifest.rs,trust.rs}`

- [ ] Read a real installation to ground the parser: `~/.claude/plugins/installed_plugins.json` (`version: 2`, `plugins: { "<name>@<marketplace>": [ { scope, installPath, version, installedAt, lastUpdated } ] }`) and an install tree's `.claude-plugin/plugin.json` (`{ name, description, author, version? }`).
- [ ] Add failing tests for `installed.rs`: a v2 file parses; multiple installs of one plugin resolve to the newest by `lastUpdated`; an entry whose `installPath` is missing or lacks `.claude-plugin/plugin.json` warns and is skipped without aborting the walk; an unrecognized top-level `version` loads nothing and warns exactly once (fail closed). Run `cargo test -p otto claude_code::installed -- --nocapture` and expect failures.
- [ ] Add failing tests for `manifest.rs`: a minimal `plugin.json` with only `name` parses; `description`/`author`/`version` are optional; malformed JSON warns and skips.
- [ ] Add failing tests for project-local directory-shaped plugins: `<project>/.claude/plugins/<id>/.claude-plugin/plugin.json` and `<project>/.otto/plugins/<id>/.claude-plugin/plugin.json` are discovered without any marketplace involvement, and are distinguished from WASM plugins by the presence of `.claude-plugin/plugin.json` rather than `plugin.toml`.
- [ ] Implement `installed.rs`, `manifest.rs`, and the `mod.rs` entry point.
- [ ] Add failing tests for `trust.rs`: a SHA-256 tree hash over the install path is stable across reads and changes when any bundled file changes; an untrusted plugin contributes nothing; a trust record round-trips through `~/.otto/plugin-trust.toml`; a tree-hash change re-arms the prompt. Implement by reusing sub-project D's trust-file helpers rather than a parallel file.
- [ ] Verify explicitly that otto never writes to `~/.claude/` — assert it in a test that snapshots the tree before and after a load.
- [ ] Run `cargo fmt --all`, `cargo test -p otto claude_code -- --nocapture`, and commit with `git commit -m "otto: read installed Claude Code plugins"`.

### Task 7: Fan plugin bundles into the existing indexes

**Files:**
- Create: `crates/otto/src/plugin/claude_code/bundle.rs`
- Modify: `crates/otto/src/plugin/builtin/user_slash_commands/discovery.rs`, `user_agents/discovery.rs`, `user_skills/discovery.rs`, `user_hooks/discovery.rs`, `crates/otto/src/plugin/builtin/plugins_manager/`

- [ ] Add failing tests, one per bundle directory, asserting namespaced registration and precedence *below* all four filesystem tiers: `commands/*.md` → `/<plugin>:<command>`; `agents/*.md` → `<plugin>:<agent>` as a `task` target; `skills/<n>/SKILL.md` → `<plugin>:<skill>`; `hooks/hooks.json` merged into the per-event index; `.mcp.json` servers registered through the existing `/mcp` path with a `<plugin>:` name prefix. Run `cargo test -p otto claude_code::bundle -- --nocapture` and expect failures.
- [ ] Add a failing test asserting a user-authored skill/command/agent slug **shadows** a plugin-supplied one of the same name, and that the shadowing emits one warning.
- [ ] Implement `bundle.rs` and the small discovery-side hooks each index needs to accept a plugin-supplied tier.
- [ ] Update the `/plugins` manager screen: add a `kind` column (`wasm` / `claude-code`), list both kinds, and make `trust` / `revoke` / `enable` / `disable` operate on either. `/plugins install <toml-url>` stays WASM-only and must say so explicitly when handed a marketplace URL rather than failing obscurely.
- [ ] Verify that MCP servers contributed by a plugin are dispatched through `ToolRegistry` like every other tool server, never bypassed, and that stdio children are reaped on shutdown — note both in review (Non-Negotiable Rule 7).
- [ ] Run `cargo build` and launch `cargo run -p otto` against a real `~/.claude/plugins/` tree; confirm `/plugins` lists the installed plugins, a trust prompt appears, and after trusting, the plugin's commands appear under `/<plugin>:<command>` and its skills under `/skills`.
- [ ] Record the public-interface check: ADDS namespaced slash commands, `task` targets, skills, and MCP server names. Additive. Document the whole Claude Code plugin format in `README.md`.
- [ ] Run `cargo fmt --all`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, and commit with `git commit -m "otto: register Claude Code plugin bundles"`.

### Task 8: E2 documentation and close-out

**Files:**
- Modify: `README.md`, `PRD.md`, `CHANGELOG.md`

- [ ] Rewrite `README.md`'s plugin section to cover both kinds: otto's WASM format (now `.otto/plugins/` only) and the Claude Code format (`installed_plugins.json` + install trees + project-local `.claude-plugin/plugin.json`), with the bundle-directory table and the trust model.
- [ ] Add the breaking-change note for WASM discovery to `CHANGELOG.md` under an explicit "Breaking" heading.
- [ ] Update the on-disk paths table in `README.md` with every new path this stream reads.
- [ ] Open the PR referencing `savvagent/otto#83`, run the three required review passes with the security pass receiving only the diff, and after merge cut the v0.28.0 release PR per `RELEASING.md`.

---

## Stream E3 — Hook parity (v0.28.1, branch `otto/issue-83-e3-hook-parity`)

### Task 9: Lift the `PostToolUse` matcher restriction

**Files:**
- Modify: `crates/otto/src/plugin/builtin/user_hooks/mod.rs`, `matcher.rs`

- [ ] Read the struct-level "v1 limitations on `PostToolUse`" doc comment in `user_hooks/mod.rs` and the glob implementation in `matcher.rs`, and confirm `PreToolUse`'s matcher path is directly reusable.
- [ ] Add failing tests: a `PostToolUse` group with `"matcher": "tool-fs:*"` fires for `tool-fs:write_file` and not for `tool-bash:run`; an exact-name matcher fires only on that tool; `"*"` keeps its current behavior; a matcher that matches no registered tool is reported once at load rather than silently dropping. Run `cargo test -p otto user_hooks -- --nocapture` and expect failures.
- [ ] Implement the lift, delete the stale limitation doc comment, and update `README.md`'s hook events table (its `PostToolUse` row currently says `*` matcher only).
- [ ] Run `cargo fmt --all`, `cargo test -p otto user_hooks`, and commit with `git commit -m "otto: honor PostToolUse matchers"`.

### Task 10: `SessionEnd`, `Notification`, `PreCompact`, and plugin hooks

**Files:**
- Modify: `crates/otto/src/plugin/builtin/user_hooks/{config.rs,mod.rs,runner.rs,discovery.rs,payload.rs}`

- [ ] Add failing tests for `SessionEnd`: fires once on clean TUI exit, before `ToolRegistry` reaps its stdio children; cannot block; a `Block` outcome is demoted to a warning like `SessionStart`'s.
- [ ] Add failing tests for `Notification`: fires when otto surfaces a `PushNote`; payload carries `message` alongside the standard `session_id` / `transcript_path` / `cwd` / `hook_event_name` fields; cannot block.
- [ ] Add a failing test for `PreCompact`: the event parses from `settings.json` and emits exactly one warning at load ("otto has no compaction pass; this hook will not fire") rather than being silently dropped. otto has no compaction today — `grep -i compact crates/` is empty — so warning is the honest behavior.
- [ ] Add failing tests for `${CLAUDE_PLUGIN_ROOT}` expansion in `runner.rs`: expands to the owning plugin's install path for a plugin-supplied hook; `${OTTO_PLUGIN_ROOT}` is accepted as a synonym; an occurrence in a non-plugin hook warns once and is left literal rather than expanding to an empty string (which would silently produce a wrong path).
- [ ] Add a failing test that a plugin's `hooks/hooks.json` merges into the per-event index below all four `settings.json` tiers.
- [ ] Implement all five, then run `cargo build` and confirm in a live session that a `SessionEnd` hook fires on `/exit`.
- [ ] Record the public-interface check: ADDS hook event names to the documented surface. Additive. Update `README.md`'s events table with the three new rows and their blocking semantics.
- [ ] Run `cargo fmt --all`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, and commit with `git commit -m "otto: add SessionEnd, Notification, and plugin-supplied hooks"`.
- [ ] Open the PR referencing `savvagent/otto#83`, run the three required review passes, and after merge cut the v0.28.1 release PR.

---

## Stream E4 — Command parity (v0.28.2, branch `otto/issue-83-e4-command-parity`)

### Task 11: Enforce `allowed-tools` and honor `disable-model-invocation`

**Files:**
- Modify: `crates/otto/src/plugin/builtin/user_slash_commands/{frontmatter.rs,mod.rs}`, `crates/otto/src/plugin/builtin/user_skills/mod.rs`

- [ ] Read `crates/otto-host/src/scoped_registry.rs` and sub-project C's `user_agents::task_tool` scoping path, and confirm `ScopedToolRegistry` can wrap the turn registry for a slash-command turn without a host-swap lock being held across an `.await`.
- [ ] Add failing tests: a command declaring `allowed-tools: tool-fs:read_file` cannot dispatch `tool-bash:run` during its turn; an unset `allowed-tools` leaves the registry unscoped; a frontmatter list naming only unknown tools filters to empty and is treated as **unset** rather than deny-everything, with a load-time warning naming the dropped entries (failing open matches author intent and matches C's existing behavior).
- [ ] Add the same enforcement and the same tests for a skill's `allowed-tools`, so agents, commands, and skills share one mechanism.
- [ ] Add failing tests for `disable-model-invocation: true`: the command stays user-invocable via `/name` but is absent from whatever surface the model sees.
- [ ] Implement both, and delete the README's standing "`allowed-tools` — parsed but not yet enforced; reserved for the upcoming agents sub-project" caveat.
- [ ] Verify explicitly that scoping wraps `ToolRegistry` rather than bypassing it, and that no `RwLock` guard is held across an `.await` — note both in review (Non-Negotiable Rule 7).
- [ ] Run `cargo fmt --all`, `cargo test -p otto -- --nocapture`, and commit with `git commit -m "otto: enforce allowed-tools for commands and skills"`.

### Task 12: E4 close-out and issue resolution

**Files:**
- Modify: `README.md`, `PRD.md`, `CHANGELOG.md`, `docs/superpowers/specs/2026-09-09-issue-83-claude-code-compat-design.md`

- [ ] Walk the spec's ten acceptance criteria one at a time against a live `cargo run -p otto` session backed by a real `~/.claude/` tree — skills listed and injected, plugins listed and trusted, namespaced commands present, hooks firing, zero-skill prompt clean — and record the result of each.
- [ ] Flip the spec's `Status:` to IMPLEMENTED and tick every checkbox in this plan in place.
- [ ] Update `PRD.md` to mark sub-project E complete.
- [ ] Open the PR referencing `savvagent/otto#83`, run the three required review passes, and after merge cut the v0.28.2 release PR.
- [ ] Close `savvagent/otto#83` with a comment listing the four PRs, the four releases, and the deferred follow-ups: marketplace install for the Claude Code format, `[compat] tool_aliases`, `PreCompact` once otto has compaction, and the unread `settings.json` keys.

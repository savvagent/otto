# issue-83-skills-command Implementation Plan

**Goal:** Add a built-in `/skills` slash command that lists the repository skills Otto can discover from the current project, including each skill's name, description, and source location, so users can inspect the supported skill set without opening the tree manually.

**Architecture:** The change stays inside `crates/otto`'s built-in plugin layer. A new `internal:user-skills` plugin will discover `SKILL.md` files under the repo-documented project roots (`.github/skills/*/SKILL.md` and `.claude/skills/*/SKILL.md`), parse their frontmatter, and emit note-based slash output. The command remains a lightweight plugin contribution that reuses the existing slash router and command-palette indexing path; it does not alter `otto-host`, the `task` tool, agent discovery, or any provider/tool transport boundary.

**Tech Stack:** Rust 2024 in `crates/otto`, `serde`/`serde_yaml_ng` frontmatter parsing, `ignore::WalkBuilder` filesystem discovery, the existing plugin manifest/effect system, and Markdown docs in `README.md`.

**Spec:** `docs/superpowers/specs/2026-09-08-issue-83-skills-command-design.md` — committed on this branch; read it first. This plan implements it exactly.

**Release line:** `v0.27.0`

**Branch:** `otto/issue-83-skills-command`

## File Map

**New files**
- `crates/otto/src/plugin/builtin/user_skills/mod.rs` — built-in `/skills` plugin and slash-output behavior.
- `crates/otto/src/plugin/builtin/user_skills/discovery.rs` — best-effort project skill discovery for `.github/skills/*/SKILL.md` and `.claude/skills/*/SKILL.md`.
- `crates/otto/src/plugin/builtin/user_skills/frontmatter.rs` — YAML frontmatter parsing for skill `name`/`description`.
- `crates/otto/src/plugin/builtin/user_skills/spec.rs` — parsed skill metadata and source-location enum.

**Modified files**
- `crates/otto/src/plugin/builtin/mod.rs` — register the new built-in plugin module.
- `crates/otto/src/plugin/mod.rs` — instantiate `internal:user-skills` in the built-in plugin set and keep the registry completeness test current.
- `README.md` — document `/skills` in the slash-command list and explain which project skill roots it reports.

## Task 1: Implement repo-skill discovery and the `/skills` built-in slash command

**Files:**
- Create: `crates/otto/src/plugin/builtin/user_skills/mod.rs`
- Create: `crates/otto/src/plugin/builtin/user_skills/discovery.rs`
- Create: `crates/otto/src/plugin/builtin/user_skills/frontmatter.rs`
- Create: `crates/otto/src/plugin/builtin/user_skills/spec.rs`
- Modify: `crates/otto/src/plugin/builtin/mod.rs`
- Modify: `crates/otto/src/plugin/mod.rs`

- [ ] Write failing unit tests in `crates/otto/src/plugin/builtin/user_skills/` covering: discovery from both `.github/skills/*/SKILL.md` and `.claude/skills/*/SKILL.md`, deterministic sorting, the empty-state `no skills discovered` path, best-effort skipping of malformed files, frontmatter-name mismatch fallback to the directory slug, duplicate names across both locations remaining visible as separate entries, non-`SKILL.md` sibling files being ignored, and slash output that includes `name`, `location`, and `description`. Expected result: `cargo test -p otto user_skills` fails before implementation because the new plugin/module does not exist yet.
- [ ] Implement the new `internal:user-skills` plugin plus discovery/parser helpers so `/skills` is contributed through the plugin manifest, rescans the project roots on each invocation, and emits note lines in the spec'd format `skills: <count> discovered` then `- <name> [<location>] — <description>`. Expected result: the built-in slash router can resolve `/skills`, and the implementation reads actual `SKILL.md` files instead of using a hardcoded list.
- [ ] Register `internal:user-skills` in `crates/otto/src/plugin/builtin/mod.rs` and `crates/otto/src/plugin/mod.rs`, and update any built-in-registry assertions that enumerate plugin ids. Expected result: the command palette/runtime indexes include `/skills` automatically via manifest indexing.
- [ ] Run `cargo test -p otto user_skills`. Expected result: the new unit coverage passes.
- [ ] Public-interface check: record in the task ledger and PR body that `/skills` is an additive slash-command-surface change; no SPP/tool-schema/plugin-ABI/on-disk format change was introduced.
- [ ] Host-swap/RwLock check: not applicable — no `crates/otto/src/app.rs` or `crates/otto/src/tui.rs` change.
- [ ] ProgressDispatcher check: not applicable — no streaming provider path touched.
- [ ] Format and commit: run `cargo fmt --all` and `git commit -m "otto: add /skills slash command"`.

## Task 2: Document `/skills` and run targeted + workspace validation

**Files:**
- Modify: `README.md`
- Modify only if Task 1 validation exposes issue-83-related regressions.

- [ ] Update `README.md`'s slash-command list to add `/skills`, documenting that it lists the skills discovered from the current project's `.github/skills/*/SKILL.md` and `.claude/skills/*/SKILL.md` trees, with each skill's name, description, and location. Expected result: the new public slash surface is documented where developers look for commands.
- [ ] Update the `README.md` skills/agents documentation near `## User-defined agents` to clarify that `/skills` reports repo-authored skills from `.github/skills/` and `.claude/skills/`, while `/reload-agents` remains the separate subagent surface. Expected result: the docs distinguish skills from agents and mirror the premise correction from the spec.
- [ ] Run `cargo build --workspace --all-targets`. Expected result: success.
- [ ] Run `cargo test --workspace --no-fail-fast`. Expected result: success.
- [ ] Run `cargo clippy --workspace --all-targets`. Expected result: success.
- [ ] Run `cargo fmt --all --check`. Expected result: success.
- [ ] Public-interface check: confirm the README documents the additive `/skills` slash command and note that the release PR must add the corresponding `CHANGELOG.md` entry for `v0.27.0`.
- [ ] Host-swap/RwLock check: verify no task introduced an `.await` while holding an `Arc<RwLock<Option<Arc<Host>>>>` guard.
- [ ] ProgressDispatcher check: verify no streaming-provider path was touched, so no forwarder-abort change was required.
- [ ] Format and commit: if Task 2 changes or validation fixes are needed, run `cargo fmt --all` and `git commit -m "docs: document /skills command"`; otherwise record in the task ledger that no extra code commit was required.

## Task 3: Note the mandatory post-merge release PR

**Files:**
- No code changes in this branch; this task documents the release handoff only.

- [ ] Record that after this PR merges, the orchestrating release flow must open a dedicated release PR that bumps `workspace.package.version` and internal `workspace.dependencies` versions in `Cargo.toml`, adds the `/skills` entry to `CHANGELOG.md` under `v0.27.0`, runs the release validation commands from `RELEASING.md`, and publishes tag `v0.27.0`.
- [ ] Public-interface check: confirm the release PR needs a MINOR bump because `/skills` is an additive new user-facing slash command.
- [ ] Host-swap/RwLock check: not applicable.
- [ ] ProgressDispatcher check: not applicable.
- [ ] Format and commit: no additional commit required unless the plan itself is revised.

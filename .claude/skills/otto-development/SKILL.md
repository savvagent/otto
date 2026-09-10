---
name: otto-development
description: Use when developing any feature or fix in the otto repository (the Rust-only, MCP-first terminal coding agent) end-to-end — from a GitHub issue or plain task brief, through ship and verify, fully autonomously with no mid-run questions. Bundles the plan-by-plan discipline (committed design specs in docs/superpowers/specs/ and implementation plans in docs/superpowers/plans/), the Rust workspace conventions (everything-is-MCP-shaped, the host/tool/provider crate boundaries, the host-swap RwLock rule, the provider transport split), autonomous spec generation, plan generation, task-by-task implementation, PR review loops, verification, release cutting, and close-out. For other repositories use general-development.
---

> **Canonical body:** `.github/skills/otto-development/SKILL.md` and `.github/skills/otto-development/agent-prompts.md`

## Read this first

Load the canonical `SKILL.md` before taking any action, and load
`agent-prompts.md` whenever a phase calls for a dispatch template — the
canonical body requires those templates be pasted verbatim.

This stub is **not a summary and must not be acted on alone.** Every
Non-Negotiable Rule, phase gate, fix-loop cap and review requirement lives in
the canonical body; nothing here replaces or overrides any of them. All this
file adds is the host-mechanism translation below.

## Copilot → Claude Code host-mechanism translation

The canonical body is written for the orchestrating GitHub Copilot CLI and says
so at its own `SKILL.md:360-361`, naming dispatch tools Claude Code does not
have. Read every such mechanism through this table:

| # | Canonical body says | In Claude Code |
| --- | --- | --- |
| 1 | the `task` tool | the `Agent` tool |
| 2 | `agent_type: "general-purpose"` | `subagent_type: "general-purpose"` |
| 3 | `agent_type: "task"` | `subagent_type: "general-purpose"` — Copilot's generic worker has no distinct Claude Code counterpart |
| 4 | `agent_type: "rubber-duck"` (spec/plan critique) | `subagent_type: "general-purpose"`; no built-in equivalent, so the critique prompt carries the role |
| 5 | `agent_type: "code-review"` | the `/code-review` skill, or `subagent_type: "general-purpose"` with the canonical template |
| 6 | `agent_type: "security-review"` | the built-in `security-review` skill |
| 7 | `agent_type: "explore"` | `subagent_type: "Explore"` |
| 8 | `agent_type: "research"` | `subagent_type: "Explore"` |
| 9 | `mode: "sync"` | a single `Agent` call — it returns the report |
| 10 | `mode: "background"` | several `Agent` calls in one message; results arrive as task notifications |
| 11 | `read_agent` | the task-completion notification, or `SendMessage` to the named agent |
| 12 | the `sql` tool's `todos` table | `TodoWrite` |
| 13 | `ask_user` | `AskUserQuestion` — still overridden for autonomy per the canonical body |

Rows 3, 7 and 8 complete the canonical body's seven-value `agent_type` enum
(`SKILL.md:370-371`).

### Model selection

The canonical body's "Model selection" paragraph (`SKILL.md:390-393`) reaches
for `reasoning_effort`, which Claude Code's `Agent` tool does not have (it has
`model` only). Resolve it as follows:

- **Mechanical, 1–2-file task:** pass `model: "haiku"`.
- **Multi-file integration or design-judgment work:** omit `model` to inherit
  the session default, and pass `model: "opus"` explicitly if that default is
  known to be weaker. An omitted `model` resolves to the agent definition's
  model, then the configured default subagent model, and only then the
  parent's — so omission alone is not a guarantee of strong reasoning.

Every `reasoning_effort` instruction in the canonical body is satisfied by that
choice; there is no such knob to turn here.

### Built-ins only

The table maps only to **built-in** Claude Code agent types and skills, so it
holds in a fresh clone. A contributor whose `~/.claude/agents/` provides
specialised agents (`rust-pro`, `architect-reviewer`, `code-reviewer`,
`security-auditor`) may use those for the mandatory review trio as an
**optional upgrade** — never a requirement.

## Two rules that are easiest to lose in translation

Both are host-mechanism-shaped rather than workflow-shaped, so they are called
out here. Each is a pointer to the canonical rule, not a replacement for it:

- **Dispatch prompts must be fully self-contained.** Paste the actual
  spec/plan/task text inline — a Claude Code subagent has full repo filesystem
  and tool access but no access to this session's context.
- **The security review receives only the PR diff** — never the spec, plan,
  brief, PR body, or implementer report (canonical Non-Negotiable Rule 5).

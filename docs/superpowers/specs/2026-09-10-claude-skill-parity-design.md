# Claude Code parity for this repo's `.github/skills/` skills — design

Date: 2026-09-10
Status: pending review
Source: savvagent/otto#128
Related: savvagent/otto#23 (decided the *policy* for `.claude/skills/`; this closes the content gap it left), `CLAUDE.md:87-95` ("Claude Code skills")

## Problem

This repo commits two skill directories with disjoint content:

| Location | Format | Skills |
| --- | --- | --- |
| `.github/skills/` | Copilot CLI | `otto-development`, `creating-github-issues` |
| `.claude/skills/` | Claude Code | `rust-engineer`, `tui-engineer` |

Claude Code discovers project skills from `.claude/skills/<name>/SKILL.md` only. That discovery
demonstrably works here — a Claude Code session in this repo lists `rust-engineer` and
`tui-engineer` as available skills, and does **not** list `otto-development` or
`creating-github-issues`. So the repo's two most load-bearing workflow documents — the autonomous
spec → plan → implement → PR → review → merge → release spine, and this repo's
GitHub-Issues-only ticket conventions — are invisible to Claude Code. A Claude Code session either
gets told those conventions by hand every time, or re-derives a thinner version from `CLAUDE.md`.

`.claude/skills/` is already committed and already the endorsed location for repo-authored,
project-specific skills (`CLAUDE.md:89`), so nothing about the policy needs changing — only the
content gap needs closing.

## Premise corrections

The issue frames the choice as **port (copy) vs. symlink**. Both premises survive only partially,
and the reason is content, not mechanism:

1. **A raw symlink imports instructions that are wrong for the host agent.**
   `.github/skills/otto-development/SKILL.md` is written *for the orchestrating GitHub Copilot
   CLI*, and says so explicitly at `SKILL.md:360-361`: "This skill is written for the
   *orchestrating* GitHub Copilot CLI's `task` tool — the CLI agent running this skill — **not
   Claude Code's `Agent` tool**." Its dispatch mechanics are Copilot-specific throughout: a fixed
   seven-value `agent_type` enum (`SKILL.md:370-382`), `mode: "sync"` / `mode: "background"`
   (`:384-388`), `read_agent` for collecting background results (`:388`), `ask_user` for the
   security-review output contract (`:400-406`), and "the orchestrating CLI's own `sql` tool and
   its session-scoped `todos` table" for progress tracking (`:349`). `agent-prompts.md` repeats
   `agent_type:` in all nine dispatch templates. Symlinked verbatim, a Claude Code session would
   read ~100KB of workflow whose every dispatch step names a tool it does not have — and whose own
   text tells it that it is not the intended reader.
2. **A full copy is not the only alternative to a symlink.** The issue treats "port" as
   copy-and-adapt, which for an 82KB `SKILL.md` plus a 20KB `agent-prompts.md` means maintaining a
   second copy of the repo's most-edited process document. The drift risk is not hypothetical: the
   Copilot original is what gets edited when the workflow changes.
3. **`creating-github-issues` has none of problem 1.** It is 7KB of plain `gh issue` / `gh label`
   invocations with no host-agent-specific tooling at all — it is already host-agnostic and would
   symlink cleanly. The asymmetry matters for the decision below.

## Approach

**Decision: Claude-Code-native adapter stubs that delegate to the canonical `.github/skills/` body.**
Not raw symlinks, not full copies.

Each skill gets a small, committed, real file:

```
.claude/skills/otto-development/SKILL.md        (new, adapter stub)
.claude/skills/creating-github-issues/SKILL.md  (new, adapter stub)
```

A stub contains exactly three things:

1. **Claude Code frontmatter** — `name` and `description` copied verbatim from the canonical skill,
   so Claude Code's trigger matching behaves identically to Copilot's. No `tools:`/`model:` key:
   both workflows need the full toolset and strong reasoning, and the existing
   `rust-engineer`/`tui-engineer` pins are not a precedent to follow here (a `model: sonnet` pin on
   `otto-development` would cap the orchestrator of a full autonomous run).
2. **A canonical-body pointer** — a machine-checkable `> **Canonical body:**` line naming
   `.github/skills/<name>/SKILL.md` (and `agent-prompts.md` for `otto-development`), with an
   instruction to read it before acting. This is the single source of truth; the ~100KB spine is
   never duplicated.
3. **A host-agent translation table** (`otto-development` only) — the Copilot→Claude Code mapping
   for every mechanism the canonical body names, so the delegation is actually actionable rather
   than leaving the reader to guess:

   | Canonical body says | In Claude Code |
   | --- | --- |
   | `task` tool call | `Agent` tool call |
   | `agent_type: "general-purpose"` | `subagent_type: "general-purpose"` |
   | `agent_type: "rubber-duck"` (spec/plan critique) | `subagent_type: "general-purpose"` — no built-in equivalent; the critique prompt carries the role |
   | `agent_type: "code-review"` | the `/code-review` skill, or `subagent_type: "general-purpose"` with the canonical template |
   | `agent_type: "security-review"` | the built-in `security-review` skill |
   | `agent_type: "explore"` / `"research"` | `subagent_type: "Explore"` |
   | `mode: "sync"` | a single `Agent` call — it returns the report |
   | `mode: "background"` | several `Agent` calls in one message; results arrive as task notifications |
   | `read_agent` | the task-completion notification, or `SendMessage` to the named agent |
   | the `sql` tool's `todos` table | `TodoWrite` |
   | `ask_user` | `AskUserQuestion` — still overridden for autonomy per the canonical body |

   The table deliberately maps only to **built-in** Claude Code agent types and skills. A
   contributor's personal `~/.claude/agents/` collection may well contain `rust-pro`,
   `architect-reviewer`, `code-reviewer` or `security-auditor` — the canonical body notes at
   `SKILL.md:366-368` that no such catalog exists in Copilot's enum, and in Claude Code they exist
   only if that contributor installed them. The stub mentions them as an optional upgrade, never as
   a requirement, so the mapping holds in a fresh clone.

**Why not raw symlinks.** They cannot carry the translation table (premise correction 1) — the one
thing that makes the canonical body usable from Claude Code. They also need `core.symlinks=true` to
materialise on a Windows checkout, and this repo ships and tests on Windows.

**Why not full copies.** An 82KB + 20KB duplicate of the repo's most-edited process document, with
drift on every workflow edit and no mechanical way to keep the two honest beyond a byte-comparison
that the intended Claude-Code adaptations would immediately break.

**Why the same mechanism for `creating-github-issues`** even though it would symlink cleanly: one
uniform, self-explaining mechanism across `.claude/skills/` beats two mechanisms chosen per skill,
and its stub still earns its keep by carrying the frontmatter Claude Code matches on plus the
pointer the parity check verifies. Its stub needs no translation table (nothing to translate).

**Drift prevention** — `.github/scripts/check-claude-skill-stubs.sh`, run by CI's existing `lint`
job and runnable locally. It asserts, for every directory under `.github/skills/`:

- a `.claude/skills/<name>/SKILL.md` exists (so a future Copilot skill cannot be added without
  being reachable from Claude Code);
- that stub's `name:` and `description:` frontmatter match the canonical skill's byte-for-byte
  after stripping optional wrapping quotes (so trigger behaviour cannot silently diverge);
- the stub's `> **Canonical body:**` pointer names a path that exists (so a rename or move of the
  canonical file fails CI instead of leaving a dangling instruction).

Claude-Code-native skills with no canonical counterpart (`rust-engineer`, `tui-engineer`) are not
iterated and need no marker.

## Scope

**In:**

- `.claude/skills/otto-development/SKILL.md` — adapter stub with frontmatter, canonical pointer
  (both `SKILL.md` and `agent-prompts.md`), and the host-agent translation table.
- `.claude/skills/creating-github-issues/SKILL.md` — adapter stub with frontmatter and canonical
  pointer.
- `.github/scripts/check-claude-skill-stubs.sh` — the parity + frontmatter-match + pointer-target
  check.
- `.github/workflows/ci.yml` — one step in the existing `lint` job invoking that script.
- `CLAUDE.md`'s "Claude Code skills" section — the two-directory layout, the adapter-stub mechanism
  and why, the parity check, and the revised precedence rule.

**Out:**

- Editing either canonical `.github/skills/` body. They stay Copilot-authoritative; the translation
  lives in the stub. (A future change may choose to make the canonical bodies host-neutral — that is
  a separate decision, not a side-effect of this one.)
- Stubs for `rust-engineer` / `tui-engineer` in the other direction (no Copilot counterpart is
  wanted; Copilot is not the agent used for TUI/Rust review here).
- Any change to personal-skill policy (`CLAUDE.md:91`) — personal skills stay in `~/.claude/skills/`.
- A generic "skill format converter". Two skills, one of which needs no conversion at all, does not
  justify tooling.
- Any product code, crate, or public-interface change.

## Public-interface changes

**None.** This touches `.claude/skills/`, `.github/scripts/`, `.github/workflows/ci.yml` and
`CLAUDE.md` only. No SPP wire-format type, no `ProviderHandler`/`ProviderClient` method, no tool MCP
schema, no plugin ABI surface, no slash command, no env var, and no on-disk transcript/keyring
format is added, renamed, or removed (Non-Negotiable Rule 6 is not engaged). No crate gains a
dependency edge.

## Assumptions

- **Claude Code reads a skill's body from `SKILL.md` and follows in-repo file references it names.**
  The stub delegates rather than inlines. Basis: skills bundling companion files is the established
  shape (`otto-development` itself bundles `agent-prompts.md`), and the reading agent has full
  filesystem access. If a future Claude Code version required the whole body inline, the stub is the
  only file that would need to change.
- **`description` verbatim reuse is correct.** The canonical descriptions are already written as
  trigger conditions ("Use when developing any feature or fix in the otto repository…"), which is
  exactly what Claude Code matches on. Rewriting them would make the two hosts trigger differently
  on the same request, which is the drift this change exists to prevent.
- **The check belongs in the existing `lint` job, not a new job.** It is a sub-second shell check;
  a dedicated runner would cost more in queue time than it saves in clarity.
- **Bash is acceptable in `.github/scripts/`.** The repo's "Rust-only" rule (`PRD.md`, `CLAUDE.md`)
  governs the shipped product; `.github/workflows/` already runs bash, and a shell script keeps the
  check locally runnable without adding a crate whose tests would run on every `cargo test`.
- **No `tools:` frontmatter key on either stub.** Both workflows need file edits, `git`/`gh`, cargo
  and dispatch; enumerating a `tools:` allowlist could only under-grant.
- **Release line is a PATCH.** Per `CHANGELOG.md`'s stated convention (MINOR for features and
  breaking boundary changes, PATCH for fixes), repo tooling and contributor docs with no runtime
  behaviour change is a PATCH: `v0.28.1` from the current `0.28.0`.

## Goal & Success Criteria

A Claude Code session started in a fresh clone of this repo discovers, triggers and correctly
executes the repo's own `otto-development` and `creating-github-issues` workflows, with one canonical
copy of each workflow body and a CI check that keeps the two hosts from drifting apart.

- `.claude/skills/otto-development/` and `.claude/skills/creating-github-issues/` each contain a
  committed `SKILL.md` whose `name`/`description` match the canonical skill byte-for-byte.
- Both stubs name an existing canonical path; `otto-development`'s stub additionally maps every
  Copilot dispatch mechanism its canonical body names to a built-in Claude Code equivalent.
- `.github/scripts/check-claude-skill-stubs.sh` exits 0 on the committed tree, and exits non-zero
  for each of its three failure modes (missing stub, mismatched frontmatter, dangling pointer) when
  those are injected.
- CI's `lint` job runs that script.
- `CLAUDE.md`'s "Claude Code skills" section documents the layout, the mechanism, the check, and the
  revised precedence rule.
- Verified out-of-band (Phase 5): a fresh `git clone` of the merged trunk, listing the skills a
  Claude Code session in it discovers, shows all four.

## Error Handling & Edge Cases

- **Canonical skill renamed or removed** → the parity check's pointer-target assertion fails CI
  with the offending stub and path named. This is the intended failure: renaming a canonical skill
  is a two-file change.
- **New skill added to `.github/skills/` without a stub** → parity assertion fails, naming the
  skill and the exact path to create.
- **Frontmatter edited on one side only** → frontmatter-match assertion fails, printing both values.
  Quoting differences alone (`description: "x"` vs `description: x`) do not fail, since the existing
  `.claude/skills/` files quote and the `.github/skills/` ones do not.
- **A description containing a `:` or `#`** → the extractor takes everything after the first `: `
  on the `description:` line and does not treat `#` as a comment; both canonical descriptions
  contain `—`, `(`, `.` and `/` and must survive unchanged.
- **Multi-line / folded YAML description** → not supported by the extractor; both canonical
  descriptions are single-line today. The script fails loudly (rather than silently comparing a
  truncated value) if a `description:` line is absent or empty.
- **CRLF checkout on Windows** → the script is CI-run on `ubuntu-latest`; local Windows runs under
  Git Bash are best-effort. Trailing `\r` is stripped during extraction so a `core.autocrlf` clone
  does not produce a spurious mismatch.
- **`.claude/skills/` entry with no canonical counterpart** → ignored by design; the check iterates
  `.github/skills/`, so `rust-engineer` and `tui-engineer` are untouched.

## Risks & Open Questions

- **The translation table is a maintained artifact.** If Claude Code renames a built-in agent type
  or skill, the table goes stale and nothing in CI catches it (the check verifies paths and
  frontmatter, not the semantic accuracy of a mapping). Mitigation: the table maps only to
  long-lived built-ins and states that personal specialised agents are optional. Accepted risk.
- **Delegation costs a file read.** A Claude Code run of `otto-development` reads the stub, then the
  82KB canonical body — the same total it would read from a copy, plus one hop. No mitigation
  needed; noted so it is not mistaken for an oversight.
- **The canonical bodies still address Copilot in the second person.** A Claude Code reader gets
  correct mechanics from the table but slightly odd prose ("this CLI's seven `agent_type`s"). Making
  the canonical bodies host-neutral is the natural follow-up and is explicitly out of scope here.
- **Open question, deliberately not blocking:** whether `CLAUDE.md`'s precedence rule should keep
  saying `.github/skills/` governs. This design answers yes-with-a-clarification — the canonical
  *body* governs, and the stub governs only the host-mechanism translation — because that is exactly
  what the file layout now encodes.

# Porting this repo's `.github/skills/` skills to Claude Code — design

Date: 2026-09-10
Status: IMPLEMENTED
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

The issue frames the choice as **port (copy) vs. symlink**. Two corrections, both about content
rather than mechanism:

1. **A symlink is not viable, because the canonical body is host-specific.**
   `.github/skills/otto-development/SKILL.md` is written *for the orchestrating GitHub Copilot
   CLI*, and says so explicitly at `SKILL.md:360-361`: "This skill is written for the
   *orchestrating* GitHub Copilot CLI's `task` tool — the CLI agent running this skill — **not
   Claude Code's `Agent` tool**." Its dispatch mechanics are Copilot-specific throughout: a fixed
   seven-value `agent_type` enum (`SKILL.md:370-382`), `mode: "sync"` / `mode: "background"`
   (`:384-388`), `read_agent` for collecting background results (`:388`), `ask_user` for the
   security-review output contract (`:400-406`), and "the orchestrating CLI's own `sql` tool and
   its session-scoped `todos` table" for progress tracking (`:349`). `agent-prompts.md` repeats
   `agent_type:` across all ten dispatch templates. Symlinked verbatim, a Claude Code session would
   read ~100KB of workflow whose every dispatch step names a tool it does not have — and whose own
   text tells it that it is not the intended reader. (Secondary, but real: a committed symlink
   needs `core.symlinks=true` to materialise on a Windows checkout, and this repo ships and tests
   on Windows.)
2. **Porting means adapting, and the adaptation is small and bounded.** "Port" here is not a
   verbatim copy — a copy of `otto-development` would be as unusable as the symlink, for the same
   reason. It means the content itself rewritten so a Claude Code session reads instructions naming
   Claude Code's own tools. Measured from the committed records, that adaptation replaces **132 of
   the 1,602 canonical lines — about 8%** — with 214 ported lines, across 25 hunks: 75 → 113 in
   `otto-development/SKILL.md` (14 hunks), 57 → 96 in `otto-development/agent-prompts.md` (10
   hunks), and 0 → 5 in `creating-github-issues/SKILL.md` (1 hunk), which is already fully
   host-neutral (plain `gh issue` / `gh label` invocations) and diverges only by the "ported from"
   note. So ~92% of the canonical text is carried into the port untouched, which is what makes the
   drift problem tractable rather than fatal. It is 8% rather than the ~4% a pure mechanical
   substitution would cost because two sections are rewritten rather than substituted (see below) —
   still small enough for the divergence to be recorded exactly rather than managed by convention,
   which is the argument this whole design rests on.

## Approach

**Decision: port the skills — adapted copies under `.claude/skills/`, with the divergence from the
canonical recorded exactly and verified in CI.**

The deliverables a Claude Code session actually reads:

```
.claude/skills/otto-development/SKILL.md          (ported: adapted for Claude Code)
.claude/skills/otto-development/agent-prompts.md  (ported: adapted for Claude Code)
.claude/skills/creating-github-issues/SKILL.md    (ported: no adaptation needed)
```

Each is a complete, self-sufficient skill. A reader follows it start to finish without consulting
`.github/skills/` and without applying any mental translation — which is the definition of "works"
this design is built to satisfy.

### What the adaptation changes

Only host-mechanism references. The workflow itself — every Non-Negotiable Rule, phase gate,
fix-loop cap, review requirement, repo convention and load-bearing invariant — is host-neutral and
is carried across unchanged.

| Canonical (Copilot CLI) | Ported (Claude Code) |
| --- | --- |
| the `task` tool | the `Agent` tool |
| `agent_type: "general-purpose"` / `"task"` | `subagent_type: "general-purpose"` |
| `agent_type: "rubber-duck"` (spec/plan critique) | `subagent_type: "general-purpose"` — no built-in equivalent, so the critique prompt carries the role |
| `agent_type: "code-review"` | `subagent_type: "general-purpose"` with the canonical review template, or the `/code-review` skill |
| `agent_type: "security-review"` | `subagent_type: "general-purpose"` carrying the blind-diff prompt — **not** the built-in `security-review` skill (see below) |
| `agent_type: "explore"` / `"research"` | `subagent_type: "Explore"` |
| `mode: "sync"` | a single `Agent` call — it returns the report |
| `mode: "background"` | several `Agent` calls in one message; results arrive as task notifications |
| `read_agent` | the task-completion notification, or `SendMessage` to the named agent |
| the `sql` tool's `todos` table | `TodoWrite` |
| `ask_user` | `AskUserQuestion` (still overridden for autonomy) |
| `model: "claude-haiku-4.5"` / `reasoning_effort:` | `model: "haiku"` for mechanical tasks; omit `model` for design-judgment work — and pass `model: "opus"` where the session default is known to be weaker, since an omitted `model` resolves to the agent definition's model, then the configured default subagent model, and only then the parent's. There is no `reasoning_effort` parameter. |

**Why the security row is not the obvious one.** Claude Code ships a built-in `security-review`
skill, and mapping `agent_type: "security-review"` onto it is the mechanical answer. It is wrong,
and the port rejects it. A `Skill` invocation runs in the **orchestrator's own context**, which by
Phase 4 step 8 already holds the spec, the plan, the task brief and the PR body — so Non-Negotiable
Rule 5's requirement that this pass see only the diff would become a promise the orchestrator makes
to itself, unenforceable and false at the moment it matters. A fresh `general-purpose` subagent has
seen nothing but its prompt, which makes the blindness structural rather than asserted. Two
consequences the port also carries: the dispatched agent may invoke the `security-review` skill
*itself*, inside its own fresh context, which is where the skill form is safe; and because
`general-purpose` holds Bash/Edit/Write where the Copilot agent type is read-only by construction,
the ported prompt opens with an explicit read-only constraint. Anyone re-porting from this table
must reproduce both, not just the `subagent_type` substitution.

Two sections need rewriting rather than substituting: "How dispatch works in this environment"
(`SKILL.md:358-410`), whose whole subject is the Copilot dispatch mechanism, and the review-trio
table (`:685-689`), whose `agent_type` column changes meaning. Both are rewritten to describe
Claude Code's mechanics directly.

**What must NOT be translated.** `SKILL.md:671-678` and `:1024` reference **GitHub's Copilot
pull-request reviewer** — the `copilot-pull-request-reviewer` bot added via `gh pr edit
--add-reviewer`. That is a real GitHub feature, unrelated to the Copilot CLI, and it stays exactly
as written in the port. A blind `s/Copilot/Claude/` would corrupt a working instruction into a
broken one; this is called out because it is the single most likely way to get the port wrong.

The ported files also keep `name` and `description` **byte-for-byte** from the canonical, so both
hosts trigger on exactly the same requests, and carry a short "ported from" note directing a
contributor to edit the canonical and re-port rather than editing the port in place.

### Drift prevention: regenerate and compare

The port is a second copy, so drift is the whole risk. It is handled mechanically rather than by
convention.

`.github/scripts/check-claude-skill-ports.sh`, run by CI's existing `lint` job, holds a committed
record of the intended divergence — one expected-diff file per ported file, under
`.github/skills/<name>/claude-port/`. For each ported file the check recomputes
`diff canonical ported` and compares it against the committed expected diff. Any difference fails
the build. Concretely:

- **Canonical edited, port not updated** → the recomputed diff no longer matches the expected diff
  → red, naming the file and telling the contributor to re-port and refresh the expected diff.
- **Port edited directly** → same failure, from the other side.
- **Both updated together, expected diff refreshed** → green. This is the intended workflow.
- **A new skill added under `.github/skills/` with no port** → red (a missing port directory is a
  missing expected diff).
- **`name`/`description` drifted between canonical and port** → red, and reported specifically
  rather than as an opaque diff mismatch, because that is the failure that silently changes which
  requests each host triggers on.

The expected-diff files are generated, reviewable artifacts: they are exactly the adaptation and
nothing else — 25 hunks over ~130 canonical lines — so a reviewer reads the port's entire divergence
from the canonical in one place, and any *unintended* divergence shows up as a diff-of-diffs in code
review. They are produced only by the check's own `--update` mode, which writes them through the
same function the verifier compares against, so generation and verification cannot disagree about
the diff's form.

`creating-github-issues` needs no adaptation, so its expected diff contains only the "ported from"
note. That makes the check strictest exactly where adaptation is absent: any change to that
canonical body must be mirrored immediately.

Claude-Code-native skills with no canonical counterpart (`rust-engineer`, `tui-engineer`) have no
port directory, so the per-file loop does not iterate them and nothing verifies their *content*.
They are not unchecked, though: the top-level sweep requires every directory under
`.claude/skills/` to be either a port or a name declared in the script's `NATIVE_SKILLS` allowlist,
so a native skill is a deliberate, reviewable line rather than something a directory drifts into
when its canonical is deleted.

On `main` before this change, `.github/scripts/` does not exist and the repo contains no `.sh` file
(this change adds the first of each), though there is a
precedent for CI *guards*: `.github/workflows/wit-dep-guard.yml` and `wit-portability-guard.yml`
are inline `run:` bash blocks with `set -euo pipefail` and GitHub `::error::` annotations. This
check follows their style but lives in a standalone script so a contributor can run it locally.
Three consequences settled here: the script opens with `set -euo pipefail`; CI invokes it as `bash
.github/scripts/check-claude-skill-ports.sh` so the git executable bit is never load-bearing (it is
set anyway); and a new `.gitattributes` pins `*.sh text eol=lf` so a `core.autocrlf=true` clone does
not produce a CRLF shebang.

## Scope

**In:**

- `.claude/skills/otto-development/SKILL.md` and `agent-prompts.md` — ported, adapted.
- `.claude/skills/creating-github-issues/SKILL.md` — ported, no adaptation needed.
- `.github/skills/<name>/claude-port/*.diff` — the committed expected divergences.
- `.github/scripts/check-claude-skill-ports.sh` — the regenerate-and-compare check, its `--update`
  record-generation mode, and the `name`/`description` assertion.
- `.github/workflows/ci.yml` — one step in the existing `lint` job, plus a workflow-level
  `permissions: contents: read` (no job in the file writes to the repository, and the check runs on
  `pull_request` where a fork's content reaches it).
- `.gitattributes` (new) — `*.sh text eol=lf` for the shebang, plus `*.diff` and both skill trees
  pinned to LF so the committed records still verify on a `core.autocrlf=true` clone.
- `CLAUDE.md`'s "Claude Code skills" section — the two-directory layout, the port mechanism, the
  edit-canonical-then-re-port workflow, the check, and the revised precedence rule.
- **Canonical `.github/skills/**/*.md` edits, where the canonical itself has a bug.** Editing a
  canonical is the *normal* first half of the workflow this design installs — edit the canonical,
  mirror the edit into the port, regenerate the record — so it cannot also be out of scope here.
  Worked example on this branch: `d4824da` fixed `creating-github-issues/SKILL.md`, where a `\`
  line continuation was followed by a `#` comment, which is broken bash in the one command that
  skill exists to hand a contributor. It was mirrored into the port and the record regenerated, and
  the check went green — so the commit is a live proof that the round trip works, not a scope
  breach. What is forbidden is editing a *port* in place; see below.

**Out:**

- Editing a **port** in place. A port is regenerated from its canonical, so an in-place edit is
  lost work as soon as the next re-port lands — and it is the drift the check exists to catch, from
  the port's side. This is the prohibition; "do not touch `.github/skills/`" is **not**.
- Rewriting the canonical bodies to be host-neutral so both hosts could share one text. That is a
  plausible future change — it would remove the need for a port at all — and it is not this one.
  The canonicals remain Copilot-authoritative and are the source the port is derived from.
- Reverse-direction ports of `rust-engineer` / `tui-engineer` into `.github/skills/`.
- Any change to personal-skill policy (`CLAUDE.md:91`) — personal skills stay in `~/.claude/skills/`.
- A general-purpose skill-format converter. Two skills, one needing no conversion, does not justify
  one.
- Any product code, crate, or public-interface change.

## Public-interface changes

**None.** This touches `.claude/skills/`, `.github/skills/*/claude-port/`, `.github/scripts/`,
`.github/workflows/ci.yml`, `.gitattributes` and `CLAUDE.md` only. No SPP wire-format type, no
`ProviderHandler`/`ProviderClient` method, no tool MCP schema, no plugin ABI surface, no slash
command, no env var, and no on-disk transcript/keyring format is added, renamed, or removed
(Non-Negotiable Rule 6 is not engaged). No crate gains a dependency edge.

## Assumptions

- **A ported skill must be self-sufficient.** The port is read start to finish with no reference
  back to `.github/skills/`. This is the requirement that ruled out a pointer-stub design (a small
  file delegating to the canonical body plus a translation table): it works only if the reader
  reliably follows a pointer out of `.claude/skills/` and then holds a mapping in mind across 82KB,
  and "adapted so it works" is the stated goal.
- **`description` verbatim reuse is correct.** The canonical descriptions are already written as
  trigger conditions, which is what Claude Code matches on. Rewriting them would make the two hosts
  trigger differently on the same request.
- **The expected-diff format is `diff -u` with a stable invocation.** Hunk headers carry line
  numbers, so an unrelated canonical edit shifts them and fails the check. That is intended: a
  canonical edit should force a re-read of the port, not be silently absorbed.
- **The translation table above is complete for the current canonical text.** It covers all seven
  `agent_type` values, both `mode` values, `read_agent`, the `todos` table, `ask_user`, and model
  selection. If a future canonical edit introduces a new host-specific mechanism, the check fails
  (the diff changes) and the porter extends the table then.
- **Built-in Claude Code mechanisms only.** A contributor's `~/.claude/agents/` may provide
  `rust-pro`, `architect-reviewer`, `code-reviewer` or `security-auditor`; the port mentions these
  as an optional upgrade for the review trio, never a requirement, so it holds in a fresh clone.
- **The check belongs in the existing `lint` job**, immediately after `actions/checkout@v4` — it
  needs no toolchain, so a failure surfaces in seconds rather than after a clippy build.
- **Bash in `.github/scripts/` is acceptable.** The "Rust-only" rule governs the shipped product;
  `.github/workflows/` already runs bash, and a script keeps the check locally runnable without
  adding a crate whose tests would run on every `cargo test`.
- **Release line is a PATCH.** Per `CHANGELOG.md`'s convention (MINOR for features and breaking
  boundary changes, PATCH for fixes), contributor-facing tooling and docs with no runtime behaviour
  change is a PATCH: `v0.28.2` from the current `0.28.1`.

  **This assumption did not survive the cut, in the way assumptions usually don't — it was right
  about this change and wrong about the world around it.** The reasoning holds: these ports are
  contributor-facing with no runtime behaviour change, and taken alone they are a PATCH. But a
  release line is a property of the *batch*, not of one change. By cut time #118 had also merged,
  adding a defaulted method to the public `Screen` trait, and one feature in the batch forces MINOR.
  This shipped in `v0.29.0`; `v0.28.2` will never exist. The lesson worth carrying to the next spec
  is that "release line" is not knowable at design time and should be written as a floor rather
  than a number.

## Goal & Success Criteria

A Claude Code session started in a fresh clone discovers this repo's own `otto-development` and
`creating-github-issues` skills and executes them correctly from the ported text alone, with the
port's divergence from the canonical recorded exactly and enforced by CI.

- All three ported files exist under `.claude/skills/`, each self-sufficient: no instruction in a
  port names a tool Claude Code does not have, and no port requires reading `.github/skills/`.
- Every host-mechanism reference in the translation table is adapted; the GitHub Copilot
  *PR-reviewer* references at `SKILL.md:671-678` and `:1024` are **not** altered.
- `name`/`description` in each port match the canonical byte-for-byte.
- `.github/scripts/check-claude-skill-ports.sh` exits 0 on the committed tree and exits non-zero for
  each failure mode: canonical edited without mirroring, port edited directly, missing port, missing
  or orphaned record, orphaned port, empty record, verbatim-copy port, unreadable file, symlinked or
  ill-named input, `name`/`description` drift, a port missing its "Ported for Claude Code" note, and
  an undeclared `.claude/skills/` entry that is neither a port nor named in `NATIVE_SKILLS`. Its
  `--update` mode is the only record generator,
  writing through the same function the verifier compares against, and is never the default.
- CI's `lint` job runs the check.
- `CLAUDE.md` documents the layout, the port mechanism, the edit-canonical-then-re-port workflow,
  and the revised precedence rule.
- Verified out-of-band (Phase 5): in a fresh clone of merged trunk, a Claude Code session lists all
  four skills, and triggering `otto-development` yields a run that follows the ported workflow using
  Claude Code's own dispatch mechanics.

## Error Handling & Edge Cases

- **Canonical renamed or moved** → the check cannot find it and fails naming both paths; the port
  and its expected diff must be renamed with it.
- **`.github/skills/` itself moved or emptied** → the check fails rather than reporting success over
  zero skills. An empty iteration passing green is the drift class most likely to go unnoticed.
- **A port directory with no expected diff, or an expected diff with no port** → fails, naming what
  is missing.
- **`name`/`description` drift** → reported as its own failure, not as an opaque diff mismatch,
  since it is the failure that changes trigger behaviour.
- **Missing or empty frontmatter key on either side** → fails loudly rather than comparing two empty
  values, which would make the assertion vacuous. Frontmatter is read only from the leading `---`
  fenced block, so an unindented `description:` in the body cannot stand in for a deleted key.
- **CRLF checkout** → trailing whitespace, `\r` included, is stripped when extracting frontmatter
  values, so a `core.autocrlf=true` clone does not produce a spurious mismatch. The `.gitattributes`
  LF pins keep the script's shebang, the records, and both skill trees byte-stable, without which a
  Windows contributor running the check locally could never get a green result.
- **A canonical `*.md` nested in a subdirectory** (e.g. `reference/deep.md`) → iterated like any
  other: it needs its own port and its own record. A non-recursive glob would let a nested canonical
  file drift unported behind a green build, which is the false-green class this check exists to
  prevent.
- **A port `*.md` with no canonical counterpart *inside a ported skill*** → fails. It is a rule
  Claude Code would execute that the source of truth does not contain, which is the precedence rule
  read backwards. (This is distinct from a whole `.claude/skills/` entry with no canonical
  directory, below, which fails unless declared in `NATIVE_SKILLS`, see below.)
- **A record that is empty, or a port byte-identical to its canonical** → fails. Zero divergence
  asserts a verbatim copy, the one arrangement this design exists to avoid, and an empty record
  would also compare equal to a diff that could not be computed.
- **An unreadable canonical or port** → fails naming the file. `diff` signals this with exit 2
  ("trouble"), which must not be collapsed with exit 1 ("differ"): collapsing them turns an
  unreadable file into an empty diff and a green build over something nobody read.
- **A canonical, port or record committed as a symlink** → rejected rather than followed. CI runs
  the check on `pull_request`, so a fork's tree reaches it, and the diff-of-diffs would print the
  target — `.git/config`, where `actions/checkout` leaves the job token — into the public job log.
- **A path segment outside `[A-Za-z0-9._-]`** → rejected before it reaches any message, so a
  filename cannot inject a shell command into remediation text or a workflow command into an
  annotation. `fail` additionally escapes `%`, CR and LF.
- **A non-C locale** → prevented: `LC_ALL=C` is exported, because GNU diffutils translates
  `\ No newline at end of file`, and a record regenerated under another locale would commit a
  translated marker and fail everywhere else.
- **A defeated frontmatter assertion** → each way it can be defeated is a named failure: a YAML
  block scalar (`description: >`), an empty quoted value, a duplicate key, an unterminated `---`
  block, a missing leading `---` (where a UTF-8 BOM lands), and a `key:` prefix that is really a
  different key (`name:s: v` declares `name:s`). This matters because "regenerate the record" is
  the endorsed fix for a red build, and after a regeneration this assertion is all that is left.
- **A canonical skill directory containing a companion file the port omits** → fails: a companion
  the port lacks is a companion Claude Code can never read, and `agent-prompts.md` holds the
  dispatch templates `otto-development` requires be pasted verbatim.
- **`.claude/skills/` entry with no canonical counterpart** → fails unless the entry is named in the
  script's `NATIVE_SKILLS` allowlist, a deliberate, reviewable declaration; `rust-engineer` and
  `tui-engineer` pass because they are listed there, matching how `CLAUDE.md`'s "Claude Code skills"
  section documents this behavior.

## Risks & Open Questions

- **Two copies of a 103KB document is a real maintenance cost**, and the honest trade this design
  makes. It is mitigated, not eliminated: the divergence is ~8% and recorded exactly, CI refuses to
  let the copies drift, and the failure mode is a loud red build rather than a silently stale
  workflow. The alternative that removes the cost entirely — making the canonical bodies
  host-neutral so both hosts read one text — is the natural follow-up and is out of scope here.
- **A canonical edit imposes work on the editor**, who must mirror the edit into the port and
  refresh the expected diff. That is the cost of the port being self-sufficient, and it is usually
  small: an edit landing outside the adapted regions is copied into the port verbatim, and only an
  edit landing *on* an adapted line needs the adaptation re-applied. The check's failure message
  says both, then names the one regeneration command
  (`bash .github/scripts/check-claude-skill-ports.sh --update`).
- **The adaptation is judgement, not mechanism.** Substituting `agent_type` values is mechanical;
  rewriting "How dispatch works in this environment" is not. A future canonical rewrite of that
  section needs a human to re-adapt it, and the check can only tell them that it changed, not
  whether their re-adaptation is good.
- **`reasoning_effort` has no Claude Code equivalent**, so the canonical's effort-tuning guidance is
  approximated by model selection. A dispatch the canonical wanted at `"xhigh"` gets whatever the
  session's model provides.
- **Open question, deliberately not blocking:** whether the ported `agent-prompts.md` should keep
  the canonical's `task tool:` YAML-ish block shape at all, or present dispatches as prose. The port
  keeps the block shape with `Agent tool:` / `subagent_type:` fields, because the canonical body
  refers to those templates by name and structure, and diverging structurally would widen the diff
  far beyond the host-mechanism changes this port is meant to contain.

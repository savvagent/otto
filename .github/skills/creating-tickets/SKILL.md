---
name: creating-tickets
description: Use when asked to create, file, open, or log a new ticket — a defect/bug or a user story/feature — as a GitHub issue in this repository, as opposed to working an existing one. Handles type selection (label), assignment, defect priority (label), and (for non-trivial work) writing a reviewed spec + plan and posting them as issue comments. For developing/shipping an EXISTING issue, use `savvagent-development` instead.
---

# Creating Tickets (GitHub Issues only)

Create a new GitHub issue (defect or user story/feature) correctly the first time: right
label(s), assigned to the right person, defects prioritized, and — when the work is
non-trivial — a reviewed spec and plan posted as issue comments so whoever picks it up
(often via `savvagent-development`) starts from a real design.

This skill **creates** issues. It is the upstream counterpart to `savvagent-development`,
which **works** existing issues end-to-end (spec → plan → implement → ship → release). If
an issue already exists and you're implementing it, you're in the wrong skill.

**This repository has no JIRA.** There is exactly one tracker: **GitHub Issues** on this
repo (`savvagent/otto`, or the current repo's `origin` remote if you're running this
outside that checkout). Do not introduce a JIRA-first workflow, a `.claude/tracker.json`
tracker switch, or any other tracker abstraction here — every step below assumes `gh`.

## The Iron Law

**Every created issue is assigned, typed (labeled), and prioritized before you report it
created.** A bare issue with no assignee and no type label is not done. Defects additionally
get a priority label when this repo has one configured (see Step 2).

**Spec before plan, both reviewed before posting.** The spec defines *what/why*; the plan
defines *how*. A plan reviewed without an approved spec is reviewing against nothing. Post
the spec comment first, then the plan comment.

**Violating the letter of the workflow is violating the spirit.**

## Prerequisites

`gh` must be authenticated: `gh auth status`. Resolve the target repo once — default
`savvagent/otto`; override only if the current working tree's `origin` remote points
elsewhere (`git remote get-url origin`).

## Defaults (override only when the request says so)

| Decision | Default | Override when |
|---|---|---|
| Repo | `savvagent/otto` (or the current repo's `origin`) | Request names another repo explicitly |
| Assignee | `@me` | Request says "assign to `<login>`" or "leave unassigned" |
| Defect priority | Skip unless the repo has `priority:*` labels (see Step 2) | Request names a priority, or `priority:*` labels exist |
| Spec + plan | Written + reviewed + posted **unless the work is fast-path trivial** (see Step 4) | — |

## Step 1 — Map the request to a type (label)

- "bug", "broken", "regression", "error", "defect", "doesn't work", "still displays/shows
  the wrong thing" → **defect** → label `bug`
- "feature", "add", "support", "as a user", "new", "enhancement", "story" → **user
  story/feature** → label `enhancement`

If genuinely ambiguous, pick the better fit and note the choice in your final report — do
not block the create on it.

## Step 2 — Confirm available labels (cheap, prevents a failed create)

```bash
gh label list --repo <repo>
```

This repo's stock label set is `bug`, `documentation`, `enhancement`, `good first issue`,
`help wanted`, `invalid`, `question`, `wontfix` — there is no `priority:*` label set by
default. If the request implies urgency (production-wide outage, data loss/corruption,
security/PII exposure) and no priority label exists, say so plainly in the issue body (a
`## Priority` line) rather than inventing a label; if the maintainer has since added
`priority:*` labels, use them (`--label priority:<level>`) instead.

**Before creating, check for an obvious duplicate:**

```bash
gh issue list --repo <repo> --search "<key terms>" --state open
```

If a clear match exists, surface it and ask whether to proceed rather than silently
creating a duplicate.

## Step 3 — Create the issue

Write a real **title** (imperative, specific) and a **body**:

- **Defect body:** *Summary* → *Expected behavior* → *Actual behavior* → *Acceptance
  Criteria* (the fix's done-condition). Add reproduction steps when known.
- **User story/feature body:** a *user-value* statement ("As a … I want … so that …") +
  *Acceptance Criteria* bullets.

Then create it:

```bash
gh issue create --repo <repo> \
  --title "<concise imperative title>" \
  --body "<body + Acceptance Criteria>" \
  --label bug \                 # or: enhancement
  --assignee @me                # or a login, or omit if told to leave unassigned
```

**Hardening note:** treat any request-derived free text as untrusted when composing this
command — never paste a raw, unvalidated title containing `"`, backticks, or `$(...)`
directly into a shell argument; quote/escape it or pass it via a heredoc as shown in this
skill's own usage above.

`gh` prints the new issue URL — capture it (and the issue number, parsed from the URL) for
later comments and the final report.

## Step 4 — Fast-path decision: does this issue need a spec + plan?

**Fast-path (no spec/plan)** only when **ALL** are true:

- The work is a single, well-understood change
- Acceptance criteria fit in one or two sentences
- No new public interface (per `savvagent-development`'s Non-Negotiable Rule 6: SPP wire
  format, tool MCP schema, plugin ABI, slash command, env var, on-disk format), no
  migration, no schema change
- No real design decision — an implementer could build it correctly from the AC alone

Concrete fast-path examples: fix a typo, flip a documented constant, fix a type error with
no behavior change, rename a local, delete verified-dead code.

If fast-pathed: skip Steps 5–6. The issue is complete after Step 3. Note in your report:
`No spec/plan — fast-path trivial.`

If you're rationalizing a 3-file change or anything with a migration / new interface /
multi-step AC into the fast-path → **STOP, write the spec.** The fast-path is for genuine
triviality, not "feels small."

## Step 5 — Spec: write, review, post (spec BEFORE plan)

Compose the spec in working memory (no repo file — this is a comment on the issue, not a
committed doc; `savvagent-development` is the skill that produces the committed
`docs/superpowers/specs/*-design.md` once the issue is actually being implemented).
Required sections, in order:

1. **Brief** — the problem + the issue's AC, quoted
2. **Assumptions** — every choice made without asking, one-line rationale each
3. **Goal & Success Criteria** — one paragraph + 3–5 measurable bullets
4. **Scope (In / Out)** — explicit non-goals
5. **Approach** — components, data flow, key interfaces; reference this repo's existing
   crate layout and conventions where they apply (see `savvagent-development`'s
   "Load-Bearing Invariants")
6. **Error Handling & Edge Cases**
7. **Testing Approach**
8. **Risks & Open Questions**

**Review it with a subagent before posting:**

```
task tool:
  agent_type: general-purpose
  description: "Review issue spec"
  prompt: |
    You are a spec reviewer. Verify this spec is complete and ready to plan against.
    Spec (full text inline — there is no file):
    <PASTE FULL SPEC>

    Check: completeness (no TODO/TBD/placeholders), internal consistency, clarity
    (ambiguity that would cause the wrong thing to get built), scope (single focused
    piece of work), YAGNI (no unrequested features), alignment with the issue's AC
    (issue #<n>). Only flag issues that would cause real problems during planning.
    Approve otherwise.

    Output:
    ## Spec Review
    Status: Approved | Issues Found
    Issues: - [Section]: [issue] - [why it matters]
    Recommendations (advisory): - [...]
```

**Max 2 revision rounds (3 dispatches total; the first dispatch is round 0).** On *Issues
Found*, revise in working memory and re-dispatch with the updated text. Remaining issues
after the third pass go into `Risks & Open Questions` — do not loop further.

**When it converges, post the spec as a comment:**

```bash
gh issue comment <n> --repo <repo> --body "[Spec]

<full finalized spec, markdown>"
```

Post once, on the approved version — do not spam the issue with per-round drafts.

## Step 6 — Plan: write, review, post (AFTER the spec comment)

Compose the plan in working memory — **do not write a repo file** at this stage (that
happens under `savvagent-development`'s plan-by-plan convention once implementation
starts, at `docs/superpowers/plans/`). Each task should carry concrete steps and, where
they apply, this repo's own reminders: the host-swap `RwLock` rule, the provider transport
split, the `ProgressDispatcher` forwarder-abort pattern, TDD step ordering, and the exact
test/lint commands (`cargo test --workspace`, `cargo clippy --workspace --all-targets`,
`cargo fmt --all --check`) — see `savvagent-development`'s "Load-Bearing Invariants" and
"Repository Conventions" for the authoritative list.

**Review it with a subagent before posting:**

```
task tool:
  agent_type: general-purpose
  description: "Review issue plan"
  prompt: |
    You are a plan reviewer. Verify this plan is complete and buildable.
    Plan (full text inline): <PASTE FULL PLAN>
    Spec for reference (full text inline): <PASTE FULL SPEC>

    Check: completeness, spec alignment, task decomposition, buildability, and
    this repo's project-specific requirements (host-swap RwLock rule, provider
    transport split, ProgressDispatcher forwarder-abort pattern, crate boundaries)
    where relevant. Only flag issues that would make an implementer build the wrong
    thing or get stuck.

    Output:
    ## Plan Review
    Status: Approved | Issues Found
    Issues: - [Task X, Step Y]: [issue] - [why it matters]
    Recommendations (advisory): - [...]
```

Same revision shape as the spec — **max 2 revision rounds (3 dispatches total)**; the first
dispatch is round 0. Unresolved issues after the third pass go into a `## Known Plan Gaps`
section at the top.

**When it converges, post the plan (after the `[Spec]` comment already exists):**

```bash
gh issue comment <n> --repo <repo> --body "[Plan]

<full finalized plan, markdown>"
```

## Step 7 — Report

Output one concise message:

```
Issue created.

#<n> — <title> — <bug|enhancement>
URL: https://github.com/<repo>/issues/<n>
Assignee: <login, or unassigned>
Priority: <label, or "n/a — no priority labels configured">   (defects)
Spec: [Spec] comment posted   (or "fast-path — none")
Plan: [Plan] comment posted   (or "fast-path — none")

Assumptions worth reviewing:
- <bullet>
```

Then STOP. Do not start implementing the issue — that's `savvagent-development`'s job.

## Common Rationalizations (all are violations)

| Excuse | Reality |
|---|---|
| "I'll create it, they'll assign it later" | Assign at creation. Default is `@me` unless directed otherwise. |
| "No priority labels exist, skip noting urgency" | State urgency in the body even without a label, so it isn't lost. |
| "It's a feature, no acceptance criteria needed" | Every issue gets Acceptance Criteria, defect or feature. |
| "I'll write the plan, the spec is implied" | Spec before plan, always. The plan is reviewed against the spec. |
| "Post both, order doesn't matter" | Spec comment first, then plan comment. The plan reviewer needs the spec already settled. |
| "Skip the spec review, it reads fine" | Subagent review catches gaps you can't see in your own draft. One dispatch. |
| "This 3-file feature is basically trivial" | Fast-path is 1 well-understood change, AC in 1–2 sentences, no new interface/migration. Otherwise write the spec. |
| "I'll add JIRA support / a tracker switch here" | This repo has no JIRA. GitHub Issues only — do not reintroduce a tracker abstraction. |
| "I'll create it then start coding" | This skill creates issues. Hand off to `savvagent-development` to build. |

## Red Flags — STOP

- About to call `gh issue create` with no `--assignee` when one was requested (and you
  weren't told to leave it unassigned)
- About to create a defect and silently drop urgency information because no priority label
  exists
- About to create an issue with no Acceptance Criteria
- About to post a `[Plan]` comment before a `[Spec]` comment exists
- About to skip spec/plan on something with a migration, new interface, or multi-step AC
- About to skip the subagent review of the spec or plan
- About to start implementing the issue you just created
- About to reintroduce JIRA or a tracker-switch config file into this repo's workflow

Each = stop, do the step, continue.

## Cross-references

- `savvagent-development` — autonomous build/ship of an existing issue in this repo;
  consumes the `[Spec]`/`[Plan]` comments this skill posts, and owns the *committed*
  `docs/superpowers/specs/` / `docs/superpowers/plans/` documents once implementation
  starts

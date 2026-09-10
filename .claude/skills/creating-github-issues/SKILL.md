---
name: creating-github-issues
description: Use when asked to create, file, open, or log a new ticket — a bug/defect or a feature/enhancement — as a GitHub issue in this repo, as opposed to working an existing one. Handles type selection via labels, an optional assignee, and a duplicate check. For developing/shipping an EXISTING issue (spec → plan → implement → PR → review → merge → release), use `otto-development` instead.
---

> **Ported for Claude Code** from `.github/skills/creating-github-issues/SKILL.md`, which is the canonical
> body maintained for the GitHub Copilot CLI. Edit the canonical file, then re-port — do not edit
> this file in place. CI verifies the two stay in step
> (`bash .github/scripts/check-claude-skill-ports.sh`).

# Creating GitHub Issues

Create a new GitHub issue in `savvagent/otto` (bug or feature) correctly the first
time: right label, a clear reproduction/acceptance-criteria description, and a
duplicate check — without inventing tracker concepts this repo doesn't have.

This skill **creates** issues. It is the upstream counterpart to
`otto-development`, which **works** existing issues end-to-end. If an issue
already exists and you're implementing it, you're in the wrong skill — load
`otto-development` instead.

## This repo is GitHub Issues only

There is no JIRA, Linear, or other tracker abstraction here — see `CLAUDE.md`
and `otto-development`'s own conventions. Every ticket-creation action in this
skill is a plain `gh issue` call against `savvagent/otto`. Do not introduce a
`.claude/tracker.json`-style tracker switch; it would be dead code in a
single-tracker repo.

Unlike some generic ticket-creation workflows, this repo's GitHub issues carry
**no priority or estimate fields** (native or via label) — the label set is
just the stock GitHub defaults (`bug`, `documentation`, `enhancement`, `good
first issue`, `help wanted`, `question`, and a few `wontfix`/`invalid`/
`duplicate` housekeeping labels; run `gh label list --repo savvagent/otto` if
that set may have changed). Don't invent `priority:*` or `estimate:*` labels
for this repo unless the user explicitly asks for them created.

## The Iron Law

**Every created issue is typed (via label) and checked for duplicates before
you report it created.** A bare issue with no type label, or one that silently
duplicates an already-open issue, is not done.

## Step 1 — Map the request to a type label

- "bug", "broken", "regression", "error", "crash", "doesn't work" → `bug`
- "feature", "add", "support", "as a user", "new", "enhancement" → `enhancement`
- Pure documentation change → `documentation`

If genuinely ambiguous, pick the better fit and note the choice in your final
report — do not block the create on it. Confirm the label still exists first:
`gh label list --repo savvagent/otto` (cheap, avoids a failed
`--label` flag).

## Step 2 — Check for an obvious duplicate

```bash
gh issue list --repo savvagent/otto --state open --search "<key terms>"
```

If a clear match exists, surface it and ask whether to proceed rather than
silently filing a second copy. A near-miss that's actually a different facet
of the same area is not a duplicate — use judgment, don't over-block.

## Step 3 — Write the body and create the issue

- **Bug** body: *Steps to reproduce* → *Expected* → *Actual*, then an
  *Acceptance criteria* checklist (the fix's done-condition).
- **Feature** body: a short *Summary* of the user value, then an *Acceptance
  criteria* checklist. Mirror the shape already used by this repo's open
  issues (`gh issue view <n> --repo savvagent/otto` on a recent one is a good
  template check) — a `## Summary`, optional `## Details`/`## Constraint`
  section, and a `## Acceptance criteria` checklist is the established house
  style here.

```bash
gh issue create --repo savvagent/otto \
  --title "<concise, imperative title>" \
  --body "<body with Acceptance criteria checklist>" \
  --label bug \                    # or: enhancement / documentation
  --assignee <login>                # omit entirely if no assignee was requested — see below
```

**Assignee default: leave unassigned.** This repo's existing open issues are
consistently unassigned (`gh issue list --repo savvagent/otto` shows no
assignees) — that's the house convention, unlike trackers that default to a
specific person. Only pass `--assignee` when the request names someone (a
GitHub login, or `@me` for "assign it to me").

`gh` prints the new issue's URL on success — capture it for the report.

## Step 4 — Non-trivial work: don't write a spec/plan here

Some ticket-creation workflows write and post a full spec + plan at creation
time. **This repo does not** — `otto-development` already owns spec → plan →
implement as committed docs (`docs/superpowers/specs/`,
`docs/superpowers/plans/`) produced *when the issue is picked up for
development*, each spec/plan going through its own critique rounds at that
time. Writing a second, throwaway spec/plan as an issue comment at creation
time would duplicate that work and could drift from what actually gets
implemented.

Instead, if the request describes something non-trivial (multi-file, a new
public interface, a migration, or a real design decision), just say so
plainly in the issue body — e.g. a one-line `## Design note` calling out that
this will need a design spec before implementation — so whoever (or whichever
skill) picks it up next knows to start with `otto-development`'s spec step
rather than jumping straight to code. Do not draft the spec itself here.

## Step 5 — Report

Output one concise message:

```
Issue created.

#<n> — <title> — <bug|enhancement|documentation>
URL: https://github.com/savvagent/otto/issues/<n>
Assignee: <login, or unassigned>
Duplicate check: no obvious duplicate found  (or: see #<m>, proceeded because …)

Assumptions worth reviewing:
- <bullet, if any>
```

Then STOP. Do not start implementing the issue — that's `otto-development`'s
job.

## Common Rationalizations (all are violations)

| Excuse | Reality |
|---|---|
| "Label's obvious, skip it" | Every issue gets a type label. Confirm it exists, then set it. |
| "I'll just search my memory for duplicates" | Run the `gh issue list --search` check. Memory of "open issues" goes stale fast. |
| "This repo probably has priority labels like other trackers" | It doesn't — check `gh label list` before assuming, and don't invent new ones. |
| "I'll write the spec now since I'm already thinking about the design" | Spec/plan authorship belongs to `otto-development`, produced (and critiqued) when the issue is actually picked up, not at creation time. |
| "I'll create it then start coding" | This skill creates issues. Hand off to `otto-development` to build. |
| "Default to assigning myself, like other trackers do" | This repo's issues default to unassigned. Only assign when asked. |

## Red Flags — STOP

- About to call `gh issue create` with no type label
- About to skip the duplicate-check search
- About to draft a `[Spec]`/`[Plan]`-style comment on a brand-new issue instead of noting it needs one later
- About to start implementing the issue you just created

Each = stop, do the step, continue.

## Cross-references

- `otto-development` — autonomous spec → plan → implement → PR → review →
  merge → release for an *existing* issue in this repo; the natural next step
  after this skill creates one.

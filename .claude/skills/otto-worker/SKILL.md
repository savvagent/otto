---
name: otto-worker
description: Use when picking up and working a single job from the otto-factory job queue for savvagent/otto — claiming exactly one job, dispatching a subagent to run it end-to-end via otto-development, and reporting the outcome back to otto-factory (complete_job/fail_job) plus a concise summary to the user. Trigger on "pick up a job", "work the next job", "process an otto-factory job", "claim a job and work it", "run otto-worker". Not for scanning/filing GitHub issues into the queue (otto-scanner) or for the per-job spec → plan → implement → PR → review → merge → release mechanics themselves (otto-development, which the dispatched subagent invokes).
---

# Otto Worker

This skill is the bridge between the otto-factory job queue and actually getting
a job done: it claims one job for `savvagent/otto`, hands it to a subagent that
works it end-to-end via `otto-development`, and reports the result back to
otto-factory. It is Claude-Code-native — it orchestrates via the `Agent` tool
and calls `otto-factory` MCP tools that have no equivalent in this repo's
Copilot-CLI-flavored canonical skills, so like `otto-scanner` it has no
`.github/skills/` counterpart to stay in sync with (see `CLAUDE.md`'s
`NATIVE_SKILLS` section).

This skill only **claims, dispatches, and resolves** a job. It never scans or
files GitHub issues (that's `otto-scanner`), and it never writes the spec,
plan, code, or PR itself — that is entirely `otto-development`'s job, run by a
subagent this skill dispatches.

## The Iron Law

**Every job this skill claims is either `complete_job`'d or `fail_job`'d
before this skill finishes, and exactly one job is claimed per run.** A job
left claimed with no resolution blocks it from ever being retried or reported
on; claiming a second job "while you're at it" doubles the blast radius of a
single run going wrong. If the run is interrupted after claiming but before
resolving, resolving that job is the first thing the next invocation must do
before claiming anything new.

## Context discipline: the real work happens in one subagent

The orchestrating session (you, reading this skill) must stay small: it reads
job *metadata* (id, title, ticket ref, branch name) and one-line results,
never full job descriptions rendered as prose, full diffs, or PR review
transcripts. The entire spec → plan → implement → PR → review → merge →
release lifecycle happens inside a single `Agent` tool call to a subagent that
was handed everything it needs up front — the orchestrator does not watch it
work, does not re-read its intermediate output, and does not do any of that
work itself. If you find yourself about to open a file, run `cargo`, or call
`gh pr` directly in the orchestrator, stop — that belongs in the subagent.

## Step 1 — Identify the repo

1. Call `whoami` to confirm which organization this token opens.
2. Call `resolve_repo` with `remote` set to the output of
   `git remote get-url origin` (`https://github.com/savvagent/otto.git`).
3. If that fails to resolve, call `list_repos`; if `savvagent/otto` is truly
   unregistered, `register_repo` it before continuing.

Keep the resolved repo slug — every following otto-factory call needs it.

## Step 2 — Claim exactly one job

1. Call `ready` for the resolved repo slug to see claimable work. If nothing
   is ready, report that (Step 6) and stop — there is nothing to do.
2. Call `claim_jobs` for **one** job. Never claim more than one in a single
   run: a second claimed-but-unworked job is exactly the orphaned-job failure
   the Iron Law exists to prevent.
3. Note the job's id, title, description, `ticketRef` (if any — this is
   normally `savvagent/otto#<n>` when the job came from `otto-scanner`), and
   any `metadata`. This is the only job content the orchestrator holds onto;
   it gets handed to the subagent whole in Step 4, not re-fetched or
   re-summarized later.

If claiming fails (another agent took it first), go back to `ready` and try
the next candidate rather than giving up immediately.

## Step 3 — Lease the branch once the subagent names it

The subagent's first action (Step 4 below) is to establish which branch it
will work on (a fresh feature branch off `origin/main`, per
`otto-development`'s worktree convention, or an existing branch if the job is
"address review comments on an already-open PR"). Once the subagent reports
that branch name back:

1. `acquire_lease` on `branch:<name>` for this repo.
2. Renew it periodically while the subagent is still running (`renew_lease`)
   — a job worth running usually outlives a single lease TTL.
3. `release_lease` once the job is resolved in Step 5, success or failure.

Leases are advisory (see the otto-factory server instructions) — this step
makes a collision with another agent visible, it doesn't prevent one.

## Step 4 — Dispatch one subagent to do the actual work

Launch exactly one subagent (a fresh `general-purpose` agent via the `Agent`
tool — it needs no prior context from this session) with a fully
self-contained prompt. It will not see anything above this point, so the
prompt must carry everything it needs:

```
Work otto-factory job <job-id> for savvagent/otto end-to-end using this
repo's `otto-development` skill.

Job title: <title>
Job description: <full description, verbatim>
GitHub issue: <ticketRef, if present, e.g. "savvagent/otto#123" — use this as
  the issue to work from in otto-development's intake step. If no ticketRef
  is present, treat the title/description above as the plain task brief
  otto-development also accepts.>

Requirements:
- Work in your own isolated worktree per otto-development's own Phase 0
  worktree convention (`.claude/worktrees/<branch>`) — never the shared main
  checkout.
- Use `otto-development` for the full lifecycle: spec, plan, implementation,
  PR, the mandatory review loop, fixing findings, merge, and — per this
  repo's normal cadence — cutting a release, unless otto-development's own
  rules say a release isn't warranted for this change.
- Use `git-expert` for multi-step git/GitHub mechanics (branch, commit, push,
  PR, merge) per this repo's CLAUDE.md.
- Do not add any attribution to commits or the PR description — no
  Co-Authored-By trailer, no "Generated with" footer, no bot marker. Omit it
  silently.
- As your very first action, report back (in your normal turn output) the
  exact branch name you're about to work on, before you do anything else,
  so the orchestrator can lease it.
- When you finish, report: outcome (shipped and merged / blocked / failed
  and why), the PR URL, whether a release was cut and its version, and the
  branch name (again, for lease release).
```

Wait for this subagent to finish. Do not poll it, do not re-derive its
progress from `gh` calls in the orchestrator — its final report is the only
thing Step 5 needs.

## Step 5 — Resolve the job on otto-factory

Based on the subagent's final report:

- **Shipped and merged** → `complete_job` with a `resultSummary` naming the PR
  URL and release version (if any).
- **Blocked or failed** → `fail_job` with the subagent's stated reason. Do not
  silently retry it in the same run — a failed job needs a human look, the
  same convention `otto-scanner` applies to failed/cancelled jobs it finds
  already on the queue.
- **The subagent asked to cancel** (e.g. the job turned out to be already
  done, or a duplicate) → `request_cancel` with the reason instead of forcing
  a `fail_job`.

Then `release_lease` on the branch from Step 3, regardless of outcome.

## Step 6 — Report

Output one concise summary:

```
Otto Worker — savvagent/otto

Job <job-id> — <title>
Outcome: shipped (PR #<n>, released as v<x.y.z>) | failed (<reason>) | cancelled (<reason>) | nothing ready to claim
Branch: <name>
```

Then STOP. Do not claim another job in the same run — that's a separate
invocation of this skill.

## Common Rationalizations (all are violations)

| Excuse | Reality |
|---|---|
| "I'll claim two jobs since I'm already here" | Exactly one job per run. A second claimed job with no subagent working it is an orphaned claim. |
| "I'll just read the job description myself to see if it's worth doing" | The description goes straight into the subagent's prompt. Reading it to decide isn't the orchestrator's job — claiming already committed you to it. |
| "The subagent's taking a while, let me check `gh pr view` on its branch" | That's re-deriving progress in the orchestrator. Wait for its final report. |
| "It failed, but I can see the fix, let me just patch it here" | The orchestrator does not touch code. Either dispatch a follow-up subagent or `fail_job` it for a human. |
| "I'll skip releasing the lease since the job's done anyway" | Release it in Step 5 regardless of outcome — a stuck lease blocks the next agent from working that branch. |
| "No `ticketRef` on this job, I'll invent one so otto-development has an issue to close" | Don't fabricate a tracker reference. Pass the job's title/description as a plain task brief — otto-development accepts that too. |

## Red Flags — STOP

- About to claim a second job before the first is resolved
- About to run `cargo`, `gh`, or edit a file directly in the orchestrator
  instead of inside the dispatched subagent
- About to leave a claimed job without calling `complete_job`, `fail_job`, or
  `request_cancel`
- About to skip `release_lease` after the job is resolved
- About to dispatch more than one subagent for a single job

Each = stop, do the step correctly, continue.

## Cross-references

- `otto-development` — the actual spec → plan → implement → PR → review →
  merge → release lifecycle, run by the subagent this skill dispatches. This
  skill never duplicates those mechanics.
- `otto-scanner` — the upstream counterpart that gets issues onto the queue
  this skill claims from; also the convention this skill mirrors for leaving
  a failed/cancelled job for a human rather than silently retrying it.

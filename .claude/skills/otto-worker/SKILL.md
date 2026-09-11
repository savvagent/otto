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
job *metadata* (id, title, ticket ref) and the subagent's one-line final
result, never full job descriptions rendered as prose, full diffs, or PR
review transcripts. The entire spec → plan → implement → PR → review → merge →
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

## Step 3 — The subagent owns its own branch lease

The orchestrator dispatches the subagent in Step 4 with a single `Agent`
call, which blocks until the subagent's entire run is finished and returns
only once, at the end — there is no channel for the orchestrator to learn the
branch name mid-run or to interleave `renew_lease` calls while that one call
is still outstanding. So leasing is not something the orchestrator does at
all: the subagent itself acquires `branch:<name>` (`acquire_lease`, for this
repo) as its own first action once it knows the branch name, renews it
periodically over the course of its own long-running work, and releases it
right before it reports back to the orchestrator — a self-contained step,
same as everything else in its prompt (Step 4 spells this out). Leases are
advisory (see the otto-factory server instructions) — this makes a collision
with another agent visible, it doesn't prevent one.

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
- As soon as your branch name is decided (otto-development's Phase 0), call
  the otto-factory MCP tool `acquire_lease` on `branch:<name>` for the
  savvagent/otto repo (resolve the repo slug yourself via `whoami`/
  `resolve_repo` first, same as the orchestrator did). Renew it
  (`renew_lease`) periodically over the course of your work, and
  `release_lease` as your very last action before you report back — you are
  the only one who can interleave lease calls with your own long-running
  work, so this whole lifecycle is yours, not the orchestrator's.
- Use `git-expert` for multi-step git/GitHub mechanics (branch, commit, push,
  PR, merge) per this repo's CLAUDE.md.
- Do not add any attribution to commits or the PR description — no
  Co-Authored-By trailer, no "Generated with" footer, no bot marker. Omit it
  silently.
- When you finish (lease already released), report: outcome (shipped and
  merged / blocked / failed and why), the PR URL, whether a release was cut
  and its version, and the branch name.
```

Wait for this subagent to finish. Do not poll it while it's running, do not
re-derive its progress from `gh` calls in the orchestrator mid-run — its
final report is what drives Step 5. Step 5 below does make exactly one
verification call against that report before trusting it, which is not
polling: it happens once, after the subagent has already finished.

## Step 5 — Resolve the job on otto-factory

Based on the subagent's final report:

- **Shipped and merged** → before calling `complete_job`, independently
  verify the claim with `gh pr view <PR-number> --repo savvagent/otto --json
  state,mergedAt` and confirm `state` is `MERGED`. Do not take "shipped and
  merged" on the subagent's word alone — a subagent reporting success on a PR
  that was never actually merged is a known failure mode in this repo's own
  history, and `complete_job` on an unmerged job is exactly the wrong
  resolution to make irreversible. If verification fails (PR open, or
  doesn't exist), treat this the same as **Blocked or failed** below rather
  than completing it. Once verified, `complete_job` with a `result` string
  naming the PR URL and release version (if any).
- **Blocked or failed** → `fail_job` with the subagent's stated reason. Do not
  silently retry it in the same run — a failed job needs a human look, the
  same convention `otto-scanner` applies to failed/cancelled jobs it finds
  already on the queue.
- **The subagent asked to cancel** (e.g. the job turned out to be already
  done, or a duplicate) → `request_cancel` with the reason instead of forcing
  a `fail_job`.

The subagent already released its own lease (Step 3/4) before reporting back,
so there is nothing left to release here.

## Step 6 — Report

If a job was claimed and worked, output:

```
Otto Worker — savvagent/otto

Job <job-id> — <title>
Outcome: shipped (PR #<n>, released as v<x.y.z>) | failed (<reason>) | cancelled (<reason>)
Branch: <name>
```

If Step 2 found nothing ready to claim, skip the job/branch lines entirely:

```
Otto Worker — savvagent/otto

Nothing ready to claim.
```

Then STOP. Do not claim another job in the same run — that's a separate
invocation of this skill.

## Common Rationalizations (all are violations)

| Excuse | Reality |
|---|---|
| "I'll claim two jobs since I'm already here" | Exactly one job per run. A second claimed job with no subagent working it is an orphaned claim. |
| "I'll just read the job description myself to see if it's worth doing" | The description goes straight into the subagent's prompt. Reading it to decide isn't the orchestrator's job — claiming already committed you to it. |
| "The subagent's taking a while, let me check `gh pr view` on its branch" | That's re-deriving progress *while it's still running*. Wait for its final report — the one `gh pr view` call Step 5 makes happens only after that report, to verify it, not to watch progress. |
| "It said 'shipped and merged', that's good enough for `complete_job`" | Verify it with `gh pr view --json state,mergedAt` first (Step 5). A subagent reporting success on a PR that was never actually merged is a known failure mode here — `complete_job` is not reversible. |
| "It failed, but I can see the fix, let me just patch it here" | The orchestrator does not touch code. Either dispatch a follow-up subagent or `fail_job` it for a human. |
| "I'll acquire the lease myself once the subagent tells me the branch name" | The `Agent` call blocks until the subagent's entire run finishes — there's no point where the orchestrator can act on a mid-run report. The subagent leases its own branch. |
| "No `ticketRef` on this job, I'll invent one so otto-development has an issue to close" | Don't fabricate a tracker reference. Pass the job's title/description as a plain task brief — otto-development accepts that too. |

## Red Flags — STOP

- About to claim a second job before the first is resolved
- About to run `cargo`, `gh`, or edit a file directly in the orchestrator
  instead of inside the dispatched subagent
- About to leave a claimed job without calling `complete_job`, `fail_job`, or
  `request_cancel`
- About to try acquiring or renewing a lease from the orchestrator instead of
  telling the subagent to own its own lease lifecycle
- About to dispatch more than one subagent for a single job
- About to call `complete_job` on a "shipped and merged" report without
  first confirming it with `gh pr view --json state,mergedAt`

Each = stop, do the step correctly, continue.

## Cross-references

- `otto-development` — the actual spec → plan → implement → PR → review →
  merge → release lifecycle, run by the subagent this skill dispatches. This
  skill never duplicates those mechanics.
- `otto-scanner` — the upstream counterpart that gets issues onto the queue
  this skill claims from; also the convention this skill mirrors for leaving
  a failed/cancelled job for a human rather than silently retrying it.

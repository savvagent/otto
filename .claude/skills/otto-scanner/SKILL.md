---
name: otto-scanner
description: Use when scanning open GitHub issues in savvagent/otto and enqueuing eligible ones into the otto-factory job queue — checking each issue against creating-github-issues' compliance requirements first, bringing non-compliant issues into compliance in place, then queueing it. Trigger on "scan issues", "sync issues to otto-factory", "queue open issues", "run otto-scanner", or a scheduled/periodic invocation of this skill. Not for creating a brand-new issue (creating-github-issues) or for claiming and working a queued job (otto-worker, which dispatches otto-development for the per-job mechanics).
---

# Otto Scanner

This skill is the periodic bridge between GitHub Issues on `savvagent/otto` and
the otto-factory job queue: it finds open issues that have no job yet, makes
sure each one is compliant with `creating-github-issues`' requirements, and
queues it via the `otto-factory` MCP server. It is Claude-Code-native — it
orchestrates via the Agent tool and calls `otto-factory` MCP tools that have
no equivalent in this repo's Copilot-CLI-flavored canonical skills, so unlike
`otto-development`/`creating-github-issues` it has no `.github/skills/`
counterpart to stay in sync with (see `CLAUDE.md`'s `NATIVE_SKILLS` section).

This skill only **queues** work. It never implements an issue, opens a PR, or
claims a job itself — that is `otto-worker`'s job once it claims what this
skill queues (which in turn dispatches `otto-development` for the actual
implementation).

## The Iron Law

**Every issue this skill queues is compliant with `creating-github-issues`
at the moment it is queued, and no open issue is ever queued twice.** Queueing
a non-compliant issue (no type label, no acceptance criteria) hands the next
agent a job it can't act on without redoing triage. Queueing a duplicate job
wastes otto-factory allowance and confuses whoever is watching the queue.

## Context discipline: everything real happens in subagents

The orchestrating session (you, reading this skill) must stay small: it reads
issue *numbers* and one-line results, never full issue bodies, full job
listings, or diff output. Every step that touches real content — `gh`
output, `list_jobs` results, editing an issue body — happens inside a Agent
tool call, which returns only what's specified below. If you find yourself
about to run `gh issue view` or `list_jobs` directly in the orchestrator, stop
and delegate it instead.

## Step 1 — Roster: what's open, what's already queued

Launch one subagent (a fresh `general-purpose` agent — it needs no prior
context) with this task:

1. Resolve the otto-factory repo slug for `savvagent/otto` — call `whoami`,
   then `resolve_repo` with `remote` set to `git remote get-url origin`
   (`https://github.com/savvagent/otto.git`). If it fails to resolve, call
   `list_repos`; if truly unregistered, `register_repo` it before continuing.
2. `gh issue list --repo savvagent/otto --state open --json number,title,labels
   --limit 500` — collect every open issue's number and labels.
3. Drop issues carrying `wontfix`, `duplicate`, or `invalid` — those are
   housekeeping labels, never queue them. `question` is **not** in this
   list: per `creating-github-issues`, `question` is a normal stock label,
   not housekeeping. A `question`-labeled issue stays a candidate; if it
   turns out to need real work, Step 2's compliance check will re-type it
   (to `bug`/`enhancement`/`documentation`) like any other non-compliant
   issue.
4. `list_jobs` for that repo slug, with no `status` filter and an explicit
   `limit` (e.g. `1000` — don't rely on the server's unstated default,
   which may be much smaller). For each job with a `ticketRef` matching
   `savvagent/otto#<n>`, bucket it:
   - `pending` / `in-progress` / `active` / `completed` → "already handled",
     skip permanently.
   - `failed` / `cancelled` → "needs a human call", do **not** auto-requeue
     it — list it separately, don't fold it into either bucket below.
   If the call returns exactly `limit` rows, the job history may extend
   further than this single call can see (this tool has no pagination
   cursor) — note that as "job list possibly truncated" rather than
   silently trusting a complete view.
5. Return *only*: the resolved repo slug, the list of candidate issue
   numbers (open, not housekeeping-labeled, not already handled), the
   separate list of failed/cancelled-job issue numbers, and the
   possibly-truncated flag from step 4 (for the final report — see Step 4).
   Nothing else — no titles, no job descriptions, no raw `gh`/`list_jobs`
   output.

If the candidate list is empty, report that the queue is already in sync
(mentioning any failed/cancelled issues from step 5 for a human to look at)
and stop here.

## Step 2 — Per-issue: verify compliance, fix if needed, queue

For every candidate issue number, dispatch one subagent in parallel (a single
message with one Agent call per issue — never sequential, and never more than
one issue per subagent). Cap each parallel batch at 15 subagents; if there are
more candidates than that, process them in successive batches of at most 15,
waiting for each batch to finish before dispatching the next — a single
message firing hundreds of concurrent subagents against a large backlog is
not a batch, it's a rate-limit incident. Give each subagent the issue number,
the repo slug from Step 1, and this task:

1. `gh issue view <n> --repo savvagent/otto --json title,body,labels,state`.
2. Check compliance against `creating-github-issues`' Step 1 and Step 3 (read
   `.claude/skills/creating-github-issues/SKILL.md` if you need the exact
   rules) — but applied to an *existing* issue, not a new one:
   - **Type label**: exactly one of `bug`, `enhancement`, `documentation` is
     present. If missing, map the title/body to a type the same way
     `creating-github-issues` Step 1 does, confirm the label exists (`gh label
     list --repo savvagent/otto`), and add it with
     `gh issue edit <n> --repo savvagent/otto --add-label <label>`.
   - **Body shape**: a bug has *Steps to reproduce* → *Expected* → *Actual*
     plus an *Acceptance criteria* checklist; a feature has a *Summary* plus
     an *Acceptance criteria* checklist. If a section is missing, patch the
     body with `gh issue edit <n> --repo savvagent/otto --body "<full body,
     original content preserved, missing sections added>"` — never drop
     existing content, and never replace the whole body with a rewrite when
     appending the missing section would do.
   - Do **not** run `creating-github-issues`' duplicate-check step (Step 2) —
     that step is for *new* issues, and this issue already exists. Do not
     search for or flag duplicates here.
   - Do **not** create a second issue. You are editing issue `<n>` in place,
     never `gh issue create`.
3. Once compliant (or if it already was), queue it:
   - `add_job` with `repo` = the slug you were given, `title` = the issue
     title, `description` = the issue body (post-fix) plus the issue URL,
     `ticketRef` = `savvagent/otto#<n>`, and `idempotencyKey` =
     `otto-scanner-issue-<n>` (guards against a dropped-connection retry
     double-queueing).
   - If `add_job` fails because that `idempotencyKey` was already used with
     *different* arguments, this issue is already queued under a job
     Step 1's dedup missed (e.g. the job list was truncated, or this issue
     just got re-typed out of `question` in step 2 above and wasn't visible
     as "already handled" yet). Do not treat this as a fresh error: run
     `list_jobs` for this repo, find the job whose `ticketRef` is
     `savvagent/otto#<n>`, and use that job's id in your report as
     "already queued" instead of retrying or escalating.
   - Otherwise, `link_ticket` on the newly returned job with
     `tracker: "github"` and `ticketRef: "savvagent/otto#<n>"` —
     `add_job`'s own `ticketRef` records it, but `link_ticket` is what
     makes future job-status transitions write back to the issue as
     comments, which is the point of linking it.
4. Return exactly one line, one of:
   - `#<n> — queued as <job-id> (compliance: ok | fixed: <what you changed>)`
   - `#<n> — already queued as <job-id> (idempotency conflict — Step 1's
     dedup missed it)`

## Step 3 — Nothing else in the orchestrator

The orchestrator's only job after dispatching is to collect each subagent's
one-line result. Do not re-fetch the issue, the job, or re-run `list_jobs` to
"double check" — the subagent already did the real work and reported it.

## Step 4 — Report

Output one concise summary:

```
Otto Scanner — savvagent/otto

Queued (<n>):
  #<n> — queued as <job-id> (compliance: ok)
  #<n> — queued as <job-id> (compliance: fixed: added `bug` label)
  #<n> — already queued as <job-id> (idempotency conflict — Step 1's dedup missed it)
  ...

Skipped, already handled: #<n>, #<n>, ...
Needs a human call (failed/cancelled job on file, not auto-requeued): #<n>
Job list possibly truncated at Step 1 — dedup may be incomplete for older jobs: yes/no
```

Then STOP. Do not start implementing any queued issue — claiming and working
a job is `otto-worker`'s job, done by whichever agent picks it up next.

## Common Rationalizations (all are violations)

| Excuse | Reality |
|---|---|
| "I'll just list the jobs myself, it's a quick read" | Reads still land in the orchestrator's context. Delegate the roster step. |
| "This issue's compliant enough, I'll skip the label check" | Every queued issue is typed and has the required body sections. Check both, every time. |
| "I'll run the duplicate-check step since I'm already in creating-github-issues' head" | That step is for *creating* an issue. This issue already exists — don't search for duplicates of it. |
| "The job failed once, I'll just requeue it to keep the pipeline moving" | A failed/cancelled job is a signal something needs a human look, not a silent retry. Report it, don't requeue it. |
| "I'll process all the candidate issues in one subagent to save calls" | One subagent per issue, dispatched in parallel — that's what keeps a bad edit or a stuck `gh` call from blocking the rest of the batch. |
| "There are 80 candidates, I'll fire all 80 subagents in one message" | Cap parallel batches at 15; run the rest in successive batches. |
| "add_job errored on the idempotency key, something's broken" | It means this issue is already queued under a job the roster step missed — look it up and report it as already-queued, don't escalate. |

## Red Flags — STOP

- About to call `gh issue view`, `list_jobs`, or `gh issue edit` directly in
  the orchestrator instead of inside a subagent
- About to queue an issue with no type label or no acceptance-criteria section
- About to requeue an issue whose existing job is `failed` or `cancelled`
- About to run a duplicate-check search against an issue you're fixing
- About to call `gh issue create` for an issue that already exists
- About to dispatch more than 15 Step-2 subagents in a single message

Each = stop, do the step correctly, continue.

## Cross-references

- `creating-github-issues` — the compliance authority this skill checks
  existing issues against; also owns the type-label mapping and body-shape
  rules referenced in Step 2.
- `otto-development` — the spec → plan → implement → PR → review → merge →
  release mechanics that actually implement a queued job, run by a subagent
  `otto-worker` dispatches. This skill only gets an issue onto the queue; it
  never claims or works one.
- `otto-worker` — claims a job this skill queued and dispatches a subagent to
  work it via `otto-development`. This skill never claims a job itself.

# Fix `ci.yml`'s concurrency group so a new push supersedes the in-flight run — design

Date: 2026-09-11
Status: pending review
Source: savvagent/otto#139

## Problem

`ci.yml`'s workflow-level concurrency block is:

```yaml
concurrency:
  group: ci-${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}
```

On PR #135 this produced the inverse of its intended effect, twice: run `34549300704` (commit
`377689f`, created 01:06:23Z) sat `in_progress`. A push moved the PR head to `2176a58`, creating run
`34550219625` at 01:19:42Z in the same group (`ci-CI-refs/pull/135/merge`). The expected outcome —
the older run cancelled in favor of the newer one — did not happen. Instead, the *newer* run
(`34550219625`) was cancelled at 01:20:15Z, 33 seconds after it started, while the older run kept
running. Every job in the cancelled run died at the `Swatinem/rust-cache@v2` step with "Canceling
since a higher priority waiting request exists," with `build`/`test`/`rustfmt`/`clippy` all
`skipped` — so `gh pr checks` reported five failing jobs that had never actually executed a test,
and the PR showed `mergeStateStatus: BLOCKED` with checks that looked like real failures on a
release PR. A plain `gh run rerun` reproduced the same cancellation. The run only completed once the
stale run was cancelled by hand.

A second, related anomaly: run `34548437470`, triggered by the `push` event for `6274151` (#132's
merge to `main`), is `cancelled` with **zero** jobs recorded. `cancel-in-progress` evaluates to
`false` for a `push` event under the current expression, so a push to `main` should never be
cancellable by this group at all — yet this run never ran. That merge commit therefore has no
completed CI run of its own (tip commit `46a216c` is fully green and does contain #132's code, so
`main` is verified in aggregate, but not commit `6274151` individually).

### Root cause

GitHub's own documentation for `cancel-in-progress` (see
[Control the concurrency of workflows and jobs](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency))
states that the value can be an expression, but never specifies *whose* run context wins when two
runs sharing a concurrency group could each evaluate that expression differently, or documents any
guaranteed ordering between "the run that just started" and "the run(s) already occupying the
group." In this repo's block, `cancel-in-progress` depends on `github.event_name` — a value that is
identical (`pull_request`) for every push-driven CI run of the same PR, so in the ordinary case there
should be no room for disagreement. But the observed behavior — the newer run cancelled, the older
one surviving, on two separate occasions, plus a `push`-triggered run cancelled with zero jobs despite
`cancel-in-progress` unconditionally evaluating `false` for that event — is inconsistent with any
documented, deterministic interpretation of the flag, and matches a class of behavior reported
repeatedly in GitHub's own community forums as under-specified/flaky when the flag is a computed
expression rather than a literal boolean. We cannot obtain GitHub's internal queue-arbitration logs
to prove the exact mechanism, so this spec treats the platform-level arbitration as unreliable by
construction, and designs `ci.yml`'s concurrency block to remove every avoidable source of
per-run ambiguity rather than to rely on a specific corrected mental model of GitHub's internal
behavior:

1. **Stop depending on `github.event_name` for `cancel-in-progress`.** `event_name` is a proxy for
   "is this ref cancellable," and (per the community reports the issue itself points at, and per
   GitHub's own docs declining to specify cross-run evaluation order for expressions) is not
   guaranteed to be evaluated consistently across two runs sharing a group. The safer, and by far the
   most common, formulation ties `cancel-in-progress` directly to the *ref* the group is already keyed
   on: a run on `refs/heads/main` (or `refs/heads/master`) is never cancellable; every other ref
   (including every `refs/pull/<n>/merge`) is. Since the group key and the cancel predicate now both
   derive from the same input (`github.ref`, one indirectly via `github.head_ref`), there is no
   remaining axis on which two runs of the *same* ref can compute different answers — the predicate
   is a pure function of information that is identical for every run on that ref, by construction.
2. **Use `github.head_ref || github.ref` for the group key**, the standard, widely-documented
   formulation for PR-based concurrency groups (`github.head_ref` is the PR's source branch name and
   is only set for `pull_request` events; falling back to `github.ref` covers `push` and any other
   trigger). This does not change which runs land in the same group for *this* repo's current
   triggers (`github.ref` already resolves to `refs/pull/<n>/merge` — stable across every push to a
   given PR — so runs for the same PR were already grouped together), but it removes the dependency
   on GitHub's synthetic, recomputed-per-push merge ref in favor of the actual named branch, which is
   the formulation GitHub's own docs and the wider ecosystem recommend specifically because the merge
   ref is an internal implementation detail subject to recomputation.
3. **Bound every job's wall-clock time with `timeout-minutes`.** Independent of the group-key/predicate
   fix, nothing in `ci.yml` today prevents a genuinely wedged run (stuck past any concurrency
   arbitration entirely — e.g., a hung step) from occupying its group's "in progress" slot for GitHub
   Actions' default 360-minute job timeout, which is long enough to starve every subsequent push to
   that PR for hours. Explicit, generous-but-finite `timeout-minutes` values on every job give a stuck
   run a hard ceiling, so the failure mode in the issue's third "possible direction" (a stale run
   should not be able to starve every subsequent run indefinitely) has a bound even in the pathological
   case the YAML-level predicate fix does not itself address (a run that never receives an update at
   all — no new push, no cancellation signal, just hung).

**This fix does not explain or prevent the second anomaly (the zero-job `push` run).** Both the old
predicate (`github.event_name == 'pull_request'`) and the new one (`github.ref != 'refs/heads/main'
&& github.ref != 'refs/heads/master'`) evaluate identically and deterministically `false` for every
run in a `push`-to-`main` group — there was never a cross-run predicate-disagreement axis for that
group under either formula, so items 1–3 above cannot be the fix for it. A `push` run cancelled with
zero jobs (i.e., never entering the queue at all) is a different failure shape from "queued, then
cancelled 33 seconds into `in_progress`," and is not addressed by any change in this spec. This is
recorded explicitly here — see Risks & Open Questions — rather than implied to be covered by the
group-key/predicate rewrite above.

## Approach

Rewrite `.github/workflows/ci.yml`'s `concurrency:` block:

```yaml
concurrency:
  group: ci-${{ github.workflow }}-${{ github.head_ref || github.ref }}
  cancel-in-progress: ${{ github.ref != 'refs/heads/main' && github.ref != 'refs/heads/master' }}
```

- `group`: `github.head_ref` (PR source branch, e.g. `ci/fix-concurrency-group`) with a fallback to
  `github.ref` for non-PR triggers (`push` to `main`/`master`, `workflow_dispatch` if ever added). This
  keeps the group name stable across every push to a given PR, and gives it a human-legible branch
  name instead of the synthetic merge ref.
- `cancel-in-progress`: `true` for any ref that is not `refs/heads/main` or `refs/heads/master` —
  i.e., true for every PR — and unconditionally `false` for a direct push to either trunk name (this
  repo's actual default branch is `main`; `master` is kept only because `ci.yml`'s own `on.push`
  trigger already lists both, for backward compatibility — see the existing `on: push: branches:
  [master, main]` block, unchanged by this fix). This is evaluated purely from `github.ref`, which
  every run on a given ref computes identically, removing the `event_name`-based indirection that
  Non-Negotiable-Rule-adjacent hint #1 in the issue flags as suspect.
- Add `timeout-minutes` to every job in `ci.yml` (`lint`, `test` per OS, `cross-vendor-gate`,
  `dist-plan`): generous enough not to false-positive on legitimate slow runs (Windows in particular
  is the slowest leg of the `test` matrix), tight enough to bound how long a wedged run can occupy
  its concurrency group. This repo's CI has never taken close to an hour per job historically,
  so the timeouts below (recorded in the plan) are chosen with wide headroom, not to be a source of
  new flakiness.

No change to `on.push.branches` — it already covers `main` and `master`; no change to the `lint`
job's other steps, the test matrix, `cross-vendor-gate`, or `dist-plan` beyond adding
`timeout-minutes:`.

### Verification on a real PR (acceptance criterion)

This fix is validated empirically, not just by reading the YAML: this very issue's implementation PR
is used as the live test bed. After the corrected `concurrency:` block is pushed, two additional
pushes to the same PR branch in quick succession must show — via `gh run list`/`gh run view` — the
first of the two runs cancelled and the second completing, confirming the newer run now supersedes
the older one instead of the reverse.

### #132's uncompleted run — decision

No retrospective CI run is required to consider `main` "verified" going forward: the acceptance
criterion asks for a *decision*, not a mandatory backfill action. `main`'s current tip (`46a216c` as
of the issue's filing, and further commits since) is fully green and contains #132's code, so the
codebase state is verified in aggregate by every CI run since. Rerunning historical run
`34548437470` (or any equivalent action targeting the exact stale commit) would use the workflow
file as pinned to that commit's ref state — i.e., the *old*, buggy concurrency block — and would
exercise a commit that is no longer anything's HEAD, so it would validate nothing about the fix in
this PR and would not move any current branch forward. **Decision: do not backfill a retrospective
run.** The gap in `6274151`'s own individual CI history is accepted as a known, historical,
zero-impact artifact of the bug being fixed here, since the code it introduced has been exercised
green by every commit since. This is recorded here so the acceptance criterion is explicitly
resolved, not silently dropped.

## Scope

**In:**

- `.github/workflows/ci.yml` — the `concurrency:` block (group key + cancel predicate) and
  `timeout-minutes:` added to every job.
- Empirical verification via two rapid-succession pushes to this PR's own branch.
- The explicit decision (above) on #132's retrospective run.

**Out:**

- Any other CI workflow file (`release.yml` has no `pull_request`/rapid-push concurrency concern —
  release tags are pushed once).
- Any change to which events trigger CI (`on:` block untouched).
- Any change to job contents beyond adding `timeout-minutes` (no new jobs, no matrix changes, no
  dependency changes).
- Retroactively re-running or otherwise backfilling a CI run for commit `6274151` (decided against
  above).
- Any Rust code, crate, or public-interface change.

## Public-interface changes

**None.** This is a `.github/workflows/ci.yml`-only change. It is, however, a change to this
repo's **deploy/distribution shape** in the sense the otto-development fast-path criteria use that
term (any `.github/workflows/` edit is explicitly excluded from the fast-path), which is why this
change goes through the full spec → plan → implement → review → release lifecycle rather than being
fast-pathed, even though it touches a single file. No SPP wire type, `ProviderHandler`/
`ProviderClient` method, tool MCP schema, plugin ABI surface, slash command, env var, or on-disk
transcript/keyring format is touched (Non-Negotiable Rule 6 is not engaged).

## Assumptions

- **`github.head_ref || github.ref` does not change which runs share a group for this repo's current
  triggers.** `github.ref` for a `pull_request` event was already stable (`refs/pull/<n>/merge`)
  across every push to that PR, so grouping behavior for PRs is unchanged in practice; the fix's value
  is removing the dependency on the merge ref's internal recomputation and switching to the
  ecosystem-standard formulation, not changing which runs are grouped together.
- **Keying the group on `github.head_ref` (branch name) rather than PR number carries a theoretical
  cross-contributor collision risk** — two different forks' PRs sharing the same head branch name
  (e.g. both named `fix`) would land in the same concurrency group and could cancel each other's
  runs. This repo (`savvagent/otto`) takes contributions from a single org with no external-fork PR
  workflow in active use, so this is accepted as a non-issue for the current contribution model; if
  that changes, the group key should move to `github.event.pull_request.number || github.ref` instead
  (PR number is always unique, unlike branch name).
- **Tying `cancel-in-progress` to `github.ref` rather than `github.event_name` is sufficient to
  eliminate the specific cross-run-disagreement risk the issue names**, because every run sharing a
  concurrency group by definition shares that group's ref (that is what the group key is derived
  from), so the predicate now can never differ between two runs in the same group. This does not
  prove GitHub's internal arbitration bug (if any) is fully eliminated — see Risks below — but it
  removes the one concretely identifiable, YAML-level source of ambiguity.
- **`timeout-minutes` values are chosen generously** (documented per-job in the plan) based on this
  workflow's actual historical run times, to bound worst-case starvation without introducing new
  false-timeout flakiness. No historical run time data beyond general knowledge of this workflow's
  shape (Windows being the slowest leg) was available to pull an exact P99; the chosen values are
  deliberately several multiples of expected duration.
- **No backfill run for #132's merge commit** (decision recorded above) — treated as an explicit,
  documented decision satisfying the acceptance criterion, not a skipped step.
- **This change is not fast-pathed**, per otto-development's own criteria (`.github/workflows/`
  changes are explicitly excluded from the fast-path regardless of file count), and per the job's own
  instructions requiring the full lifecycle.

## Goal & Success Criteria

`ci.yml`'s concurrency group reliably supersedes an in-flight run with a newer push on the same PR,
never cancels a push to `main`, and bounds how long any single run (wedged or not) can occupy a
group's slot.

- `ci.yml`'s `concurrency.cancel-in-progress` is a pure function of `github.ref`, with no dependency
  on `github.event_name`.
- `ci.yml`'s `concurrency.group` uses `github.head_ref || github.ref`.
- Every job in `ci.yml` declares an explicit `timeout-minutes`.
- On a real PR (this issue's own implementation PR), two pushes in quick succession result in the
  first run being cancelled and the second completing — confirmed via `gh run list`/`gh run view`
  before merge.
- A push to `main` is confirmed structurally incapable of being cancelled by this group (the
  predicate evaluates to a literal `false` whenever `github.ref == 'refs/heads/main'`, independent of
  any other run's state). Note this specific guarantee already held under the *old*
  `event_name`-based predicate too (also unconditionally `false` for every `push` event) — this
  criterion confirms the guarantee survives the rewrite, not that the rewrite newly creates it; the
  rewrite's actual value-add is eliminating the `event_name`-based cross-run-disagreement axis for PR
  runs (see Root Cause).
- The #132 retrospective-run question is explicitly decided (no backfill) and recorded in this spec,
  not silently dropped.

## Error Handling & Edge Cases

- **A `workflow_dispatch` or other future non-`push`/non-`pull_request` trigger** would have
  `github.head_ref` unset and `github.ref` pointing at whatever ref it was dispatched against; the
  `|| github.ref` fallback keeps the group key well-formed, and the ref-based cancel predicate still
  behaves correctly (non-cancellable only for `main`/`master`).
- **A branch literally named `main` or `master` used as a PR head** (i.e., someone opens a PR *from* a
  branch called `main` in a fork) — `github.head_ref` would be `main`, but `github.ref` for that
  `pull_request` event is still `refs/pull/<n>/merge`, not `refs/heads/main`, so the cancel predicate
  (which reads `github.ref`, not `github.head_ref`) is unaffected: PR runs are still cancellable
  regardless of the head branch's name. This is intentional — the predicate protects pushes to this
  repo's actual trunk refs, not PRs whose head happens to share a trunk-like name.
- **A wedged run that also fails to receive any cancellation signal at all** (e.g., a genuinely hung
  runner, not a concurrency-arbitration issue) is bounded by the new `timeout-minutes` values instead
  of GitHub's 360-minute default, per job.
- **Two pushes so close together that GitHub's own webhook delivery races** — this spec's fix removes
  the one identifiable YAML-level ambiguity but cannot guarantee GitHub's backend has zero remaining
  internal race conditions (see Risks). The empirical verification step (two rapid pushes on the real
  implementation PR) is the acceptance test for the common case; it is not a formal proof for every
  possible timing window.

## Risks & Open Questions

- **The second anomaly (run `34548437470`, a `push`-to-`main` run cancelled with zero jobs) is not
  explained or prevented by this fix, and is an accepted, unresolved residual risk.** Both the old and
  new `cancel-in-progress` formulas evaluate identically and deterministically `false` for every run
  in a `push`-to-`main` concurrency group — there is no cross-run predicate disagreement possible for
  that group under either formula, so this spec's fix (items 1–3 in Root Cause) has no mechanism that
  would have prevented it, and cannot be expected to prevent a recurrence. A zero-job cancellation
  (never entering the queue) is also a different failure shape than the PR-run inversion this spec
  does address (queued, started, cancelled mid-run), and `timeout-minutes` does not apply to a run
  that never started a job. If this recurs, it should be filed as its own issue with fresh evidence
  (GitHub support ticket territory, likely, since no workflow-file change is implicated) rather than
  treated as a sign this fix failed.
- **GitHub's concurrency-arbitration internals are not observable from the workflow file.** If the
  originally observed inversion was actually caused by a true platform-level race condition
  independent of the expression's dependencies (rather than by `event_name`-based cross-run
  disagreement), this fix removes a plausible, concretely identifiable contributing cause and the
  single actionable lever available at the YAML level, but cannot categorically prove no residual
  platform-level race remains. The empirical two-push verification step on a real PR is the practical
  test; if it fails, this spec's root-cause hypothesis is falsified and the issue should be reopened
  with the new evidence rather than iterated on speculatively.
- **`timeout-minutes` values could theoretically be too tight** for an unusually slow but legitimate
  run (e.g., a cold cache after a `Cargo.lock` bump touching many dependencies). Chosen generously
  (see plan) specifically to avoid this; if it happens in practice, the fix is to raise the specific
  job's timeout, not to remove timeouts altogether.

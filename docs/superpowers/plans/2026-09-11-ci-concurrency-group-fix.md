# ci-concurrency-group-fix Implementation Plan

**Goal:** Correct `.github/workflows/ci.yml`'s `concurrency` block so a new push to a PR supersedes
an in-flight run rather than the reverse (the inversion observed twice on PR #135), guarantee a push
to `main`/`master` can never be cancelled by this group, bound how long any single run can occupy the
group's slot with explicit `timeout-minutes`, and empirically confirm the fix on this very PR by
pushing twice in quick succession, per `savvagent/otto#139`.

**Architecture:** A single-file YAML change to `.github/workflows/ci.yml`: rewrite the workflow-level
`concurrency:` block (group key + cancel predicate) and add `timeout-minutes:` to each of the four
existing jobs (`lint`, `test` matrix, `cross-vendor-gate`, `dist-plan`). No Rust code, no crate, no
job added/removed/restructured beyond the new key. This is a CI-infrastructure change with no
runtime application behavior — the everything-is-MCP-shaped architecture, the host-swap `RwLock`
rule, the provider transport split, and the `ProgressDispatcher` forwarder-abort pattern are all not
engaged (nothing under `crates/` changes).

**Tech Stack:** GitHub Actions workflow YAML. Verification is `gh` CLI observation of real workflow
runs (`gh run list`, `gh run view`) on this PR's own branch — there is no local test harness for
GitHub's concurrency-group semantics.

**Spec:** `docs/superpowers/specs/2026-09-11-ci-concurrency-group-fix-design.md` — read it first.
This plan implements it exactly.

**Release line:** next PATCH after whatever `workspace.package.version` reads at cut time (currently
`0.30.2` as of branch creation — re-read at cut time per the release step below, since `origin/main`
moves while this branch is open). CI-only fix, no runtime behavior change, no public-interface
change → PATCH per `CHANGELOG.md`'s convention.

**Branch:** `ci/fix-concurrency-group`

## File Map

**New files**
- None.

**Modified files**
- `.github/workflows/ci.yml` — `concurrency:` block rewritten (group key + cancel predicate);
  `timeout-minutes:` added to all four jobs.
- `CHANGELOG.md` — a `Fixed` entry under `## [Unreleased]` (added in the dedicated release PR per
  Phase 4 step 12, not in this PR — see Task 2).

## Task 1: Fix the concurrency block and bound job wall-clock time

**Files:**
- Modify: `.github/workflows/ci.yml`

No Rust, so no `cargo test` step in this task. Verification is `gh` CLI observation of real runs on
this PR's own branch (Task 1 also covers the plan's empirical-verification acceptance criterion,
since it cannot be satisfied until the fix is pushed and this PR exists).

- [ ] **Step 1: Confirm the current (buggy) block, for the record.** From the worktree root:
  ```bash
  grep -n -A2 "^concurrency:" .github/workflows/ci.yml
  ```
  Expect:
  ```yaml
  concurrency:
    group: ci-${{ github.workflow }}-${{ github.ref }}
    cancel-in-progress: ${{ github.event_name == 'pull_request' }}
  ```
  This is the baseline this task replaces.

- [ ] **Step 2: Rewrite the `concurrency:` block.** In `.github/workflows/ci.yml`, replace:
  ```yaml
  concurrency:
    group: ci-${{ github.workflow }}-${{ github.ref }}
    cancel-in-progress: ${{ github.event_name == 'pull_request' }}
  ```
  with:
  ```yaml
  concurrency:
    group: ci-${{ github.workflow }}-${{ github.head_ref || github.ref }}
    cancel-in-progress: ${{ github.ref != 'refs/heads/main' && github.ref != 'refs/heads/master' }}
  ```
  Update the comment immediately above the block (currently "Cancel in-flight runs on the same ref
  when a new push comes in. Saves CI minutes on PRs that get force-pushed.") to also note *why* the
  predicate is ref-based rather than event-based, e.g.:
  ```yaml
  # Cancel in-flight runs on the same ref when a new push comes in. Saves CI minutes on PRs that get
  # force-pushed. cancel-in-progress is a pure function of github.ref (not github.event_name) so
  # every run sharing a group computes the identical value — a push to main/master is always false,
  # every PR ref is always true, with no room for two runs in the same group to disagree. See
  # docs/superpowers/specs/2026-09-11-ci-concurrency-group-fix-design.md for the incident this fixes.
  ```

- [ ] **Step 3: Verify Step 2.**
  ```bash
  grep -n -A2 "^concurrency:" .github/workflows/ci.yml
  ```
  Expect the new `group`/`cancel-in-progress` lines exactly as written above.
  ```bash
  python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml'))" && echo "YAML parses OK"
  ```
  (Or `ruby -ryaml -e "YAML.load_file('.github/workflows/ci.yml')"` if `python3`/`pyyaml` is
  unavailable — either confirms the file is still well-formed YAML after the edit; GitHub Actions'
  own `${{ }}` expressions are opaque strings to a generic YAML parser, so this only checks structural
  validity, not expression semantics.)

- [ ] **Step 4: Add `timeout-minutes` to the `lint` job.** In the `lint:` job block (fmt + clippy +
  skill-port parity check), add `timeout-minutes: 20` immediately after `runs-on: ubuntu-latest`.
  This job has no matrix and historically finishes in a few minutes; 20 is several multiples of
  observed duration.

- [ ] **Step 5: Add `timeout-minutes` to the `test` job.** In the `test:` job block (matrix:
  ubuntu-latest/macos-latest/windows-latest), add `timeout-minutes: 45` immediately after the
  `strategy:` block's closing (i.e., as a sibling of `runs-on:` and `strategy:` at the job level, so
  it applies per-matrix-leg, not once for the whole matrix). This is the slowest job (full workspace
  build + test across three OSes) — Windows is called out in the file's own existing comment as the
  slowest leg — so 45 minutes is generous headroom over any historical run.

- [ ] **Step 6: Add `timeout-minutes` to the `cross-vendor-gate` job.** Add `timeout-minutes: 20`
  immediately after `runs-on: ubuntu-latest` in that job block. It builds once and runs a single
  crate's test binary (`cargo test -p otto-host --test cross_vendor_history`), well under this
  ceiling historically.

- [ ] **Step 7: Add `timeout-minutes` to the `dist-plan` job.** Add `timeout-minutes: 15` immediately
  after `runs-on: ubuntu-latest` in that job block. It only downloads `dist` and runs `dist plan`
  (no `cargo build`), the fastest job in the file.

- [ ] **Step 8: Verify Steps 4-7.**
  ```bash
  grep -n "timeout-minutes" .github/workflows/ci.yml
  ```
  Expect exactly 4 matches (one per job: `lint`, `test`, `cross-vendor-gate`, `dist-plan`), with
  values `20`, `45`, `20`, `15` respectively.
  ```bash
  python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml'))" && echo "YAML parses OK"
  ```

- [ ] **Step 9: Public-interface note.** No SPP wire type, `ProviderHandler`/`ProviderClient` method,
  tool MCP schema, plugin ABI surface, slash command, env var, or on-disk transcript/keyring format
  touched — Non-Negotiable Rule 6 is not engaged. This is a deploy/distribution-shape change
  (`.github/workflows/`), which is why the plan runs the full lifecycle rather than fast-pathing
  despite touching one file — no additional CHANGELOG "public interface" callout is owed beyond the
  ordinary `Fixed` entry in the release PR (Task 2).

- [ ] **Step 10: Host-swap / streaming invariants — vacuously satisfied.** No `crates/otto/src/app.rs`
  or `crates/otto/src/tui.rs` touched, so the host-swap `RwLock` rule is not engaged. No streaming
  provider path touched, so the `ProgressDispatcher` forwarder-abort pattern is not engaged. No
  `Host` provider-pool code touched.

- [ ] **Step 11: Format and commit.**
  ```bash
  cargo fmt --all
  git add .github/workflows/ci.yml
  git commit -m "ci: fix concurrency group so a new push supersedes the in-flight run"
  ```
  (`cargo fmt --all` is a no-op here since no Rust changed — run anyway for house-style consistency
  with every other task in this repo's plans.)

## Task 2: Push, open the PR, and empirically verify the fix with two rapid pushes

**Files:** none further modified in this PR beyond Task 1's commit — this task is the empirical
verification acceptance criterion (`savvagent/otto#139`'s "Confirmed on a real PR: push twice in
quick succession...").

This task's steps are procedural (git/gh commands + `gh run` observation), not code changes. It
happens after Phase 4 step 7 (PR opened) in the parent skill's flow, but is recorded here as its own
task because it is a plan-mandated verification step specific to this fix, not the skill's generic
review loop.

- [ ] **Step 1: Push the branch and open the PR** (mechanics are the parent `otto-development`
  skill's Phase 4 step 7 — not duplicated here beyond noting this PR's body must reference
  `savvagent/otto#139` and link
  `docs/superpowers/specs/2026-09-11-ci-concurrency-group-fix-design.md` and this plan file).

- [ ] **Step 2: Record the PR's first CI run.**
  ```bash
  gh run list --repo savvagent/otto --branch ci/fix-concurrency-group --limit 5 --json databaseId,status,createdAt,headSha
  ```
  Note the run id for the PR's initial push (`RUN_A`).

- [ ] **Step 3: Make a second, trivial push while `RUN_A` is still in flight** (e.g., a
  no-op/whitespace commit, or simply timing Step 1's own follow-up commit if one is still pending —
  whatever is natural given the PR's actual commit sequence; the goal is two pushes to this branch
  close enough together that the first run is still `in_progress`/`queued` when the second run is
  created). Confirm timing:
  ```bash
  gh run list --repo savvagent/otto --branch ci/fix-concurrency-group --limit 5 --json databaseId,status,createdAt,headSha
  ```
  Note the new run id (`RUN_B`) and confirm `RUN_A` was still not `completed` at the moment `RUN_B`
  was created (i.e., this is a genuine overlap, not a sequential push-after-completion).

- [ ] **Step 4: Poll until both runs settle, then verify the fix's actual claim.**
  ```bash
  until gh run view $RUN_B --repo savvagent/otto --json status --jq .status | grep -qx completed; do sleep 15; done
  gh run view $RUN_A --repo savvagent/otto --json status,conclusion --jq '. '
  gh run view $RUN_B --repo savvagent/otto --json status,conclusion --jq '. '
  ```
  Expect: `RUN_A` (the older run) shows `status: completed`, `conclusion: cancelled`; `RUN_B` (the
  newer run) shows `status: completed`, `conclusion: success` (or `failure` on its own legitimate
  merits — but NOT `cancelled`, and NOT jobs that died at the `Swatinem/rust-cache@v2` step per the
  original incident's signature). If the *reverse* is observed (newer cancelled, older survives —
  the original bug's exact signature), the fix has not worked and this task is not complete; treat
  as a Stop & Escalate condition (the spec's root-cause hypothesis would be falsified — see the
  spec's Risks & Open Questions) rather than silently re-pushing and hoping.

- [ ] **Step 5: Confirm no push to `main` is cancellable, structurally.** This cannot be tested by
  pushing to `main` directly (Non-Negotiable Rule 2 — no direct pushes to `main` outside a merge).
  Instead, confirm by inspection that the predicate is a literal, unconditional `false` whenever
  `github.ref == 'refs/heads/main'`, independent of any other run's state or `github.event_name`:
  ```bash
  grep -n "cancel-in-progress" .github/workflows/ci.yml
  ```
  Confirm the printed expression reads `${{ github.ref != 'refs/heads/main' && github.ref !=
  'refs/heads/master' }}` — i.e., for `github.ref == 'refs/heads/main'` the first comparison is
  `false`, and `false && anything` is `false`, so `cancel-in-progress` is `false` unconditionally.
  This is a structural/logical confirmation (the expression's truth table), not a live-run
  observation — record it as such rather than claiming an actual `main` push was tested.

- [ ] **Step 6: Record the verification outcome in the PR description or a PR comment** (run ids,
  conclusions, timestamps) so the acceptance criterion's evidence is durable and reviewable, not just
  asserted in this plan's checkbox.

## Task 3: Cut the release (notes only — performed per Phase 4 step 12, not in this PR)

**Files:** none in this PR.

- [ ] **Step 1:** This PR does **not** bump `workspace.package.version` and does **not** add a
  `CHANGELOG.md` section — that happens in the dedicated release PR after this merges, per
  Non-Negotiable Rule 8 / Phase 4 step 12. Re-read `workspace.package.version` at cut time (it may
  have moved past `0.30.2` if another PR merges first) and cut the next PATCH from whatever it then
  reads. The `CHANGELOG.md` entry: a `Fixed` bullet describing that `ci.yml`'s concurrency group no
  longer cancels the wrong run on rapid PR pushes, that a push to `main`/`master` is structurally
  guaranteed non-cancellable by the group, and that every job now has an explicit `timeout-minutes`
  ceiling.

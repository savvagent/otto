# rule-8-batching-reconcile Implementation Plan

**Goal:** Reword `otto-development`'s Non-Negotiable Rule 8 (and its four restatements — "The Iron
Law" summary, Phase 4 step 12's intro, Phase 2 step 5's `Release line:` guidance, and the
rationalization table) so the mandatory-release requirement keeps its force while explicitly
permitting batching of already-merged, unreleased work at cut time — matching actual,
owner-approved practice (v0.29.0 batched three PRs) instead of contradicting it. State that a
batched release's line is the highest SemVer bump across the batch, require the release PR body to
enumerate every issue/PR the release covers, and reframe the plan format's `Release line:` field
as a floor rather than a predicted version. Apply the edit to the canonical
`.github/skills/otto-development/SKILL.md` first, mirror verbatim into the Claude Code port
`.claude/skills/otto-development/SKILL.md`, and regenerate the port-parity diff record.

**Architecture:** One set of five textual edits (all in the same file, `SKILL.md`) applied twice —
once to the canonical, once (verbatim, by anchor-text match, not line-number arithmetic — the two
files are offset +5 lines at the first two edit sites and +39 lines at the latter two) to the port
— followed by regenerating `.github/skills/otto-development/claude-port/SKILL.md.diff` via the
existing check script's `--update` mode. No Rust, no crate, no public interface.

**Tech Stack:** Markdown (skill docs). `bash .github/scripts/check-claude-skill-ports.sh` (existing,
unmodified) regenerates and verifies the port-parity record.

**Spec:** `docs/superpowers/specs/2026-09-11-rule-8-batching-reconcile-design.md` — read it first.
This plan implements it exactly.

**Release line:** at least PATCH (docs/workflow-only, no runtime behavior change, no
public-interface change, per `CHANGELOG.md`'s convention) — MINOR if a feature-level change lands
in the same batch by cut time. Re-read `workspace.package.version` at cut time (currently `0.30.3`
as of branch creation; `origin/main` moves while this branch is open) and check for any
already-merged, unreleased work to fold in, per this very PR's own reworded Rule 8.

**Branch:** `docs/reconcile-rule-8-batching`

## File Map

**New files**
- None.

**Modified files**
- `.github/skills/otto-development/SKILL.md` — five edits: Iron Law paragraph, Non-Negotiable
  Rule 8, Phase 2 step 5's `Release line:` bullet, Phase 4 step 12 (intro + three sub-steps), and
  the rationalization-table row (canonical).
- `.claude/skills/otto-development/SKILL.md` — the same five edits, mirrored verbatim (port).
- `.github/skills/otto-development/claude-port/SKILL.md.diff` — regenerated.
- `CHANGELOG.md` — a `Changed` entry under `## [Unreleased]` (added in the dedicated release PR
  per Phase 4 step 12, not in this PR — see Task 2).

## Task 1: Reword Rule 8 and its four restatements, canonical then port

**Files:**
- Modify: `.github/skills/otto-development/SKILL.md`
- Modify: `.claude/skills/otto-development/SKILL.md`
- Regenerate: `.github/skills/otto-development/claude-port/SKILL.md.diff`

No Rust, so no `cargo test` step in this task. Verification is
`bash .github/scripts/check-claude-skill-ports.sh`, run before and after, plus targeted `grep`
checks confirming old text is gone and new text is present.

- [ ] **Step 1: Confirm the current state.** From the worktree root:
  ```bash
  bash .github/scripts/check-claude-skill-ports.sh
  ```
  Expect exit 0 (baseline — the tree is in sync before this task's edits).
  ```bash
  grep -n "batch" .github/skills/otto-development/SKILL.md
  grep -n "batch" .claude/skills/otto-development/SKILL.md
  ```
  Expect 4 hits in each file (canonical: 57, 119, 759, 1023; port: 62, 124, 798, 1062 — exact line
  numbers may drift slightly if the working tree has moved since this plan was written; match by
  content, not line number).

- [ ] **Step 2: Reword the Iron Law's release-mandatory paragraph — canonical.** In
  `.github/skills/otto-development/SKILL.md`, find:
  ```
  **A merge to the trunk branch (`main` — CI also triggers on `master` for compatibility, but `main` is
  this repo's actual default branch) is not done until a release is cut.** Every PR that lands on trunk
  triggers a version bump, tag, and published GitHub Release with build artifacts (Phase 4 step 12).
  There is no "batch several PRs into one release later" carve-out in this workflow — see
  Non-Negotiable Rule 8.
  ```
  Replace with:
  ```
  **A merge to the trunk branch (`main` — CI also triggers on `master` for compatibility, but `main` is
  this repo's actual default branch) is not shipped until a release covers it.** Every PR that lands on
  trunk is covered by a version bump, tag, and published GitHub Release with build artifacts (Phase 4
  step 12) — cut promptly, and, when other merges are already sitting unreleased on `main` at cut time,
  batched into that same release rather than left to rot waiting for a narrower one. See Non-Negotiable
  Rule 8.
  ```

- [ ] **Step 3: Reword Non-Negotiable Rule 8 — canonical.** In the same file, find:
  ```
  8. **Every merge to the trunk branch (`main`, or `master` if this repo ever reverts to that name —
     see the Trunk row above) cuts a release.** No feature/fix PR merges and is considered shipped
     without a version bump, a `CHANGELOG.md` entry, a pushed `vX.Y.Z` tag, and a published GitHub
     Release with build artifacts, following `RELEASING.md`'s manual process. This is not optional and
     not batchable across PRs — but "cut the release" means opening the dedicated release PR (Phase 4
     step 12) and completing the tag/release for it immediately after the feature/fix PR merges, not
     necessarily folding the version bump into the same commit or PR.
  ```
  Replace with:
  ```
  8. **Every merge to the trunk branch (`main`, or `master` if this repo ever reverts to that name —
     see the Trunk row above) is covered by a release before it counts as shipped.** No feature/fix PR
     merges and is considered shipped without a version bump, a `CHANGELOG.md` entry, a pushed
     `vX.Y.Z` tag, and a published GitHub Release with build artifacts, following `RELEASING.md`'s
     manual process. This is not optional, and never skipped for size — but it is **not
     one-release-per-PR** either: `workspace.package.version` and `CHANGELOG.md` are global, so
     whoever cuts a release necessarily sweeps up every merge already sitting unreleased on `main`,
     regardless of which job or PR put it there. **Batching already-merged, unreleased work into one
     release is therefore permitted — expected, even** — a release cut while other merges sit
     unreleased on `main` MUST fold them in rather than ship a narrower release and leave them behind;
     what is never permitted is a merge that stays unreleased indefinitely because every job assumed
     another job would cut it. Two consequences follow: **a batched release's line is the highest
     SemVer bump required by any PR in the batch**, not the one any single plan predicted — a
     PATCH-planned change ships under MINOR if a feature merged alongside it in the same batch — so
     re-read `workspace.package.version` and what is actually unreleased on `main` at cut time rather
     than trusting a plan's `Release line:` field (Phase 2 step 5) as a number; and **the release PR
     body must enumerate every issue/PR the release covers**, not only the one that triggered the cut
     (Phase 4 step 12). "Cut the release" means opening the dedicated release PR (Phase 4 step 12) and
     completing the tag/release for it immediately after the feature/fix PR merges, not necessarily
     folding the version bump into the same commit or PR — and not delaying that cut on the theory
     that a future PR will batch with it; batching covers what is *already* merged and unreleased at
     cut time, never a deliberate wait for what merges next.
  ```

- [ ] **Step 4: Reframe the `Release line:` plan-format bullet as a floor — canonical.** In the same
  file, Phase 2 step 5, find:
  ```
  - **Release line:** the next `vX.Y.Z` this work ships as (per the SemVer convention in
    `CHANGELOG.md`). Not every pre-existing plan has this line — this skill adds it so the mandatory
    release-cut (Non-Negotiable Rule 8) is traceable back to the plan that necessitated it.
  ```
  Replace with:
  ```
  - **Release line:** a **floor**, not a predicted version — the minimum SemVer bump this work alone
    requires (e.g. "at least PATCH" or "at least MINOR", per the convention in `CHANGELOG.md`), not a
    guessed `vX.Y.Z`. Non-Negotiable Rule 8 permits batching this work's release with whatever else is
    already merged and unreleased on `main` at cut time, and a batched release's actual line is the
    **highest** bump required across the whole batch — so a PATCH floor can still ship under MINOR if a
    feature lands in the same batch. Re-read `workspace.package.version` and what's unreleased on
    `main` at cut time (Phase 4 step 12) rather than trusting this field as the number it will ship
    under. Not every pre-existing plan has this line — this skill adds it so the mandatory release-cut
    (Non-Negotiable Rule 8) is traceable back to the plan that necessitated it.
  ```

- [ ] **Step 5: Add batching mechanics to Phase 4 step 12 — canonical.** In the same file, find the
  step 12 intro:
  ```
  ### Step 12: Cut the release (mandatory — Non-Negotiable Rule 8)

  **Every merge to the trunk branch (`main`) cuts a release.** This is not optional, not batchable, and
  not skippable for "small" PRs. Follow `RELEASING.md`'s manual process (release-plz automation is
  currently broken upstream — see that file), from a fresh worktree off the just-updated `main`,
  itself landing via its own PR:
  ```
  Replace with:
  ```
  ### Step 12: Cut the release (mandatory — Non-Negotiable Rule 8)

  **Every merge to the trunk branch (`main`) is covered by a release.** This is not optional and never
  skippable for "small" PRs. It is **not necessarily one-release-per-PR**: `workspace.package.version`
  and `CHANGELOG.md` are global, so cutting a release sweeps up every merge already sitting unreleased
  on `main`, not just the PR that triggered the cut. **Before assuming this release covers only your
  own PR, check `workspace.package.version`, `gh release list`, and recently-merged PRs against
  `main`** — if other unreleased merges are already there, fold them into this same release rather than
  cutting a narrower one and leaving them behind (Non-Negotiable Rule 8). Follow `RELEASING.md`'s
  manual process (release-plz automation is currently broken upstream — see that file), from a fresh
  worktree off the just-updated `main`, itself landing via its own PR:
  ```

- [ ] **Step 6: Update the three numbered sub-steps — canonical.** In the same file's Phase 4 step
  12 numbered list, find sub-step 1:
  ```
  1. **Bump the version.** In the root `Cargo.toml`, update `workspace.package.version` and every
     internal `workspace.dependencies` entry's `version` field to match. Run `cargo check --workspace`
     to regenerate `Cargo.lock`.
  ```
  Replace with:
  ```
  1. **Bump the version.** Pick the **highest** SemVer bump required by anything this release covers —
     your own PR plus any other already-merged, unreleased work being batched in (Non-Negotiable
     Rule 8) — not necessarily the floor a plan's `Release line:` field predicted. In the root
     `Cargo.toml`, update `workspace.package.version` and every internal `workspace.dependencies`
     entry's `version` field to match. Run `cargo check --workspace` to regenerate `Cargo.lock`.
  ```
  Find sub-step 2:
  ```
  2. **Update `CHANGELOG.md`.** Rename `## [Unreleased]` to `## X.Y.Z - YYYY-MM-DD` (today's date);
     add a fresh empty `## [Unreleased]` above it. Follow Keep a Changelog categories
     (Added/Changed/Fixed/Removed) and this repo's SemVer convention (pre-1.0: MINOR =
     features/breaking changes, PATCH = fixes).
  ```
  Replace with:
  ```
  2. **Update `CHANGELOG.md`.** Rename `## [Unreleased]` to `## X.Y.Z - YYYY-MM-DD` (today's date);
     add a fresh empty `## [Unreleased]` above it. Follow Keep a Changelog categories
     (Added/Changed/Fixed/Removed) and this repo's SemVer convention (pre-1.0: MINOR =
     features/breaking changes, PATCH = fixes). Include an entry for every PR this release batches in,
     not only the one that triggered the cut.
  ```
  Find sub-step 4:
  ```
  4. **Commit, open a PR, and merge to `main`** — same worktree + PR discipline as any other change,
     reviewed like any other PR (the mandatory trio still applies; a version-bump-only PR is a small,
     fast review, not a skipped one).
  ```
  Replace with:
  ```
  4. **Commit, open a PR, and merge to `main`** — same worktree + PR discipline as any other change,
     reviewed like any other PR (the mandatory trio still applies; a version-bump-only PR is a small,
     fast review, not a skipped one). **The release PR body must enumerate every issue/PR this release
     covers** — every `#N` this batch closes, not only the most recent one (v0.29.0's PR body initially
     named only two of the three issues it actually batched). List them explicitly, e.g. `Closes #A`,
     `Closes #B`, `Closes #C` or a bullet list under a "Batches" heading.
  ```

- [ ] **Step 7: Update the rationalization-table entry — canonical.** In the same file's "Common
  Rationalizations" table, find:
  ```
  | "This is a tiny PR, I'll batch the release with the next one"          | Rule 8 has no batching carve-out. Cut the release now, as part of closing out this PR.                                                              |
  ```
  Replace with:
  ```
  | "This is a tiny PR, I'll batch the release with the next one"          | Rule 8 permits batching only what is *already* merged and unreleased on `main` at cut time — never a deliberate wait for a future PR. Cut the release now, as part of closing out this PR. |
  ```

- [ ] **Step 8: Verify Steps 2-7 landed correctly in the canonical.**
  ```bash
  grep -n "not batchable\|no \"batch several\|no batching carve-out" .github/skills/otto-development/SKILL.md
  ```
  Must return nothing (all stale "not batchable"/"no carve-out" phrasing is gone).
  ```bash
  grep -n "highest SemVer bump\|highest.*bump required" .github/skills/otto-development/SKILL.md
  ```
  Expect at least 2 matches (Rule 8 and Phase 4 step 12 sub-step 1).
  ```bash
  grep -n "enumerate every issue" .github/skills/otto-development/SKILL.md
  ```
  Expect at least 2 matches (Rule 8 and Phase 4 step 12 sub-step 4).
  ```bash
  grep -n "a \*\*floor\*\*, not a predicted version" .github/skills/otto-development/SKILL.md
  ```
  Expect 1 match (Phase 2 step 5).

- [ ] **Step 9: Repeat Steps 2-7 verbatim in the port.** In
  `.claude/skills/otto-development/SKILL.md`, apply the identical seven edits (same old-text /
  new-text pairs from Steps 2-7) to the port's copies of the same seven passages. The port's line
  numbers differ (offset +5 for the Iron Law/Rule 8 edits, +39 for the Phase 4 step 12/
  rationalization-table edits, per the spec's Approach section) — locate each by its old-text
  content, not by line number. No host-mechanism adaptation is needed at any of the five edit
  sites (none names `agent_type`, `mode: "sync"`, `read_agent`, `ask_user`, or a `todos` table) —
  the text is copied across exactly as written in Steps 2-7.

- [ ] **Step 10: Verify Step 9 landed correctly in the port.** Re-run the same four `grep` checks
  from Step 8 against `.claude/skills/otto-development/SKILL.md` instead of the canonical path.
  Same expected results.

- [ ] **Step 11: Confirm no other passage in either file still contains stale wording.**
  ```bash
  grep -n "batch" .github/skills/otto-development/SKILL.md
  grep -n "batch" .claude/skills/otto-development/SKILL.md
  ```
  Every hit in both files must now read as one of the reworded passages (spot-check by eye) — none
  should still say "not batchable" or "no ... carve-out."

- [ ] **Step 12: Regenerate the port-parity record.**
  ```bash
  bash .github/scripts/check-claude-skill-ports.sh --update
  ```
  Confirm it reports rewriting exactly
  `.github/skills/otto-development/claude-port/SKILL.md.diff` (the only port file changed; no
  other skill's record should be touched — `agent-prompts.md.diff` must be unaffected).

- [ ] **Step 13: Verify the check passes.**
  ```bash
  bash .github/scripts/check-claude-skill-ports.sh
  ```
  Must exit 0.

- [ ] **Step 14: Eyeball the regenerated record.**
  ```bash
  git diff --stat .github/skills/otto-development/claude-port/
  grep -c "highest SemVer bump\|enumerate every issue\|a \*\*floor\*\*, not a predicted version" .github/skills/otto-development/claude-port/SKILL.md.diff
  ```
  Confirm the new hunks include the reworded Rule 8, the `Release line:` floor reframing, the
  Phase 4 step 12 batching mechanics, and the rationalization-table row (as `+`/`-` pairs), and
  that no unrelated hunk appears (only `SKILL.md.diff` should show a diff — `agent-prompts.md.diff`
  untouched).

- [ ] **Step 15: Public-interface note.** No SPP wire type, tool schema, plugin ABI, slash command,
  env var, or on-disk format touched — Non-Negotiable Rule 6 is not engaged. No `CHANGELOG.md`
  interface note owed by this task.

- [ ] **Step 16: Host-swap / streaming invariants — vacuously satisfied.** No
  `crates/otto/src/app.rs` or `tui.rs` touched, so the host-swap `RwLock` rule is not engaged. No
  streaming provider path touched, so the `ProgressDispatcher` forwarder-abort pattern is not
  engaged.

- [ ] **Step 17: Format and commit.** No Rust changed, so `cargo fmt --all` is a no-op here but run
  it anyway for consistency with house style:
  ```bash
  cargo fmt --all
  git add .github/skills/otto-development/SKILL.md \
          .claude/skills/otto-development/SKILL.md \
          .github/skills/otto-development/claude-port/SKILL.md.diff
  git commit -m "docs: permit batching already-merged work into one release (Rule 8)"
  ```

## Task 2: Cut the release (notes only — performed per Phase 4 step 12, not in this PR)

**Files:** none in this PR.

- [ ] **Step 1:** This PR does **not** bump `workspace.package.version` and does **not** add a
  `CHANGELOG.md` section — that happens in the dedicated release PR after this merges, per
  Non-Negotiable Rule 8 / Phase 4 step 12 (the very rule this PR is rewording — its mandatory-
  release-cut requirement still applies to this PR itself). Re-read `workspace.package.version` at
  cut time (it may have moved past `0.30.3` if another PR merges first — check `gh release list`
  and recently-merged PRs against `main` for anything else sitting unreleased, per the reworded
  Rule 8 this PR ships, and batch it in if so, enumerating every batched issue/PR in the release
  PR body) and cut the next PATCH (or MINOR, if something else in the batch requires it) from
  whatever it then reads. The `CHANGELOG.md` entry: a `Changed` bullet describing that Rule 8 now
  permits batching already-merged, unreleased work into one release, states the highest-bump-
  across-the-batch rule, and requires the release PR body to enumerate every issue in the batch;
  plus a bullet noting the `Release line:` plan-format field is now a floor, not a prediction.

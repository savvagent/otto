# Reconcile Rule 8's batching prohibition with actual release practice — design

Date: 2026-09-11
Status: IMPLEMENTED
Source: savvagent/otto#138
Related: v0.29.0 (#135, batches #118/#81/#128); #128/#131 (the plan whose predicted release line
changed at cut time). Distinct from #133 (untrusted-input hardening) and #137 (the ported
code-review templates' lost read-only guarantee) — same skill body, different defects, already
fixed separately.

## Problem

`otto-development`'s Non-Negotiable Rule 8 states, in both the canonical
`.github/skills/otto-development/SKILL.md` and its Claude Code port
`.claude/skills/otto-development/SKILL.md`: a release is "not optional and not batchable across
PRs." The claim is not confined to the rule's own paragraph — it is restated in "The Iron Law"
summary, in Phase 4 step 12's intro ("This is not optional, not batchable, and not skippable..."),
and in the rationalization table's answer to "I'll batch the release with the next one" — four
restatements per file, eight across canonical and port.

Actual practice already contradicts this, with the repository owner's standing approval:
same-session merges are batched into a single release. v0.29.0 (`#135`) batches three PRs
(`#118`, `#81`, `#128`) into one release. This is not a one-off: `workspace.package.version` and
`CHANGELOG.md` are both single, global files — whoever cuts a release necessarily sweeps up
*every* merge sitting unreleased on `main` at that moment, regardless of which job or PR put it
there. One-release-per-PR is only achievable if every job that merges a PR promptly cuts its own
release before another job's merge lands on `main` first. v0.29.0 is the counterexample: the jobs
behind `#118` and `#81` were each abandoned mid-workflow with expired otto-factory queue claims,
leaving their PRs merged and unreleased; a third, unrelated job's release then had to carry them,
because there was no other way for those merges to ever become "shipped."

So the rule as written is not merely unenforced — it is not always *enforceable* by the agent it
binds. An agent cutting a release cannot un-merge someone else's abandoned PR to keep its own
release "clean"; the only choices at cut time are to batch the other merges in, or to leave them
permanently unreleased, and the second option is worse by the rule's own logic (a merge is not
shipped until released).

The rule and the practice have diverged silently: nothing on either side records the divergence,
so a future reader (human or agent) hits a rule that contradicts what every recent release
actually did, with no explanation of why, and no guidance for what to do about the two known
knock-on problems:

1. **The release line is not always the one predicted at planning time.** Plans carry a
   `**Release line:**` field, set when the plan is written. `#128`'s plan predicted `v0.28.2`
   (a PATCH); it actually shipped in `v0.29.0` (a MINOR), because a feature-level change (from one
   of the other batched PRs) landed in the same release. The plan's own hedge — "re-read
   `workspace.package.version` at cut time" — is what caught the mismatch, which shows the hedge
   is necessary, not decorative; but the field is phrased as if it names *the* version, when the
   only thing knowable at planning time is a floor.
2. **The release PR body did not originally enumerate everything it covered.** v0.29.0's release
   PR body initially named only two of the three issues it actually batched. Nothing in the
   workflow's Phase 4 step 12 (Cut the release) currently requires the release PR body to
   enumerate every issue/PR folded into a batch — it only requires *a* PR, reviewed like any
   other.

## Approach

This is a pure documentation/workflow-text change — no Rust, no crate, no runtime behavior. Five
edits, all textual, in the canonical `.github/skills/otto-development/SKILL.md`, each mirrored
verbatim into the Claude Code port `.claude/skills/otto-development/SKILL.md`. The two files are
near-identical at these locations but not at a fixed offset: the port carries earlier,
already-shipped port-only additions, so the first two edit sites (Iron Law, Rule 8) sit +5 lines
in the port versus canonical, while the latter two (Phase 4 step 12 intro, rationalization table)
sit +39 lines — edits are therefore applied by matching the anchor text quoted below, never by
line-number arithmetic. See `CLAUDE.md`'s "Claude Code skills" section: a canonical edit that
lands nowhere near an adapted line is copied into the port as-is, which is the case for every edit
below; none of the five touched passages carries a host-mechanism substitution.

### 1. Reword "The Iron Law"'s release-mandatory paragraph

Currently asserts a bare "no batch carve-out" absolute. Reworded to state the same
mandatory-release requirement without contradicting practice: a merge is not shipped until a
release covers it, cut promptly, and — when other merges are already sitting unreleased on `main`
at cut time — batched into that same release rather than left to wait for a narrower one.

### 2. Reword Non-Negotiable Rule 8

This is the rule of record; the other four edits are downstream consequences of it. New wording
keeps the mandatory-release clause (no PR ships without a version bump, `CHANGELOG.md` entry,
pushed tag, and published Release) and adds, explicitly:

- **Batching already-merged, unreleased work is permitted — expected, even** — because the global
  version file and changelog make one-release-per-PR structurally unachievable whenever another
  job's merge is already on `main` and unreleased. A release cut while such work sits unreleased
  MUST fold it in rather than ship a narrower release and strand it.
- **What stays forbidden**: a merge left unreleased indefinitely because every job assumes some
  other job will eventually cut it. The mandatory-release requirement's force is unchanged — only
  the unit of "release" moves from "one per PR" to "one per batch of whatever is unreleased on
  `main` at cut time."
- **A batched release's line is the highest SemVer bump required by any PR in the batch**, not
  the one any single plan predicted (directly resolves the `#128` knock-on).
- **The release PR body must enumerate every issue/PR the release covers**, not only the one that
  triggered the cut (directly resolves the v0.29.0 knock-on).
- Batching covers only what is *already* merged and unreleased **at cut time** — it is never
  license to deliberately delay cutting a release now on the theory that a future PR will batch
  with it. This closes the door on "I'll batch mine with the next one" as a way to skip Phase 4
  step 12 promptly.

### 3. Reframe the plan format's `Release line:` field as a floor

Phase 2 step 5's bullet currently reads as a version prediction ("the next `vX.Y.Z` this work
ships as"). Reworded to state a floor — the minimum bump this work alone requires ("at least
PATCH" / "at least MINOR") — with an explicit note that Rule 8 permits the actual release to batch
in other work and ship under a higher line, so the field is honest about what is knowable at
planning time versus what is only knowable at cut time.

### 4. Add batching mechanics to Phase 4 step 12 (Cut the release)

The step's intro paragraph currently repeats the old "not batchable" framing; reworded to match
the new Rule 8, with an explicit instruction to check `workspace.package.version`, `gh release
list`, and recently-merged PRs against `main` before assuming a release covers only the agent's
own PR. Three of the six numbered sub-steps gain one clause each:

- **Bump the version** — pick the highest bump required across the whole batch, not the plan's
  predicted floor.
- **Update `CHANGELOG.md`** — include an entry for every PR the release batches in.
- **Commit, open a PR, and merge** — the release PR body must enumerate every issue/PR the release
  covers (this is the acceptance criterion's "release step requires the PR body to enumerate every
  issue in the batch").

### 5. Update the stale rationalization-table entry

"This is a tiny PR, I'll batch the release with the next one" currently gets the answer "Rule 8
has no batching carve-out." That answer is now false for the *permitted* case (already-merged,
unreleased work) and needs to keep being true for the *actually-wrong* case (deliberately
delaying a release now in anticipation of a future merge). Reworded to draw that line explicitly.

## Scope

**In:**

- `.github/skills/otto-development/SKILL.md` (canonical) — the five edits above.
- `.claude/skills/otto-development/SKILL.md` (port) — the same five edits, mirrored verbatim (no
  host-mechanism substitution needed at any of the five locations).
- `.github/skills/otto-development/claude-port/SKILL.md.diff` — regenerated via
  `bash .github/scripts/check-claude-skill-ports.sh --update`.

**Out:**

- `.github/skills/otto-development/agent-prompts.md` / its port — not touched; no dispatch
  template references batching.
- Any other skill (`creating-github-issues`, `rust-engineer`, etc.) — Rule 8 belongs to
  `otto-development` alone.
- Re-litigating whether batching *should* be permitted — the job brief and the issue's acceptance
  criteria already settle this: batching already-merged work is standing, owner-approved practice;
  this change makes the rule match it, not the other way around.
- Any Rust code, crate, or public-interface change.
- Issues #133 and #137 — same two files, different, already-filed/already-fixed defects.

## Public-interface changes

**None.** This touches only the canonical and ported `SKILL.md` prose and the port's regenerated
diff record. No SPP wire type, `ProviderHandler`/`ProviderClient` method, tool MCP schema, plugin
ABI surface, slash command, env var, or on-disk transcript/keyring format is added, renamed, or
removed. Non-Negotiable Rule 6 is not engaged.

## Assumptions

- **Batching is reworded as permitted, not reaffirmed as forbidden.** The acceptance criteria
  offer both options; the issue's own "What the rule should probably say" section and the job's
  own note ("per project convention, when Rule 8's own release step runs for THIS job, batching in
  already-merged unreleased work from main is permitted with the repository owner's standing
  approval") both point at the permitted-batching framing, and it is the only framing consistent
  with what the workflow actually cannot prevent structurally (a global version file, concurrent
  jobs, expired claims).
- **The edit is mirrored verbatim into the port with no host-mechanism adaptation**, because none
  of the five touched passages names a Copilot-CLI-specific mechanism (`agent_type`, `mode:
  "sync"`, `read_agent`, `ask_user`, a session-scoped `todos` table) — they are pure workflow
  prose about releases, identical in meaning for both hosts. This matches `CLAUDE.md`'s own
  description of the common case: "a canonical edit that lands nowhere near an adapted line ... is
  copied into the port as-is."
- **The five edits are exhaustive** for the phrase "batch"/"batchable" in the canonical
  `SKILL.md`: `grep -n "batch" .github/skills/otto-development/SKILL.md` returns exactly four
  hits (the Iron Law paragraph, Rule 8, Phase 4 step 12's intro, and the rationalization table),
  and edits 1, 2, 4, and 5 address exactly those four, one each — edit 3 (the `Release line:`
  floor reframing) is the one edit of the five that isn't a grep hit, since it's a knock-on
  consequence of Rule 8 rather than a restatement of the batching prohibition itself. No other
  file in either skill references batching.
- **The `Release line:` floor wording keeps the field required**, not making it optional — the
  acceptance criterion asks for reframing the *guidance*, not removing the field; a plan still
  states its own PATCH/MINOR floor, it just no longer implies that floor is the number the release
  will ship under.
- **This is fast-path-adjacent but not fast-pathed**, per the job's explicit instruction requiring
  the full spec → plan → implement → PR → review → merge → release lifecycle, and because the
  acceptance criteria run to six bullets (not one sentence) and the change touches two logical
  files (canonical + port, which do count as one under the fast-path's own "required
  mirror/duplicate" carve-out) plus a regenerated diff record and carries real judgment about how
  a Non-Negotiable Rule should read — not a mechanical one-line fix.

## Goal & Success Criteria

Rule 8 (and its Iron Law summary, its plan-format guidance, its release-step mechanics, and its
rationalization-table entry) states what the workflow actually does and is structurally capable of
doing, without weakening the one part of the old rule that is load-bearing: no merge ships without
an eventual release.

- Rule 8 (canonical and port) keeps the mandatory-release requirement's force, states that
  batching already-merged, unreleased work at cut time is permitted, and states that batching
  covers only what is *already* merged and unreleased — never a deliberate wait for a future PR.
- Rule 8 states that a batched release's line is the highest SemVer bump required across the
  batch.
- Phase 4 step 12's "Commit, open a PR, and merge" sub-step requires the release PR body to
  enumerate every issue/PR the release covers.
- Phase 2 step 5's `Release line:` guidance describes a floor ("at least PATCH"/"at least MINOR"),
  not a predicted version number, and cross-references Rule 8's highest-bump-across-the-batch
  rule.
- The Iron Law paragraph and the rationalization-table entry no longer contradict the reworded
  Rule 8.
- `bash .github/scripts/check-claude-skill-ports.sh` exits 0 with `SKILL.md.diff` regenerated to
  reflect the (verbatim, non-divergent) canonical edit.
- `cargo build && cargo test --workspace` and `bacon clippy-all` remain clean (vacuously — no Rust
  is touched).

## Error Handling & Edge Cases

- **Editing the port without editing the canonical first** would violate `CLAUDE.md`'s "Edit the
  canonical file, mirror the edit into the port, then regenerate the record" ordering and would be
  caught by `check-claude-skill-ports.sh` as a port-edited-directly mismatch. Mitigated
  procedurally: canonical edits land first, port edits are a verbatim copy of the same diff,
  `--update` runs last.
- **Regenerating the diff record before mirroring the edit into the port** would commit a stale
  record. Mitigated by running `--update` only after both files carry the edit, then eyeballing
  the regenerated `SKILL.md.diff` for the expected (small, unchanged-shape) hunks.
- **Over-broadening the change to also touch `agent-prompts.md`** — none of that file mentions
  batching or release lines, so there is nothing to edit there; confirmed by grep before writing
  this spec.
- **Wording the new Rule 8 so loosely that it re-legitimizes "I'll delay my release and hope
  someone else's batches with mine"** — mitigated by the explicit "at cut time" qualifier and the
  rationalization-table edit, both of which name and reject that specific misreading.

## Risks & Open Questions

- **None identified.** Every edit is a bounded prose correction to a workflow document, in files
  with no runtime behavior to regress, following a precedent (`otto-development-readonly-review-fix`,
  `otto-development-drop-deb-rpm`) of small canonical+port doc changes with mechanical
  `check-claude-skill-ports.sh --update` regeneration.

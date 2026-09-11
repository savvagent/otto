# otto-development-readonly-review-fix Implementation Plan

**Goal:** Restore the read-only guarantee on the Claude Code port's Code Quality Review (Phase 3
step E) and Final Code Review (Phase 3 step H) dispatch templates in
`.claude/skills/otto-development/agent-prompts.md`, so the three port assertions that already claim
this property become true; fix four smaller drifts in the same skill body per `savvagent/otto#137`
(a `done`→`completed` vocabulary slip, an inaccurate `/clear` claim, a contradiction in the security
prompt's diff-delivery prose, and two stale bullets in the skill-parity design spec); and fix a
counter-ordering bug in `.github/scripts/check-claude-skill-ports.sh`.

**Architecture:** Six independent, mechanical prose/script edits across four files: two are
port-only edits to `.claude/skills/otto-development/{agent-prompts.md,SKILL.md}` with no canonical
counterpart change (the canonical is verified correct as-is for each); one is a direct edit to a
repo design doc (`docs/superpowers/specs/2026-09-10-claude-skill-parity-design.md`); one is a direct
edit to the check script (`.github/scripts/check-claude-skill-ports.sh`). The two port-only edits
require regenerating their expected-diff records via `bash .github/scripts/check-claude-skill-ports.sh
--update` after editing. No Rust code, no crate, no public interface.

**Tech Stack:** Markdown (skill/spec docs) and POSIX shell (`check-claude-skill-ports.sh`, edited
directly — not a port). `bash .github/scripts/check-claude-skill-ports.sh` (existing) regenerates and
verifies the port-parity records.

**Spec:** `docs/superpowers/specs/2026-09-11-otto-development-readonly-review-fix-design.md` — read
it first. This plan implements it exactly.

**Release line:** next PATCH after whatever `workspace.package.version` reads at cut time (currently
`0.30.0` as of branch creation — re-read at cut time per the release step below, since `origin/main`
moves while this branch is open). Docs/script-only, no runtime behavior change, no public-interface
change → PATCH per `CHANGELOG.md`'s convention.

**Branch:** `docs/otto-development-readonly-review-fix`

## File Map

**New files**
- None.

**Modified files**
- `.claude/skills/otto-development/agent-prompts.md` — read-only constraint added to two templates;
  security-prompt diff-delivery contradiction removed (port).
- `.claude/skills/otto-development/SKILL.md` — `done`→`completed` fix; `/clear` claim corrected
  (port).
- `.github/skills/otto-development/claude-port/agent-prompts.md.diff` — regenerated.
- `.github/skills/otto-development/claude-port/SKILL.md.diff` — regenerated.
- `docs/superpowers/specs/2026-09-10-claude-skill-parity-design.md` — two failure-mode-enumeration
  corrections (direct repo doc, not a port).
- `.github/scripts/check-claude-skill-ports.sh` — `checked` increment-ordering fix.
- `CHANGELOG.md` — a `Fixed` entry under `## [Unreleased]` (added in the dedicated release PR per
  Phase 4 step 12, not in this PR — see Task 2).

## Task 1: Restore the read-only guarantee and fix the five smaller drifts

**Files:**
- Modify: `.claude/skills/otto-development/agent-prompts.md`
- Modify: `.claude/skills/otto-development/SKILL.md`
- Modify: `docs/superpowers/specs/2026-09-10-claude-skill-parity-design.md`
- Modify: `.github/scripts/check-claude-skill-ports.sh`
- Regenerate: `.github/skills/otto-development/claude-port/agent-prompts.md.diff`
- Regenerate: `.github/skills/otto-development/claude-port/SKILL.md.diff`

No Rust, so no `cargo test` step in this task. Verification is
`bash .github/scripts/check-claude-skill-ports.sh`, run before and after, plus targeted `grep`
checks.

- [ ] **Step 1: Confirm the current state.** From the worktree root:
  ```bash
  bash .github/scripts/check-claude-skill-ports.sh
  ```
  Expect exit 0 (the tree is currently in sync — this run is a baseline, not a fix target).

- [ ] **Step 2: Add the read-only constraint to the Code Quality Review template**, in
  `.claude/skills/otto-development/agent-prompts.md`, at the `## Code Quality Review — Phase 3 Step
  E` section. Insert a new paragraph immediately after the existing prose paragraph ("Claude Code's
  read-only diff reviewer is the built-in `/code-review` skill... the body is what constrains a
  general-purpose agent to a read-only, high-confidence-only diff review.") and before the "Capture
  commit boundaries first" sentence:

  ```
  **READ-ONLY, HARD CONSTRAINT.** Do not modify, create or delete any file. Do not commit, push,
  stage, stash, or run any command that writes — to the worktree, to git, or to GitHub. `git log`,
  `git diff`, `git show` and other read commands are fine. If you believe a fix is needed, describe
  it in your findings with a file:line ref and the concrete change — do not apply it. Report only
  high-confidence bugs and logic errors; skip low-confidence stylistic guesses.
  ```

  Then, inside the fenced `prompt: |` body itself (the part actually sent to the dispatched agent),
  add the same constraint as the prompt's opening lines, before "Review the code changes between
  <BASE_SHA> and <HEAD_SHA>.":

  ```
  READ-ONLY, HARD CONSTRAINT. Do not modify, create or delete any file. Do not commit, push, stage,
  stash, or run any command that writes — to the worktree, to git, or to GitHub. Read commands (git
  log, git diff, git show) are fine. If you believe a fix is needed, describe it in your findings
  with a file:line ref and the concrete change — do not apply it. Report only high-confidence bugs
  and logic errors.
  ```

  (Both additions are needed: the paragraph above the code fence is the human-readable rationale
  matching the existing prose style around the template; the line inside the `prompt: |` block is
  what actually reaches the dispatched agent, matching how the Independent Security Review template
  further down in the same file carries its constraint inside its own `prompt: |` block.)

- [ ] **Step 3: Add the identical read-only constraint to the Final Code Review template**, at the
  `## Final Code Review — Phase 3 Step H` section, same shape: a prose-paragraph addition after "Same
  substitution as Step E: the built-in `/code-review` skill, or `general-purpose` with this body
  verbatim." and before the code fence, plus the constraint as the opening lines inside the
  `prompt: |` body, before "Final review of the complete implementation." Use the same two blocks of
  text as Step 2 (word-for-word — both templates get the identical constraint, since both are the
  same kind of read-only diff review).

- [ ] **Step 4: Fix the security prompt's diff-delivery contradiction.** In the same file's
  `### Independent security review` section, find the sentence "Pass the diff by writing `gh pr diff
  <N>` to a file and naming that path in the prompt, or inline it — but never pass the spec, the
  plan, the brief, the PR body or an implementer report." Replace it with: "Pass the diff by having
  the dispatched agent run `gh pr diff <N>` itself, exactly as the prompt body below instructs — but
  never pass the spec, the plan, the brief, the PR body or an implementer report." Do not touch the
  `prompt: |` body itself — it already says the true thing ("Read ONLY the diff: `gh pr diff <N>`.").

- [ ] **Step 5: Verify Steps 2-4.**
  ```bash
  grep -n "READ-ONLY, HARD CONSTRAINT" .claude/skills/otto-development/agent-prompts.md
  ```
  Expect 5 matches: 4 new (2 prose + 2 in-prompt, for Code Quality Review and Final Code Review)
  plus the 1 pre-existing match in the Independent Security Review section's own "READ-ONLY, HARD
  CONSTRAINT" `prompt: |` body (confirmed present in the file before this task starts).
  ```bash
  grep -n "write \`gh pr diff\|naming that path" .claude/skills/otto-development/agent-prompts.md
  ```
  Must return nothing (the contradiction is gone).

- [ ] **Step 6: Fix the `done`→`completed` vocabulary slip.** In
  `.claude/skills/otto-development/SKILL.md`, Phase 3 step F ("Quality fix loop"), find "No
  Critical/Important → mark the task's todo `done`; record any Minor issues in per-task ledger."
  Change `done` to `completed`.

- [ ] **Step 7: Correct the `/clear` claim.** In `.claude/skills/otto-development/SKILL.md`'s Phase 0
  pre-flight section, find: "This orchestrating CLI has no `/clear`-and-reinvoke primitive of its own
  within a session, so the only fresh-context mechanism available to it is dispatching isolated
  subagents via the `Agent` tool". Replace with wording that states the narrower, correct claim — a
  running agent cannot invoke `/clear` on itself mid-run to reset its own context (Claude Code does
  ship a user-facing `/clear`, but it is not a tool call available to the session executing this
  skill) — while preserving the conclusion that the `Agent` tool dispatch is the only fresh-context
  mechanism available to a dispatched step. Example replacement text: "Claude Code ships a
  user-facing `/clear` command, but a running agent cannot invoke it on itself mid-run to reset its
  own context — `/clear` is not a tool call this session can make. So the only fresh-context
  mechanism available to a dispatched step is the isolated subagent context created by the `Agent`
  tool". Keep the surrounding sentence structure (the "Note:" callout about `README.md`'s `/clear`
  documenting the otto *product's* own slash command) unchanged — only the "This orchestrating CLI
  has no..." sentence is being corrected.

- [ ] **Step 8: Verify Steps 6-7.**
  ```bash
  grep -n "mark the task's todo \`done\`" .claude/skills/otto-development/SKILL.md
  ```
  Must return nothing.
  ```bash
  grep -n "This orchestrating CLI has no" .claude/skills/otto-development/SKILL.md
  ```
  Must return nothing. (Note: a grep for the old phrase's back half,
  "`/clear`-and-reinvoke primitive", is NOT a valid check here — that phrase wraps across two source
  lines in the current file, so a single-line grep for it already returns nothing before the edit,
  making it a vacuous check either way. Grepping for the old sentence's opening, which lives on one
  line, is the check that actually distinguishes before/after.) Additionally confirm the new wording
  is present:
  ```bash
  grep -n "cannot invoke it on itself mid-run\|cannot invoke \`/clear\` on itself" .claude/skills/otto-development/SKILL.md
  ```
  Expect a match (adjust the pattern if Step 7's exact replacement wording differs, but some
  positive assertion of the new text must be checked, not just the old text's absence).

- [ ] **Step 9: Confirm no canonical file was touched.**
  ```bash
  git status --porcelain .github/skills/otto-development/SKILL.md .github/skills/otto-development/agent-prompts.md
  ```
  Must return nothing — Steps 2-8 only touch the port files.

- [ ] **Step 10: Regenerate the port-parity records.**
  ```bash
  bash .github/scripts/check-claude-skill-ports.sh --update
  ```
  Confirm it reports rewriting exactly
  `.github/skills/otto-development/claude-port/agent-prompts.md.diff` and
  `.github/skills/otto-development/claude-port/SKILL.md.diff` (both port files changed; no other
  skill's record should be touched).

- [ ] **Step 11: Verify the check passes.**
  ```bash
  bash .github/scripts/check-claude-skill-ports.sh
  ```
  Must exit 0.

- [ ] **Step 12: Eyeball the regenerated records.**
  ```bash
  git diff --stat .github/skills/otto-development/claude-port/
  cat .github/skills/otto-development/claude-port/agent-prompts.md.diff | grep -c "READ-ONLY, HARD CONSTRAINT"
  ```
  Confirm the new hunks in `agent-prompts.md.diff` include the added read-only constraint text (as
  `+` lines) and the removed "write to a file" sentence (as a `-`/`+` pair), and that
  `SKILL.md.diff` shows the `done`→`completed` and `/clear` hunks. No unrelated hunks should appear.

- [ ] **Step 13: Align the design spec's failure-mode enumerations.** In
  `docs/superpowers/specs/2026-09-10-claude-skill-parity-design.md`:

  1. In `## Goal & Success Criteria`, find the bullet "`.github/scripts/check-claude-skill-ports.sh`
     exits 0 on the committed tree and exits non-zero for each failure mode: canonical edited without
     mirroring, port edited directly, missing port, missing or orphaned record, orphaned port, empty
     record, verbatim-copy port, unreadable file, symlinked or ill-named input, and
     `name`/`description` drift." Add two clauses to the enumeration: a port missing its "Ported for
     Claude Code" note, and an undeclared `.claude/skills/` entry that is neither a port nor named in
     `NATIVE_SKILLS`.
  2. In `## Error Handling & Edge Cases`, find the bullet "`.claude/skills/` entry with no canonical
     counterpart** → ignored by design; the check iterates `.github/skills/`, so `rust-engineer` and
     `tui-engineer` are untouched." This is now factually wrong — reword it to state that such an
     entry fails unless it is named in the script's `NATIVE_SKILLS` allowlist (a deliberate,
     reviewable declaration), matching how `CLAUDE.md`'s "Claude Code skills" section already
     documents this behavior.
  3. The same file has a second, forward cross-reference to this same now-stale claim: the bullet "A
     port `*.md` with no canonical counterpart *inside a ported skill*" ends with the parenthetical
     "(This is distinct from a whole `.claude/skills/` entry with no canonical directory, below,
     which is ignored by design.)" Update this parenthetical too, so it points at the corrected
     behavior from edit 2 above instead of repeating the stale "ignored by design" claim — e.g.
     "...which fails unless declared in `NATIVE_SKILLS`, see below." Both occurrences of "ignored by
     design" in this file describe the same claim and must be corrected together, or the check in
     Step 14 below will find one fixed and one still stale.

  Do not touch this spec's `> **Status:**` header (stays `IMPLEMENTED`) or any other section — this
  is a targeted accuracy correction to an already-shipped spec, not a scope change.

- [ ] **Step 14: Verify Step 13.**
  ```bash
  grep -n "Ported for Claude Code.*note\|NATIVE_SKILLS" docs/superpowers/specs/2026-09-10-claude-skill-parity-design.md
  ```
  Expect matches in both the updated `Goal & Success Criteria` bullet and the updated
  `Error Handling & Edge Cases` bullet.
  ```bash
  grep -n "ignored by design" docs/superpowers/specs/2026-09-10-claude-skill-parity-design.md
  ```
  Must return nothing (the stale bullet has been reworded).

- [ ] **Step 15: Fix the `checked` increment-ordering bug.** In
  `.github/scripts/check-claude-skill-ports.sh`, find the loop body around the "1-3. Every canonical
  Markdown file" comment. Currently:
  ```bash
          checked=$((checked + 1))

          reject_symlink "$canonical" || continue

          if [ -e "$port" ] || [ -L "$port" ]; then
              reject_symlink "$port" || continue
          fi
          if [ ! -f "$port" ]; then
              fail "skill '$name': no Claude Code port of $rel. ..."
              continue
          fi

          # 4. The note, asserted here because this loop is already reading every
          ...
  ```
  Move the `checked=$((checked + 1))` line to immediately after the `if [ ! -f "$port" ]; then ... fi`
  block and before the "# 4. The note" comment, so it only counts a file once it has survived the
  symlink rejection and the missing-port rejection. Result:
  ```bash
          reject_symlink "$canonical" || continue

          if [ -e "$port" ] || [ -L "$port" ]; then
              reject_symlink "$port" || continue
          fi
          if [ ! -f "$port" ]; then
              fail "skill '$name': no Claude Code port of $rel. ..."
              continue
          fi

          checked=$((checked + 1))

          # 4. The note, asserted here because this loop is already reading every
          ...
  ```
  Do not change any other line in the loop, and do not change the `fail`/`continue` control flow
  itself — only the position of the increment.

- [ ] **Step 16: Verify Step 15 behaviorally.**
  ```bash
  bash .github/scripts/check-claude-skill-ports.sh
  ```
  Must still exit 0, with the same summary counts as Step 11's run (the moved increment changes
  nothing about which files pass, only where in the loop `checked` is incremented — on a fully
  passing tree every file reaches the new increment point exactly as it reached the old one).

- [ ] **Step 17: Public-interface note.** No SPP wire type, tool schema, plugin ABI, slash command,
  env var, or on-disk format touched — Non-Negotiable Rule 6 is not engaged. No `CHANGELOG.md`
  interface note owed by this task.

- [ ] **Step 18: Host-swap / streaming invariants — vacuously satisfied.** No `crates/otto/src/app.rs`
  or `tui.rs` touched, so the host-swap `RwLock` rule is not engaged. No streaming provider path
  touched, so the `ProgressDispatcher` forwarder-abort pattern is not engaged.

- [ ] **Step 19: Format and commit.** No Rust changed, so `cargo fmt --all` is a no-op here but run
  it anyway for consistency with house style:
  ```bash
  cargo fmt --all
  git add .claude/skills/otto-development/agent-prompts.md \
          .claude/skills/otto-development/SKILL.md \
          .github/skills/otto-development/claude-port/agent-prompts.md.diff \
          .github/skills/otto-development/claude-port/SKILL.md.diff \
          docs/superpowers/specs/2026-09-10-claude-skill-parity-design.md \
          .github/scripts/check-claude-skill-ports.sh
  git commit -m "docs: restore read-only guarantee on ported code-review dispatches"
  ```

## Task 2: Cut the release (notes only — performed per Phase 4 step 12, not in this PR)

**Files:** none in this PR.

- [ ] **Step 1:** This PR does **not** bump `workspace.package.version` and does **not** add a
  `CHANGELOG.md` section — that happens in the dedicated release PR after this merges, per
  Non-Negotiable Rule 8 / Phase 4 step 12. Re-read `workspace.package.version` at cut time (it may
  have moved past `0.30.0` if another PR merges first) and cut the next PATCH from whatever it then
  reads. The `CHANGELOG.md` entry: a `Fixed` bullet describing that the Claude Code port's Code
  Quality Review and Final Code Review dispatch templates are now read-only, high-confidence-only, as
  the port already claimed; plus mention of the smaller vocabulary/prose/script corrections folded in
  from the same issue.

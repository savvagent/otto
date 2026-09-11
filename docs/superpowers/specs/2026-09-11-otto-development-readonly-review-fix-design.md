# Restore the read-only guarantee on the ported code-review dispatches — design

Date: 2026-09-11
Status: pending review
Source: savvagent/otto#137
Related: `.github/skills/otto-development/agent-prompts.md`'s Independent security review section
(the same substitution class, already fixed correctly there); follow-up to #128/#131. Distinct from
#133 (untrusted-input hardening) and #138 (Rule 8 batching vs. practice) — same skill body, different
defects.

## Problem

The Claude Code port of `otto-development` maps the canonical `agent_type: "code-review"` dispatches
(Code Quality Review, Phase 3 step E, and Final Code Review, Phase 3 step H) onto
`subagent_type: "general-purpose"`. The canonical `code-review` agent type is read-only by
construction — it has no Bash/Edit/Write. `general-purpose` is not: it holds the full toolset in the
live worktree. The security-pass substitution in the same file got the correct treatment for exactly
this gap: `.claude/skills/otto-development/agent-prompts.md`'s Independent security review template
carries an explicit "READ-ONLY, HARD CONSTRAINT" paragraph, with the reasoning ("an auditor that also
writes is not an independent gate") stated inline. The Code Quality Review and Final Code Review
templates were mirrored from the canonical verbatim and never received the same treatment.

Three places in the port assert the templates already have this property, and none of it is true of
the template bodies:

- `.claude/skills/otto-development/agent-prompts.md:225` — "the body is what constrains a
  general-purpose agent to a read-only, high-confidence-only diff review"
- `.claude/skills/otto-development/SKILL.md:394` (the "How dispatch works" table, Code quality
  review / final code review row) — "The template in `agent-prompts.md` is what makes a
  `general-purpose` agent behave that way, so paste it verbatim"
- `.claude/skills/otto-development/SKILL.md:633-634` (Phase 3 step E) — "the template is what turns
  a `general-purpose` agent into a read-only diff reviewer"

This was demonstrated, not just theorized: on PR #131 the Phase 3 step H dispatch behaved read-only
only because the orchestrator hand-added a read-only line the template does not supply — the
template alone would not have constrained it. Non-Negotiable Rule 4 makes these review passes
mandatory on every PR; an auditor that can also write is not an independent gate.

The issue also folds in five smaller defects in the same two port files, found in the same review
pass:

1. `.claude/skills/otto-development/SKILL.md:640` ("No Critical/Important → mark the task's todo
   `done`") uses a status value (`done`) that does not exist in the port's own vocabulary. The port's
   own Phase 0 (`SKILL.md:356`) redefines the status set as `pending` → `in_progress` → `completed`
   specifically because Claude Code's `TodoWrite` tool has no `done` state — the canonical's own
   todo tool *does* use `done`/`blocked` (canonical `SKILL.md:352`), so this line is a straight
   verbatim-copy artifact that was never adapted to the port's own redefinition two sections earlier
   in the same file. (The canonical's own equivalent line is at `SKILL.md:603` — the port is offset
   ~37 lines later throughout by its earlier, already-shipped security read-only paragraph.)
2. `.claude/skills/otto-development/SKILL.md:312-313` states "This orchestrating CLI has no
   `/clear`-and-reinvoke primitive of its own within a session." This sentence is verbatim-identical
   to the canonical, where it is accurate (the Copilot CLI referenced there has no user-facing
   `/clear`). Claude Code does ship a user-facing `/clear` slash command, so the same sentence reads
   as a factual error to a Claude Code operator, even though the operative conclusion — a dispatched
   agent cannot invoke `/clear` on itself mid-run to reset its own context, which is why the `Agent`
   tool is the only fresh-context mechanism available to the orchestrating session — still holds.
3. `.claude/skills/otto-development/agent-prompts.md`'s Independent security review section
   (the paragraph before the template) says the orchestrator may "[pass] the diff by writing
   `gh pr diff <N>` to a file and naming that path in the prompt, or inline it." The template body
   itself says unconditionally "Read ONLY the diff: `gh pr diff <N>`" — i.e. the dispatched agent
   runs the command itself; the template has no place to receive a file path or inlined diff text.
   The prose offers two delivery mechanisms the body doesn't implement.
4. `docs/superpowers/specs/2026-09-10-claude-skill-parity-design.md`'s two failure-mode
   enumerations (`Goal & Success Criteria` and `Error Handling & Edge Cases`) predate two assertions
   the shipped `check-claude-skill-ports.sh` actually makes: the "Ported for Claude Code" note
   requirement (script lines ~551-561) and the `NATIVE_SKILLS` top-level sweep (script lines
   ~692-714, which rejects any `.claude/skills/` entry that is neither a port nor declared native).
   `CLAUDE.md`'s "Claude Code skills" section documents both correctly already; the spec has drifted
   behind the shipped check it describes. The spec's `Error Handling & Edge Cases` bullet "`.claude/
   skills/` entry with no canonical counterpart → ignored by design" is now actively wrong — the
   shipped check does not ignore this case, it fails unless the entry is named in `NATIVE_SKILLS`.
5. `.github/scripts/check-claude-skill-ports.sh:539` increments the `checked` counter before the
   symlink rejection (`541`) and the missing-port rejection (`546-549`) that can `continue` past the
   rest of that file's checks. `checked` is documented (in its own success-path `printf`, line 736-737)
   as counting "canonical skill file(s) ... [that] match their Claude Code port and its recorded
   divergence" — a file that got rejected by one of those two checks never had that verified, so
   counting it anyway is wrong bookkeeping. (Today this has no visible effect, because any `fail()`
   call — which both rejections trigger — sets `failures > 0`, and the script exits 1 before either
   summary `printf` that reads `$checked` is reached; the fix is still correct bookkeeping, defensive
   against a future refactor of the exit gate, and is what the issue asks for.)

## Approach

All five defects are prose/script corrections in three files, no Rust, no public interface.

### 1. Add the read-only constraint to the two code-review templates (port-only)

Add a "READ-ONLY, HARD CONSTRAINT" paragraph, shaped like the one already proven correct in the
Independent security review template (`agent-prompts.md`'s security section), to the start of both
the **Code Quality Review** (Phase 3 step E) and **Final Code Review** (Phase 3 step H) prompt bodies
in `.claude/skills/otto-development/agent-prompts.md`. Each gets:

- A `READ-ONLY, HARD CONSTRAINT` sentence: do not modify/create/delete any file, do not commit, push,
  stage, stash, or run any write command — to the worktree, git, or GitHub; read-only commands
  (`git log`, `git diff`, `gh pr diff`, etc.) are fine; describe a needed fix with a file:line ref and
  the concrete change instead of applying it.
- A "report only high-confidence bugs/logic errors" line, so the reviewer doesn't flood the loop with
  low-confidence stylistic noise the same way `Explore`/quality passes are meant to avoid.

This is a **port-only** edit, exactly as CLAUDE.md's "Claude Code skills" section describes as the
one legitimate shape of port divergence (an adapted region responding to a host-mechanism gap the
canonical doesn't have, since the canonical's `code-review` agent type needs no such prose — it is
read-only by construction). The canonical `.github/skills/otto-development/agent-prompts.md` is
**not** touched.

Once the templates carry this constraint, the three assertions listed in the Problem section become
true of the template bodies as written — no additional wording change to those three assertions is
needed; they are corrected by making the templates match the claim, per the issue's suggested fix
("mirrors the shape already used for the security prompt").

### 2. Fix the `done` → `completed` vocabulary slip (port-only)

In `.claude/skills/otto-development/SKILL.md`'s Phase 3 step F ("Quality fix loop"), change "mark the
task's todo `done`" to "mark the task's todo `completed`", matching the status vocabulary the port's
own Phase 0 already establishes for `TodoWrite`. Port-only: the canonical's own todo tool genuinely
uses `done`, so canonical line 603 stays as-is.

### 3. Correct the `/clear` claim (port-only)

In `.claude/skills/otto-development/SKILL.md`'s Phase 0 pre-flight section, reword the sentence "This
orchestrating CLI has no `/clear`-and-reinvoke primitive of its own within a session" to state the
narrower, still-true claim: a dispatched agent (or the orchestrating session itself, mid-run) cannot
invoke `/clear` on itself to reset its own context — `/clear` is a user-facing command, not a
tool call available to the running session. The conclusion that follows (the `Agent` tool dispatch is
the only fresh-context mechanism available) is unchanged and still holds; only the premise sentence
is corrected. Port-only: canonical's orchestrating CLI (Copilot CLI) genuinely has no user-facing
`/clear`, so the original sentence stays accurate there.

### 4. Remove the security-prompt diff-delivery contradiction (port-only)

In `.claude/skills/otto-development/agent-prompts.md`'s Independent security review section, the
prose sentence offering "writing `gh pr diff <N>` to a file and naming that path in the prompt, or
inline it" is dropped in favor of stating what the shipped template actually does: the dispatched
agent runs `gh pr diff <N>` itself, per the template body's own "Read ONLY the diff: `gh pr diff
<N>`" instruction. The template body itself is unchanged — it already says the true thing; only the
prose paragraph above it is corrected to stop promising a delivery mechanism the template doesn't
implement. Port-only, same reasoning as the canonical's equivalent sentence (`agent-prompts.md:360`,
canonical) needing no change: the canonical prose has no equivalent "or write to a file" alternative
to begin with.

### 5. Align the design spec's failure-mode enumerations (direct repo doc, not a port)

`docs/superpowers/specs/2026-09-10-claude-skill-parity-design.md` is a repo design doc, not one of
the two skill-port files — it is edited directly, with no canonical/port mirroring concern. Two
edits:

- `Goal & Success Criteria`'s failure-mode list gains two clauses: the "Ported for Claude Code" note
  requirement, and the `NATIVE_SKILLS` top-level sweep (an undeclared, non-native `.claude/skills/`
  entry with no canonical counterpart).
- `Error Handling & Edge Cases`'s bullet "`.claude/skills/` entry with no canonical counterpart →
  ignored by design" is corrected: the shipped check does *not* ignore this case — it fails unless
  the entry is named in the script's `NATIVE_SKILLS` allowlist. The bullet is reworded to describe
  the actual (correct, already-shipped) behavior, matching how `CLAUDE.md`'s "Claude Code skills"
  section already documents it.

This spec's own `> **Status:**` stays `IMPLEMENTED` — the feature it describes is shipped and this is
a documentation-accuracy correction to an already-implemented spec, not a status regression.

### 6. Fix the `checked` increment ordering (script, not a port)

In `.github/scripts/check-claude-skill-ports.sh`, move the `checked=$((checked + 1))` increment
(currently at line 539, before the symlink rejection at 541 and the missing-port rejection at
546-549) to immediately after those two checks pass — i.e., right before the "Ported for Claude Code"
note check that currently follows at line 551. This makes `checked` count only files that survived
both rejections, matching what the summary `printf` lines already claim it counts.

## Scope

**In:**

- `.claude/skills/otto-development/agent-prompts.md` (port) — read-only constraint on two templates;
  security-prompt contradiction removed.
- `.claude/skills/otto-development/SKILL.md` (port) — `done`→`completed` vocabulary fix; `/clear`
  claim correction.
- `.github/skills/otto-development/claude-port/agent-prompts.md.diff` and
  `.github/skills/otto-development/claude-port/SKILL.md.diff` — regenerated via
  `bash .github/scripts/check-claude-skill-ports.sh --update`, since both port files diverge further
  from their canonicals without any canonical change (this is the same shape of divergence the
  security-pass fix already established, and CLAUDE.md's own precedent for it).
- `docs/superpowers/specs/2026-09-10-claude-skill-parity-design.md` — two failure-mode-enumeration
  corrections (a direct repo-doc edit, not a port).
- `.github/scripts/check-claude-skill-ports.sh` — the `checked` increment-ordering fix.

**Out:**

- Any canonical `.github/skills/otto-development/{SKILL.md,agent-prompts.md}` edit. Every defect in
  this issue is a Claude-Code-host adaptation gap or a Claude-Code-reader-only factual error; the
  canonical is correct as written for its own host in every case reviewed above (confirmed by reading
  the corresponding canonical text for all five prose defects before writing this spec).
- Issue #133 (untrusted-input hardening) and #138 (Rule 8 batching vs. practice) — same two files,
  different, already-filed defects; not touched here.
- Any Rust code, crate, or public-interface change.
- Any behavior change to `check-claude-skill-ports.sh` beyond the counter-ordering fix — in
  particular, the fix does not change which files pass or fail, only what `checked` counts on the
  (currently unreachable, per the Problem section's parenthetical) success path where it would
  matter.

## Public-interface changes

**None.** This touches two `.claude/skills/otto-development/*.md` port files, their two regenerated
`.diff` records, one `docs/superpowers/specs/*.md` design doc, and one `.github/scripts/*.sh` check
script. No SPP wire type, no `ProviderHandler`/`ProviderClient` method, no tool MCP schema, no plugin
ABI surface, no slash command, no env var, and no on-disk transcript/keyring format is added,
renamed, or removed (Non-Negotiable Rule 6 is not engaged). No crate gains a dependency edge.

## Assumptions

- **The read-only constraint text should closely mirror the security template's**, not invent new
  phrasing, so a reader who has already internalized the security-pass constraint recognizes the same
  shape immediately in the two review templates. The "report only high-confidence bugs/logic errors"
  line is new phrasing (the security template has no equivalent, since it reports severity-graded
  findings by table, not a confidence filter) but is explicitly requested by the issue's suggested
  fix and mirrors this repo's own `pr-review-toolkit:code-reviewer`/`feature-dev:code-reviewer`
  agents' stated "confidence-based filtering."
- **The three false assertions need no wording change once the templates are fixed.** The issue's
  acceptance criteria accept either fixing the assertions to be true, or rewording them to match what
  the templates actually supply; making the templates match is the approach the issue's own
  "Suggested fix" section prescribes, and it is strictly less code churn than rewording three
  separate prose locations that already say the right thing once the underlying claim is true.
- **The `/clear` correction narrows the claim rather than deleting it.** The operative conclusion
  (no in-session self-`/clear`-and-reinvoke primitive) is still correct and still the reason the
  `Agent` tool dispatch pattern exists; only the premise needs qualifying, not removing.
- **The security-prompt contradiction is resolved by narrowing the prose, not by extending the
  template.** Extending the template to accept a file-path parameter would be a real behavior change
  to a mandatory, already-working review pass; the minimal, lowest-risk fix is to stop promising a
  delivery mode the template never implements.
- **The design-spec edit is in scope for this change** even though it's a third file beyond the two
  ported skill files, because the issue explicitly lists it as one of the "Also worth folding in"
  items under the same acceptance criteria, and it is a same-day, same-topic (skill-porting
  mechanism) correction with no dependency on the other edits.
- **This is fast-path-adjacent but not fast-pathed**, per the job instructions explicitly requiring
  the full spec → plan → implement → PR → review → merge → release lifecycle, and because the change
  touches 4 files (`agent-prompts.md`, `SKILL.md`, the design spec, and the check script) plus two
  regenerated diff records — over the fast-path's 2-logical-file cap.

## Goal & Success Criteria

The two ported code-review dispatch templates actually constrain a `general-purpose` agent to
read-only, high-confidence-only behavior, and every prose claim in the port about that behavior is
true of the templates as written.

- Both the Code Quality Review and Final Code Review templates in
  `.claude/skills/otto-development/agent-prompts.md` carry an explicit read-only, hard-constraint
  paragraph and a high-confidence-only instruction.
- The three previously-false assertions (`agent-prompts.md:225`, `SKILL.md`'s dispatch table, `SKILL.md`
  Phase 3 step E) are true of the templates as written, with no wording change needed to the
  assertions themselves.
- `.claude/skills/otto-development/SKILL.md`'s Phase 3 step F says `completed`, not `done`.
- `.claude/skills/otto-development/SKILL.md`'s Phase 0 `/clear` sentence states the narrower, correct
  claim (an agent cannot invoke `/clear` on itself mid-run) rather than the broader, now-false one.
- `.claude/skills/otto-development/agent-prompts.md`'s security-review prose no longer offers a
  diff-delivery mechanism ("write to a file and name that path") the template body doesn't implement.
- `docs/superpowers/specs/2026-09-10-claude-skill-parity-design.md`'s two failure-mode lists name the
  "Ported for Claude Code" note requirement and the `NATIVE_SKILLS` sweep, and its stale "ignored by
  design" bullet is corrected.
- `.github/scripts/check-claude-skill-ports.sh` increments `checked` only after a file survives the
  symlink and missing-port rejections.
- `bash .github/scripts/check-claude-skill-ports.sh` exits 0 on the resulting tree, with both
  `agent-prompts.md.diff` and `SKILL.md.diff` regenerated to reflect the new (still port-only)
  divergence.
- `cargo build && cargo test --workspace` and `bacon clippy-all` remain clean (vacuously — no Rust is
  touched, so this is a "nothing broke" check, not a behavior-change verification).

## Error Handling & Edge Cases

- **Regenerating records before mirroring the edit into the port** would commit a stale record that
  doesn't include the new text. Mitigated procedurally: edit the port file(s) fully first, then run
  `--update`, then eyeball the regenerated diff for the expected new hunks before committing — same
  discipline the `otto-development-drop-deb-rpm` precedent plan used.
- **Accidentally touching the canonical while intending a port-only edit** would violate the issue's
  explicit "this is a port-only edit" constraint and CLAUDE.md's precedence rule. Mitigated by the
  per-file scope list above and by re-running `git diff --stat` against
  `.github/skills/otto-development/` before committing to confirm it shows no changes.
- **The read-only constraint text drifting from the security template's proven wording** — mitigated
  by keeping the new paragraphs structurally parallel to the existing security constraint (same
  "READ-ONLY, HARD CONSTRAINT" opening, same "describe, don't apply" fallback instruction) rather
  than free-writing new phrasing.
- **The `checked`-ordering fix silently changing pass/fail behavior** — it does not: every `fail()`
  call already sets `failures > 0`, which exits the script before either `printf` that reads
  `$checked` is reached (see Problem section, item 5's parenthetical), so this fix is pure
  bookkeeping correctness with no observable behavior change today. Verified by running the check
  script before and after the edit and confirming identical exit code and pass/fail file set.

## Risks & Open Questions

- **None identified.** Every edit is a bounded, mechanical prose/script correction with an
  unambiguous correct wording already established by precedent (the security template's read-only
  constraint, `CLAUDE.md`'s already-correct NATIVE_SKILLS/note-requirement documentation, the port's
  own Phase 0 status vocabulary), in files with no runtime behavior to regress.

# claude-skill-parity Implementation Plan

> **For agentic workers:** Steps use checkbox (`- [ ]`) syntax for tracking. Implement task-by-task, in order.

> **Revised mid-flight.** This plan originally specified *adapter stubs* — small `.claude/skills/`
> files pointing at the canonical body plus a Copilot→Claude Code translation table. That design was
> rejected during implementation: porting a skill means adapting it so it **works** natively, and a
> stub still required the reader to follow a pointer into 82KB of Copilot-authored prose and apply a
> mapping while reading it. The plan below is the port design that replaced it, and Tasks 1–6 were
> executed against this version. The rejected design is recorded here only because the reasoning is
> worth keeping: see the spec's "Premise corrections".

**Goal:** Port this repo's own `.github/skills/` workflows — `otto-development` (plus its `agent-prompts.md` companion) and `creating-github-issues` — to Claude Code as adapted, self-sufficient copies under `.claude/skills/`, and make CI refuse to let the two copies drift apart.

**Architecture:** Each canonical file gets a **ported** counterpart under `.claude/skills/<name>/`, complete enough to execute start to finish without ever opening `.github/skills/`. The adaptation touches only host-mechanism references — `task`→`Agent`, the seven-value `agent_type` enum→`subagent_type`/built-in skills, `mode:`→one-call-vs-several-in-one-message, `read_agent`→task notifications and `SendMessage`, the `sql` `todos` table→`TodoWrite`, `ask_user`→`AskUserQuestion`, and `reasoning_effort`→model selection. Everything else, including every Non-Negotiable Rule and Load-Bearing Invariant, carries across byte-identical. Because the divergence is small — 132 of the 1,597 canonical lines, about 8%, replaced by 198 ported lines across 25 hunks — it is recorded exactly as a committed `diff -u` per ported file under `.github/skills/<name>/claude-port/`, and a CI-run script recomputes each diff and compares — so a canonical edit, a port edit, or a frontmatter drift all fail the build.

**Tech Stack:** Markdown (the ported skill bodies, `CLAUDE.md`), POSIX-ish bash (`.github/scripts/check-claude-skill-ports.sh`, `set -euo pipefail`), GitHub Actions YAML (`.github/workflows/ci.yml`). No Rust, no crate, no dependency change.

**Spec:** `docs/superpowers/specs/2026-09-10-claude-skill-parity-design.md` — read it first. This plan implements it exactly.

**Release line:** `v0.28.2` (PATCH — contributor-facing tooling and docs, no runtime behaviour change; trunk is at `0.28.1` as of the rebase — re-read `workspace.package.version` at cut time, since trunk moves while this branch is open).

**Branch:** `docs/claude-skills-parity`

---

## File Structure

| File | Role |
| ---- | ---- |
| `.claude/skills/otto-development/SKILL.md` (new) | Port of the canonical `SKILL.md`, adapted for Claude Code. What a Claude Code session actually executes. |
| `.claude/skills/otto-development/agent-prompts.md` (new) | Port of the canonical dispatch-prompt templates, adapted. |
| `.claude/skills/creating-github-issues/SKILL.md` (new) | Port of the canonical body. Needs **no** host adaptation — plain `gh issue`/`gh label` usage — so it diverges only by the "ported from" note. |
| `.github/skills/*/claude-port/*.diff` (new) | The committed record of intended divergence, one per ported file. Doubles as the review artifact: the port's entire delta from the canonical, in one place. |
| `.github/scripts/check-claude-skill-ports.sh` (new) | Recomputes each `diff canonical ported` and compares to the record; also **writes** every record through the same function under `--update`, which is the only way records are generated; asserts frontmatter parity separately; fails on missing/orphaned ports and records, an empty record, a verbatim-copy port, an unreadable file, and an empty canonical root; refuses symlinked and oddly-named inputs. First `.sh` in the repo. |
| `.gitattributes` (new) | `*.sh text eol=lf` for the shebang, plus `*.diff` and both skill trees pinned to LF so the records verify on a `core.autocrlf=true` clone. |
| `.github/workflows/ci.yml` (modify) | One step in the existing `lint` job, immediately after `actions/checkout@v4`, plus a workflow-level `permissions: contents: read`. |
| `CLAUDE.md` (modify) | Rewrite the "Claude Code skills" section: two-directory layout, the port mechanism and why, the edit-canonical-then-re-port workflow, the check, and the revised precedence rule. |

`.github/skills/**/*.md` — the canonical bodies — are **not** edited; only new `claude-port/` subdirectories are added beside them. `.claude/skills/rust-engineer/` and `tui-engineer/` are not touched: Claude-Code-native, no canonical counterpart, not iterated by the check.

**Test-first note.** No Rust, so no `cargo test` target. The check script is the executable specification: it is written before the ports exist, proven to fail for a real reason, and each failure mode is fault-injected and reverted.

---

### Task 1: The port-parity check (written first, must fail)

**Files:**
- Create: `.github/scripts/check-claude-skill-ports.sh`, `.gitattributes`
- Delete: `.github/scripts/check-claude-skill-stubs.sh` (the rejected design's check)

- [x] **Step 1: `.gitattributes`** — `*.sh text eol=lf`, with a comment saying why (a CRLF shebang fails confusingly). Also pin `*.diff` and both skill trees to LF: they are LF artifacts, and under `core.autocrlf=true` a checked-out record carries CRLF while the recomputed diff does not, so a Windows contributor running the check locally as documented could never get green.

- [x] **Step 2: Write the check.** Requirements:
  - `#!/usr/bin/env bash`, `set -euo pipefail`, repo root resolved from the script's own location.
  - A leading comment block explaining **why** it exists (per invariant 9), including the edit-canonical-then-re-port workflow and the limits of what it does and does not verify.
  - `::error::` annotation per failure, matching `.github/workflows/wit-dep-guard.yml`'s house style.
  - For each directory under `.github/skills/`: every canonical `*.md`, **at any depth** (a `reference/` subdirectory is a normal skill shape, and a non-recursive glob lets a nested canonical file drift unported behind a green build), must have a port at `.claude/skills/<name>/<relative path>` and a record at `.github/skills/<name>/claude-port/<relative path>.diff`. An orphaned record fails, and so does an orphaned **port** — a port with no canonical is a rule Claude Code executes that the source of truth does not contain, which contradicts the precedence rule.
  - Refuse to be defeated by an empty or unreadable input: `diff`'s exit 2 ("trouble", e.g. an unreadable file) must not be collapsed with exit 1 ("differ"), or it becomes an empty diff that compares equal to an empty record; and a zero-byte record must be rejected outright, since it asserts the port is a verbatim copy — the one arrangement this design exists to avoid.
  - Refuse to read hostile repository content. CI runs on `pull_request`, so a fork's tree reaches the script: reject symlinked canonicals, ports and records (`[ -f ]` follows a symlink, and the diff-of-diffs would print `.git/config` — where `actions/checkout` leaves the job token — into the public log); validate every path segment against `[A-Za-z0-9._-]+`; and escape `%`, CR and LF in `fail`, so a filename carrying a newline cannot forge an `::error::`/`::add-mask::`/`::stop-commands::` line at column 0.
  - Recompute the diff through a `port_diff()` function, invoked as `diff -u -L <canonical> -L <ported>` — plain `diff -u` emits mtimes, which would make every record machine- and date-specific. Give the script an `--update` mode that *writes* every record through that same function, so generation and verification structurally share one code path instead of agreeing by authoring coincidence, and default (no-argument) behaviour stays verify-and-exit-non-zero. `--update` must report which records it rewrote, and must not paper over the failures a regeneration cannot fix (missing port, orphan, frontmatter drift). Pin `LC_ALL=C`: GNU diffutils translates `\ No newline at end of file`, so an unpinned locale makes a record non-reproducible.
  - Compare byte-for-byte against the record — via `cmp`, not `$(...)` command substitution, which strips trailing newlines from both sides and hides exactly the drift the comparison claims to catch. On mismatch, distinguish the two repair cases (mirror the canonical edit verbatim vs. re-apply the adaptation on an adapted line), name the `--update` command, and print a diff-of-diffs.
  - Assert `name`/`description` parity **separately**, so the failure that changes trigger behaviour is reported as itself rather than as an opaque diff mismatch. This assertion carries more weight than it looks: "regenerate the record" is the endorsed fix for a red build, and after a regeneration it is the *only* surviving check. Read only the leading `---` fenced block, strip trailing whitespace (CR included), strip one optional layer of wrapping quotes, and fail loudly on a missing key, an empty value (before *and* after unquoting), a YAML block scalar (`>`/`|` and their chomping variants, which would let the two sides carry entirely different text), a duplicate key (parsers variously take the last or error), an unterminated frontmatter block, and a `key:` prefix that is really a different key (`name:s: v` declares `name:s`, not `name`). Report the actual cause — a missing `---` fence, which is where a UTF-8 BOM lands, is not "key missing".
  - Fail if `.github/skills/` has no skill directories — success over an empty set is the drift class most likely to go unnoticed.
  - Accumulate all failures; portable constructs only; never `eval` or unquote an extracted value (one canonical description contains backticks); `failures=$((failures + 1))` as an assignment so `set -e` does not kill the script.

- [x] **Step 3: Run it; confirm it fails for the right reason.** `exit=1`, naming the missing ports — not a syntax error, not an unbound variable, and no mention of `rust-engineer`/`tui-engineer`.

- [x] **Step 4:** `chmod +x`, commit as `ci: replace the skill-stub check with a skill-port parity check`. Confirm mode `100755` with `git ls-files -s`.

---

### Task 2: Port `creating-github-issues` (the zero-adaptation case)

**Files:** Create `.claude/skills/creating-github-issues/SKILL.md`

- [x] **Step 1: Copy the canonical exactly.** It contains **zero** host-mechanism references — verified by grep for `agent_type|read_agent|ask_user|Copilot|sql|mode: "`. Its port is a verbatim copy plus the note.

- [x] **Step 2: Insert the "ported from" note** immediately after the frontmatter's closing `---`, directing the reader to edit the canonical and re-port rather than editing in place, and naming the check.

- [x] **Step 3: Generate the record and run the check.** The divergence must be exactly the note — a 5-line hunk. Anything larger means the copy was not verbatim.

- [x] **Step 4:** Commit.

---

### Task 3: Port `otto-development` (the adaptation)

**Files:** Create `.claude/skills/otto-development/SKILL.md` and `agent-prompts.md`

- [x] **Step 1: Copy both canonicals exactly, then adapt only host mechanisms.** Apply the spec's translation table. Do not reword, reorder, tighten, improve or reformat anything else — the design depends on the divergence staying small and reviewable.

- [x] **Step 2: Rewrite the two sections that cannot be substituted.** "How dispatch works in this environment" (`SKILL.md:358-410`), whose whole subject is the Copilot dispatch mechanism; and the review-trio table (`:685-689`), whose `agent_type` column changes meaning. Preserve their load-bearing content: self-contained dispatch prompts, the role→agent mapping, sync-vs-parallel guidance, the security review's diff-only rule and its autonomy override, and the distinction against otto's **own** in-product `task` tool (`{ description, prompt, subagent_type }` from `.otto/agents/`, depth-capped by `OTTO_AGENT_MAX_DEPTH`) — which remains a different thing from the dispatch mechanism.

- [x] **Step 3: DO NOT translate the GitHub Copilot PR reviewer.** `SKILL.md:671-678` and `:1024` reference the `copilot-pull-request-reviewer` bot added via `gh pr edit --add-reviewer`, and the rationalization row about its comments. That is a GitHub feature unrelated to the Copilot CLI and must survive verbatim. A blind `s/Copilot/Claude/` turns a working instruction into a broken one — this is the single most likely way to get the port wrong. Verify the login string still appears in the port.

- [x] **Step 4: Keep frontmatter byte-identical**; add the note; no `tools:`/`model:` key.

- [x] **Step 5: Generate both records, run the check to green, commit.**

---

### Task 4: Fault-inject every failure mode

**Files:** none permanently — every mutation applied to a throwaway copy of the two skill trees plus the script under `$(mktemp -d)`, never to the working tree.

- [x] **Step 1:** Canonical edited without re-porting → diff-mismatch failure naming the file and the fix commands.
- [x] **Step 2:** Port edited directly → the same failure from the other side.
- [x] **Step 3:** A record deleted → failure naming the regeneration command. An orphaned record → its own failure.
- [x] **Step 4:** A port's `description` drifted **with the record refreshed** → the dedicated frontmatter failure fires *alone*. This is the case that assertion exists for; if it only shows up as a diff mismatch, the separate assertion is not doing its job.
- [x] **Step 5:** A companion port missing → failure naming it (`agent-prompts.md` holds templates the workflow requires be pasted verbatim, so losing it loses the workflow).
- [x] **Step 6:** `.github/skills/` moved aside → the empty-set failure, not a green build.
- [x] **Step 7: Hostile content.** A symlinked port → rejected, not followed. A canonical named with a quote and a command substitution → "unsupported name", so nothing is interpolated into a message a maintainer might paste. A filename containing a newline → the same rejection, and `fail`'s escaping proven separately to collapse a crafted multi-line value into one `::error::` line with no forged workflow command at column 0.
- [x] **Step 8: False greens.** A nested canonical `.md` with a stale nested port → red. An extra port with no canonical → red. A zero-byte record → red. A verbatim-copy port → red. A `chmod 000` canonical (`diff` exit 2) → red, naming the unreadable file rather than reporting a recorded-divergence mismatch.
- [x] **Step 9: Frontmatter escapes and locale.** Block scalar on both sides, empty quoted value on both sides, `name:s: v`, an unterminated block, a duplicate key, and a BOM → each red with its own cause, *with the record refreshed first* so the frontmatter assertion is the only thing left to fire. Locale matrix (`C`, `en_US.UTF-8`, `fr_FR.UTF-8`) over a tree where one side lacks a trailing newline → all three green with the pin; red under `fr_FR.UTF-8` with the pin removed, which is the control.
- [x] **Step 10:** Confirm the tree is clean (`git status --porcelain` scoped to the mutated paths) and the check is back to `exit=0`.

---

### Task 5: Wire the check into CI

**Files:** Modify `.github/workflows/ci.yml`

- [x] **Step 1:** Point the existing `lint`-job step at `bash .github/scripts/check-claude-skill-ports.sh`, rename the step and the job's display name to match (the job is no longer just `fmt + clippy`). Keep it **immediately after** `- uses: actions/checkout@v4` — it needs only the checkout, so a failure surfaces in seconds instead of after a toolchain install and a full clippy build — with six-space indentation and the explanatory comment retained.

- [x] **Step 2:** Verify by eye that the step sits inside `lint`'s `steps:` at the right indentation. An error here breaks CI for every job at once.

- [x] **Step 3:** Commit.

- [x] **Step 4:** Add `permissions: contents: read` at workflow level. None of `lint`, `test`, `cross-vendor-gate` or `dist-plan` writes to the repository or calls the API, and without the block they inherit the default token scope — so on `pull_request` a fork's content is processed by a job holding a token it has no use for. Workflow level rather than job level because it is safe for all four; the style matches `release.yml`/`release-plz.yml`, which set `permissions:` where they need more.

---

### Task 6: Rewrite `CLAUDE.md`'s "Claude Code skills" section

**Files:** Modify `CLAUDE.md`

- [x] **Step 1:** The section currently documents the rejected stub design and is now wrong. It must state: the two-directory layout (canonical Copilot bodies vs. what Claude Code reads, both committed); that the repo's own workflow skills are **ported** — adapted copies — and why adaptation is required rather than a symlink or a verbatim copy; the **edit-canonical-then-re-port** workflow and that a port is never edited in place; the records and the CI check, with **both** local invocations named — verify, and the single `--update` regeneration command, so a contributor does not have to trigger a CI failure to learn the workflow; the caveat that **the check compares bytes, not meaning**, so "CI verifies the two stay in step" cannot be read as "CI verifies the port is correct"; that a new `.github/skills/` skill requires a port and a record in the same change, one per Markdown file at any depth; and the revised precedence rule — the canonical body is the source of truth for the workflow, the port is what Claude Code executes, so a discrepancy is a bug in the port, not a decision. Keep the existing personal-skills and GitHub-Issues-only paragraphs, and keep the final paragraph's narrowing of precedence to the personal-vs-repo case.

- [x] **Step 2:** Confirm every path the new section names exists, and the check is still green.

- [x] **Step 3:** Commit.

---

### Task 7: Full-tree verification

**Files:** none.

> **Use three-dot (merge-base) diffs.** `origin/main` moves while this branch is open, so a two-dot `git diff origin/main` reports trunk's commits as this branch's changes. (`git log origin/main..HEAD` is two-dot and correct — that form already means "commits on HEAD not on origin/main".)

- [x] **Step 1: Canonical bodies untouched.** `git diff --name-only origin/main...HEAD -- .github/skills/` must list **only** `claude-port/*.diff` additions. A canonical `.md` in that list means scope crept.
- [x] **Step 2: Changed-file set matches the File Structure table**, via `git diff --name-status origin/main...HEAD`. No Rust file, no `Cargo.toml`, no `Cargo.lock`, no `CHANGELOG.md`.
- [x] **Step 3: No stale Copilot mechanics in any port.** Grep the three ports for `agent_type`, `read_agent`, `ask_user`, `mode: "sync"`, `mode: "background"`, `` `sql` ``, `claude-haiku-4.5`, `reasoning_effort`, `todo_deps`. Note `agent_type` matches as a substring of `subagent_type` — Claude Code's own field — so read each hit rather than counting them. Every remaining hit must be a deliberate, explained reference; list them and why.
- [x] **Step 4: Public-interface record.** Nothing changed: no SPP wire type, no `ProviderHandler`/`ProviderClient` method, no tool MCP schema, no plugin ABI surface, no slash command, no env var, no on-disk transcript/keyring format. Non-Negotiable Rule 6 is not engaged; no `CHANGELOG.md` interface note is owed by this PR.
- [x] **Step 5: Rust invariants — vacuously satisfied, stated explicitly.** No `crates/otto/src/app.rs` or `tui.rs` edit, so the host-swap `RwLock` rule is not engaged; no streaming provider path, so the `ProgressDispatcher` forwarder-abort pattern is not engaged; no crate boundary or dependency edge changes. Verify from Step 2's file list, not by assertion.
- [x] **Step 6: Lint gates.** `cargo fmt --all --check` and `cargo clippy --workspace --all-targets` both clean. Unaffected by a docs/CI-only change, but the PR should not be where that is discovered. `cargo build`/`cargo test` are deliberately skipped: Step 2 establishes no Rust is in the diff, and CI's `test` job runs the full matrix on the PR regardless.
- [x] **Step 7: No attribution.**
  ```bash
  git log origin/main..HEAD --format='%s%n%b' | grep -niE 'co-authored-by|generated with|🤖' || echo "clean"
  git diff origin/main...HEAD | grep -niE 'co-authored-by|generated with|🤖' || echo "clean"
  ```
  Neither grep searches for "Claude": this change is *about* Claude Code, so the term appears legitimately in the diff and in commit subjects, and searching for it would guarantee a false positive on a clean branch. Expect the diff grep to hit the ported `otto-development` text where it **forbids** attribution (its own Non-Negotiable Rule 3) — read the hits rather than counting them.

- [x] **Step 8: Record what the PR body must state.**
  1. `Closes #128`.
  2. **The decision and why**, in a few sentences: neither of the issue's two options was taken verbatim — porting means adapting, so the ports are adapted copies; a symlink would have delivered Copilot-only instructions.
  3. **The honest cost**: this is a second copy of a ~103KB document. Say so, and say how it is contained (~8% divergence — 132 of 1,597 canonical lines — recorded exactly, CI-enforced, loud failure) and what would remove it (making the canonical bodies host-neutral — the natural follow-up, out of scope here).
  4. **The `reasoning_effort` gap**: approximated by model selection, so a dispatch the canonical wanted at `"xhigh"` gets whatever the session's model provides.

---

### Task 8: Release note and post-merge verification (notes only)

**Files:** none in this PR.

- [x] **Step 1: Release line.** Cut in a **dedicated release PR after this merges**, per `RELEASING.md` and the canonical Phase 4 step 12. This PR must **not** bump `workspace.package.version` and must **not** add the `CHANGELOG.md` section — that would collide with the release PR. The release PR bumps `workspace.package.version` and every internal `workspace.dependencies` version from `0.28.1` to `0.28.2`, adds the `## 0.28.2` section describing the ports and the parity check, tags `v0.28.2`, and pushes the tag so `release.yml` publishes the release. **Re-read `workspace.package.version` at cut time** — `origin/main` moves while this branch is open.

- [ ] **Step 2: Out-of-band verification (Phase 5, post-merge).** One success criterion cannot run on this branch:
  - `git clone` the merged trunk into a fresh directory — not this worktree, not the main checkout — so discovery is exercised as a new contributor would experience it.
  - Confirm all four skills are discovered: `otto-development`, `creating-github-issues`, `rust-engineer`, `tui-engineer`.
  - Confirm **execution**: trigger `otto-development` and check the run follows the ported workflow using Claude Code's own dispatch mechanics. The port is self-sufficient by design, so this is now a check that the adaptation is *correct*, not that a pointer gets followed.
  - This result is what justifies closing #128.

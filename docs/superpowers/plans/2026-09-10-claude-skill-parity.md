# claude-skill-parity Implementation Plan

> **For agentic workers:** Steps use checkbox (`- [ ]`) syntax for tracking. Implement task-by-task, in order.

**Goal:** Make this repo's own `.github/skills/` workflows — `otto-development` and `creating-github-issues` — discoverable and correctly executable from Claude Code, without duplicating their ~100KB of body text, and with a CI check that stops the two hosts from drifting apart.

**Architecture:** Each skill gains a small Claude-Code-native **adapter stub** at `.claude/skills/<name>/SKILL.md`. A stub carries frontmatter copied verbatim from the canonical skill (so Claude Code triggers on exactly what Copilot triggers on), a machine-checkable `> **Canonical body:**` pointer at the canonical `.github/skills/<name>/` file(s), and — for `otto-development` only — a Copilot→Claude Code host-mechanism translation table. The canonical bodies are untouched and remain the single source of truth. A new shell script, run by CI's existing `lint` job, asserts parity: every `.github/skills/` skill has a stub, every stub's frontmatter matches its canonical, and every path a stub points at exists.

**Tech Stack:** Markdown (skill bodies, `CLAUDE.md`), POSIX-ish bash (`.github/scripts/check-claude-skill-stubs.sh`, `set -euo pipefail`), GitHub Actions YAML (`.github/workflows/ci.yml`). No Rust, no crate, no dependency change.

**Spec:** `docs/superpowers/specs/2026-09-10-claude-skill-parity-design.md` — read it first. This plan implements it exactly.

**Release line:** `v0.28.1` (PATCH — repo tooling + contributor docs, no runtime behaviour change; current workspace version is `0.28.0`).

**Branch:** `docs/claude-skills-parity`

---

## File Structure

| File | Role |
| ---- | ---- |
| `.github/scripts/check-claude-skill-stubs.sh` (new) | The parity check. Iterates `.github/skills/*/`, asserting a stub exists, its `name`/`description` match the canonical, and every canonical path the stub names exists. First `.sh` in the repo. |
| `.gitattributes` (new) | `*.sh text eol=lf` — keeps the script's shebang LF on a `core.autocrlf=true` clone. |
| `.claude/skills/creating-github-issues/SKILL.md` (new) | Adapter stub: verbatim frontmatter + canonical pointer. No translation table (the canonical body is pure `gh`, host-agnostic). |
| `.claude/skills/otto-development/SKILL.md` (new) | Adapter stub: verbatim frontmatter + canonical pointer (both `SKILL.md` and `agent-prompts.md`) + the Copilot→Claude Code translation table. |
| `.github/workflows/ci.yml` (modify) | One step in the existing `lint` job: `bash .github/scripts/check-claude-skill-stubs.sh`. |
| `CLAUDE.md` (modify) | Rewrite the "Claude Code skills" section: two-directory layout, adapter-stub mechanism and rationale, the parity check, and the revised precedence rule. |

Neither `.github/skills/otto-development/SKILL.md` nor `.github/skills/creating-github-issues/SKILL.md` is edited. `.claude/skills/rust-engineer/` and `.claude/skills/tui-engineer/` are not edited — they are Claude-Code-native with no canonical counterpart, and the check does not iterate them.

**Test-first note.** This plan has no Rust and therefore no `cargo test` target. The executable specification here is the check script itself: Task 1 writes it and drives it red-then-green against a tree where the stubs do **not** yet exist, so the script is proven to fail for a real reason before the stubs are written to satisfy it. Tasks 2 and 3 then turn it green one skill at a time. Each of the three failure modes is fault-injected and asserted in Task 4.

---

### Task 1: The parity check script (written first, must fail)

**Files:**
- Create: `.github/scripts/check-claude-skill-stubs.sh`
- Create: `.gitattributes`

- [ ] **Step 1: Create `.gitattributes`**

Create `.gitattributes` at the repo root with exactly:

```
# Keep shell scripts LF-only so a core.autocrlf=true clone does not produce a
# CRLF shebang that fails with a confusing "bad interpreter" error.
*.sh text eol=lf
```

- [ ] **Step 2: Write the check script**

Create `.github/scripts/check-claude-skill-stubs.sh`. Requirements, all from the spec's "Drift prevention" section:

- Opens with `#!/usr/bin/env bash` and `set -euo pipefail`.
- Resolves the repo root from its own location (`git rev-parse --show-toplevel`, or `cd "$(dirname "$0")/../.."`) so it runs correctly from any working directory.
- A leading comment block explaining **why** the check exists (Claude Code only reads `.claude/skills/`; the canonical body lives in `.github/skills/`; these must not drift) — per invariant 9, comments explain why.
- For each directory `.github/skills/<name>/`:
  1. **Parity:** `.claude/skills/<name>/SKILL.md` must exist. On failure print the skill name and the exact path to create.
  2. **Frontmatter match:** extract `name:` and `description:` from both `SKILL.md` files and compare. Extraction: first matching line in the file, everything after the first `: `, strip a trailing `\r`, then strip one layer of wrapping `"` or `'` if present. Fail loudly (not silently) if either key is missing or its value is empty on either side. On mismatch print both values, labelled by path.
  3. **Pointer targets:** every path named on the stub's `> **Canonical body:**` line must exist. Extract the backtick-quoted paths from that line (there may be more than one — `otto-development` names two) and test each. Fail if the line is absent, or names no path, or names a path that does not exist.
- Accumulates failures and reports **all** of them before exiting `1`, rather than dying on the first — a contributor fixing two stubs should not need two CI runs.
- Prints a one-line success summary naming how many skills were checked when everything passes, and exits `0`.
- Does not use `grep -P`, `mapfile`, `readarray`, or GNU-only `sed -i` forms — the CI runner is `ubuntu-latest`, but keeping to portable constructs means it also runs on a contributor's macOS box.

- [ ] **Step 3: Run it; confirm it fails for the right reason**

Run: `bash .github/scripts/check-claude-skill-stubs.sh; echo "exit=$?"`

Expected: `exit=1`, with two parity failures — one naming `.claude/skills/otto-development/SKILL.md` and one naming `.claude/skills/creating-github-issues/SKILL.md` as missing. It must **not** fail with a bash syntax error, an unbound variable, or a message about `rust-engineer`/`tui-engineer` (which it does not iterate). If the output mentions either of those two skills, the iteration source is wrong — fix it before continuing.

- [ ] **Step 4: Set the executable bit and commit**

```bash
chmod +x .github/scripts/check-claude-skill-stubs.sh
git add .gitattributes .github/scripts/check-claude-skill-stubs.sh
git commit -m "ci: add Claude Code skill-stub parity check"
```

Confirm the mode landed as `100755`: `git show --stat --format= HEAD | head` / `git ls-files -s .github/scripts/check-claude-skill-stubs.sh`. CI invokes it via `bash …` so the bit is not load-bearing, but it should be right.

---

### Task 2: `creating-github-issues` adapter stub (the simple case first)

**Files:**
- Create: `.claude/skills/creating-github-issues/SKILL.md`

- [ ] **Step 1: Read the canonical frontmatter verbatim**

Run: `sed -n '1,5p' .github/skills/creating-github-issues/SKILL.md`

Copy the `name:` and `description:` values **byte-for-byte**. Do not re-wrap, re-punctuate, shorten, or "improve" the description — the parity check compares it exactly, and a rewrite would make Claude Code and Copilot trigger differently on the same request (the exact drift this change exists to prevent).

- [ ] **Step 2: Write the stub**

Create `.claude/skills/creating-github-issues/SKILL.md`:

- YAML frontmatter with `name` and `description` only. No `tools:`, no `model:` (spec: enumerating a `tools:` allowlist could only under-grant; a `model:` pin would cap the run).
- A `> **Canonical body:** `.github/skills/creating-github-issues/SKILL.md`` line, formatted so the check's extractor finds the backtick-quoted path.
- One short paragraph: this stub exists because Claude Code discovers project skills only under `.claude/skills/`, while the canonical body is maintained once in `.github/skills/` for the Copilot CLI. **Read the canonical body now and follow it** — it is the authority on this repo's GitHub-Issues-only conventions (labels, duplicate check, unassigned default, no spec/plan at creation time).
- An explicit note that **no host-mechanism translation is needed**: the canonical body is plain `gh issue` / `gh label` invocations, which work identically from Claude Code.
- No copy of the canonical body's rules. If a rule is worth stating, it is worth stating once, in the canonical file.

- [ ] **Step 3: Run the check; confirm one failure remains**

Run: `bash .github/scripts/check-claude-skill-stubs.sh; echo "exit=$?"`

Expected: `exit=1`, and now exactly **one** parity failure, naming `.claude/skills/otto-development/SKILL.md`. `creating-github-issues` must no longer appear in the output. If it still appears with a frontmatter mismatch, the description was not copied byte-for-byte — fix the stub, not the check.

- [ ] **Step 4: Commit**

```bash
git add .claude/skills/creating-github-issues/SKILL.md
git commit -m "docs: add creating-github-issues Claude Code adapter stub"
```

---

### Task 3: `otto-development` adapter stub (with the translation table)

**Files:**
- Create: `.claude/skills/otto-development/SKILL.md`

- [ ] **Step 1: Read the canonical frontmatter verbatim**

Run: `sed -n '1,5p' .github/skills/otto-development/SKILL.md`

Same rule as Task 2 step 1 — byte-for-byte, including the em-dashes and the trailing "For other repositories use general-development." sentence.

- [ ] **Step 2: Write the stub**

Create `.claude/skills/otto-development/SKILL.md`:

- YAML frontmatter: `name` + `description` verbatim, nothing else.
- A `> **Canonical body:**` line naming **both** `` `.github/skills/otto-development/SKILL.md` `` and `` `.github/skills/otto-development/agent-prompts.md` `` in backticks — the check validates every path on that line, and the second file holds the dispatch prompt templates the canonical body requires be pasted verbatim.
- A "Read this first" instruction: load the canonical `SKILL.md` before acting, and `agent-prompts.md` when a phase calls for a dispatch template. State plainly that the stub is **not** a summary and must not be acted on alone — every Non-Negotiable Rule, phase gate, fix-loop cap and review requirement lives in the canonical body.
- **The host-mechanism translation table**, exactly as specified in the spec's Approach section — `task`→`Agent`, each `agent_type` value→its Claude Code equivalent, `mode: "sync"`/`"background"`→single vs. concurrent `Agent` calls, `read_agent`→task-completion notification or `SendMessage`, the `sql` `todos` table→`TodoWrite`, `ask_user`→`AskUserQuestion` (still overridden for autonomy per the canonical body). Preceded by one sentence explaining why the table is needed: the canonical body is written for the orchestrating Copilot CLI and says so at its own `SKILL.md:360-361`, naming tools Claude Code does not have.
- A note that the table maps only to **built-in** Claude Code agent types and skills, and that a contributor whose `~/.claude/agents/` provides specialised agents (`rust-pro`, `architect-reviewer`, `code-reviewer`, `security-auditor`) may use those for the mandatory review trio as an **optional upgrade** — never a requirement, so the mapping still holds in a fresh clone.
- An explicit carry-over of the two rules most likely to be lost in translation, because they are host-mechanism-shaped rather than workflow-shaped: **dispatch prompts must be fully self-contained** (paste the actual spec/plan/task text inline; a Claude Code subagent has no access to this session's context), and the **security review must receive only the PR diff** — never the spec, plan, brief, PR body, or implementer report (canonical Non-Negotiable Rule 5). Both are stated as pointers to the canonical rule, not as replacements for it.
- Nothing else. No phase list, no rule summary, no convention table — those would be a partial copy, which is what this design rejects.

- [ ] **Step 3: Run the check; confirm it passes**

Run: `bash .github/scripts/check-claude-skill-stubs.sh; echo "exit=$?"`

Expected: `exit=0` and the success summary naming 2 skills checked.

- [ ] **Step 4: Commit**

```bash
git add .claude/skills/otto-development/SKILL.md
git commit -m "docs: add otto-development Claude Code adapter stub"
```

---

### Task 4: Fault-inject each failure mode

**Files:** none modified permanently — this task proves the check actually catches what the spec claims. Every mutation is reverted before the task ends.

- [ ] **Step 1: Missing stub**

```bash
mv .claude/skills/creating-github-issues/SKILL.md /tmp/stub-backup.md
bash .github/scripts/check-claude-skill-stubs.sh; echo "exit=$?"
mv /tmp/stub-backup.md .claude/skills/creating-github-issues/SKILL.md
```

Expected: `exit=1`, output names `creating-github-issues` and the missing path.

- [ ] **Step 2: Mismatched frontmatter**

Temporarily append a word to the `description:` in `.claude/skills/creating-github-issues/SKILL.md`, run the check, then restore with `git checkout -- .claude/skills/creating-github-issues/SKILL.md`.

Expected: `exit=1`, output prints both description values labelled by path.

- [ ] **Step 3: Dangling pointer**

Temporarily edit the `> **Canonical body:**` line in `.claude/skills/otto-development/SKILL.md` to name `` `.github/skills/otto-development/agent-prompts-typo.md` ``, run the check, then restore with `git checkout --`.

Expected: `exit=1`, output names the non-existent path. **This step specifically proves the `agent-prompts.md` slot is validated, not just the first path on the line** — if the check passes here, the extractor is only reading one path and must be fixed (then re-run Tasks 1–3's checks).

- [ ] **Step 4: Confirm the tree is clean and the check is green**

```bash
git status --porcelain     # must be empty
bash .github/scripts/check-claude-skill-stubs.sh; echo "exit=$?"    # must be 0
```

No commit for this task — it produces no lasting change. If `git status` is not empty, a fault injection was not reverted; revert it before continuing.

---

### Task 5: Wire the check into CI

**Files:**
- Modify: `.github/workflows/ci.yml` — the existing `lint` job.

- [ ] **Step 1: Add the step**

In `.github/workflows/ci.yml`, in the `lint` job, add a step invoking the script. Place it **before** the `rustfmt` step: it needs no toolchain and no system deps, so a stub-parity failure surfaces in seconds rather than after a full clippy build.

```yaml
      - name: Claude Code skill-stub parity
        run: bash .github/scripts/check-claude-skill-stubs.sh
```

Invoke via `bash …` (not `./…`) so the git executable bit is not load-bearing, per the spec.

- [ ] **Step 2: Verify the YAML parses and the step is in the right job**

Run: `sed -n '/^  lint:/,/^  test:/p' .github/workflows/ci.yml`

Confirm by eye: the new step sits inside `lint`'s `steps:` list at the same indentation as `- name: rustfmt`, after `actions/checkout@v4` (the script needs the checkout) and before `rustfmt`. Indentation errors here are the one way this task can break CI for every job at once.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: run the skill-stub parity check in the lint job"
```

---

### Task 6: Rewrite `CLAUDE.md`'s "Claude Code skills" section

**Files:**
- Modify: `CLAUDE.md` — the `## Claude Code skills` section (currently four paragraphs at `CLAUDE.md:87-95`).

- [ ] **Step 1: Rewrite the section**

Keep what is still true and add what this change makes true. The section must state:

- **The two-directory layout.** `.github/skills/` holds the canonical, Copilot-CLI-format bodies of this repo's own workflow skills (`otto-development`, `creating-github-issues`). `.claude/skills/` is what Claude Code discovers, and holds two kinds of entry: Claude-Code-native skills with no canonical counterpart (`rust-engineer`, `tui-engineer`) and **adapter stubs** that delegate to a canonical body. Both directories are committed; neither is gitignored.
- **The adapter-stub mechanism and why it is not a symlink or a copy.** One canonical body, no duplication; a symlink cannot carry the host-mechanism translation the canonical body needs (it is written for the Copilot CLI and says so) and would need `core.symlinks=true` on Windows; a copy would duplicate ~100KB of the repo's most-edited process document.
- **The rule for adding a new skill.** A new skill under `.github/skills/` **must** come with a `.claude/skills/<name>/SKILL.md` adapter stub in the same change — `.github/scripts/check-claude-skill-stubs.sh` (run by CI's `lint` job) fails the build otherwise, and also fails if a stub's `name`/`description` drift from the canonical or if a stub points at a path that no longer exists. Note that the script is runnable locally with `bash .github/scripts/check-claude-skill-stubs.sh`.
- **The revised precedence rule.** Replace the old blanket "`.github/skills/` governs" sentence with the distinction the layout now encodes: for a repo-authored skill, the **canonical body governs the workflow** and the **stub governs only host-mechanism translation** — a stub never overrides a rule, it only says how to perform it with Claude Code's tools. For a *personal* skill from `~/.claude/skills/` that overlaps a repo skill in purpose, this repo's own skill still governs, unchanged.
- **Unchanged, keep it:** personal/generic Claude Code skills stay machine-local in `~/.claude/skills/` and are never committed here; any tracker-related skill defers to GitHub Issues via `gh`, never JIRA.

- [ ] **Step 2: Verify the cross-references resolve**

Run: `bash .github/scripts/check-claude-skill-stubs.sh; echo "exit=$?"` (still `0`), and confirm every path the new section names exists:

```bash
ls .github/skills/ .claude/skills/ .github/scripts/check-claude-skill-stubs.sh
```

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: document the Claude Code adapter-stub layout in CLAUDE.md"
```

---

### Task 7: Full-tree verification and final commit hygiene

**Files:** none.

- [ ] **Step 1: Confirm the canonical bodies are untouched**

```bash
git diff --stat origin/main -- .github/skills/
```

Expected: **empty**. The spec puts editing the canonical bodies out of scope; a non-empty diff here means scope crept.

- [ ] **Step 2: Confirm the changed-file set matches the plan's File Structure exactly**

```bash
git diff --name-status origin/main
```

Expected exactly: `A .claude/skills/creating-github-issues/SKILL.md`, `A .claude/skills/otto-development/SKILL.md`, `A .gitattributes`, `A .github/scripts/check-claude-skill-stubs.sh`, `M .github/workflows/ci.yml`, `M CLAUDE.md`, plus the two `A docs/superpowers/{specs,plans}/…` design docs. Nothing else — no Rust file, no `Cargo.toml`, no `Cargo.lock`.

- [ ] **Step 3: Public-interface record**

No public interface changed — no SPP wire type, no `ProviderHandler`/`ProviderClient` method, no tool MCP schema, no plugin ABI surface, no slash command, no env var, no on-disk transcript/keyring format. Non-Negotiable Rule 6 is not engaged and no `CHANGELOG.md` interface note is needed from this PR. (The `v0.28.1` entry itself is written in the release PR — see Task 8.)

- [ ] **Step 4: Rust invariants — confirmed not applicable**

This change touches no Rust. Explicitly: no `crates/otto/src/app.rs` or `tui.rs` edit, so the host-swap `RwLock` rule is not engaged; no streaming provider path, so the `rmcp` `ProgressDispatcher` forwarder-abort pattern is not engaged; no crate boundary or dependency edge changes. Verify with the Step 2 file list rather than by assertion.

- [ ] **Step 5: Lint gates still pass**

`cargo fmt --all --check` and `cargo clippy --workspace --all-targets` are unaffected by a docs/CI-only change, but run them once so the PR is not the place that discovers otherwise:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
```

Expected: both clean. (Requires `libdbus-1-dev`, `libfontconfig1-dev`, `pkg-config` on Linux.)

- [ ] **Step 6: No attribution anywhere**

```bash
git log origin/main..HEAD --format='%s%n%b' | grep -niE 'co-authored-by|generated with|claude|🤖' || echo "clean"
git diff origin/main | grep -niE 'co-authored-by|generated with|🤖' || echo "clean"
```

Expected: `clean` from both. Note the diff legitimately contains the word "Claude" throughout (it is a change *about* Claude Code) — the second grep deliberately omits that term; the first must find no AI-credit trailer in any commit message.

- [ ] **Step 7: Format and commit**

Nothing to `cargo fmt` (no Rust changed). If any step above produced a fix, commit it as `<scope>: <subject>` with no attribution. Otherwise the branch is ready for the PR.

---

### Task 8: Release note (this PR does not bump the version)

**Files:** none in this PR.

- [ ] **Step 1: Record the release line**

The release is cut in a **dedicated release PR after this PR merges**, per the canonical body's Phase 4 step 12 and `RELEASING.md`. This PR must **not** bump `workspace.package.version` and must **not** add the `0.28.1` `CHANGELOG.md` section — doing so here would collide with the release PR.

The release PR will: bump `workspace.package.version` and every internal `workspace.dependencies` version in the root `Cargo.toml` from `0.28.0` to `0.28.1`, add a `## 0.28.1` `CHANGELOG.md` section describing the Claude Code adapter stubs and the parity check (Added/Changed — contributor-facing tooling, no runtime behaviour change), tag `v0.28.1`, and push the tag so `release.yml` publishes the GitHub Release with artifacts.

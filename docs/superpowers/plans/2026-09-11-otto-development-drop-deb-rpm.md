# otto-development-drop-deb-rpm Implementation Plan

**Goal:** Remove the seven stale `.deb`/`.rpm`/`package-linux.yml` references from
`.github/skills/otto-development/SKILL.md` (which cause a false Phase 5 escalation on every release,
per `savvagent/otto#141`), mirror the identical edits into the Claude Code port at
`.claude/skills/otto-development/SKILL.md`, and regenerate the port-parity record so
`bash .github/scripts/check-claude-skill-ports.sh` exits 0.

**Architecture:** A single logical edit applied to two files: the canonical Copilot-CLI skill body
and its Claude Code port. This section of the skill (Phase 4 step 12, Phase 5, and the release-
conventions table row) carries no Claude-Code-specific adaptation — canonical and port read
identical prose here, offset only by line numbers — so the port edit is a verbatim copy of the
canonical edit, not a re-adaptation. No Rust code, no crate, no workflow file changes.

**Tech Stack:** Markdown only. `bash .github/scripts/check-claude-skill-ports.sh` (existing,
unmodified) regenerates the expected-diff record.

**Spec:** `docs/superpowers/specs/2026-09-11-otto-development-drop-deb-rpm-design.md` — read it
first. This plan implements it exactly.

**Release line:** next PATCH after whatever `workspace.package.version` reads at cut time (currently
`0.29.0` as of branch creation — re-read at cut time per the release step below, since `origin/main`
moves while this branch is open). Docs-only, no runtime behavior change, no public-interface change
→ PATCH per `CHANGELOG.md`'s convention.

**Branch:** `docs/otto-development-drop-deb-rpm`

## File Map

**New files**
- None.

**Modified files**
- `.github/skills/otto-development/SKILL.md` — six edits removing `.deb`/`.rpm`/`package-linux.yml`
  references (canonical).
- `.claude/skills/otto-development/SKILL.md` — the identical six edits mirrored (port).
- `.github/skills/otto-development/claude-port/SKILL.md.diff` — regenerated via `--update`.
- `CHANGELOG.md` — an `Added`/`Fixed` entry under `## [Unreleased]` (added in the dedicated release
  PR per Phase 4 step 12, not in this PR — see Task 2).

## Task 1: Remove stale `.deb`/`.rpm`/`package-linux.yml` references

**Files:**
- Modify: `.github/skills/otto-development/SKILL.md`
- Modify: `.claude/skills/otto-development/SKILL.md`
- Regenerate: `.github/skills/otto-development/claude-port/SKILL.md.diff`

No Rust, so no `cargo test` step in this task. The verification step is
`bash .github/scripts/check-claude-skill-ports.sh`, run before and after.

- [ ] **Step 1: Confirm the current failure state.** From the worktree root:
  ```bash
  grep -niE '\.deb|\.rpm|package-linux' .github/skills/otto-development/SKILL.md
  ```
  Expect 9 matching lines (the 7 references from the issue, two of which are 2-line spans:
  `798-800` and `881-882`). Record the exact current line numbers — the issue notes they "may have
  drifted."

- [ ] **Step 2: Edit the canonical file**, `.github/skills/otto-development/SKILL.md`, six edits:

  1. **Release conventions table row** (Release row, currently ending "...
     `.github/workflows/package-linux.yml` attaches `.deb`/`.rpm` automatically afterward."):
     replace the clause after "...platform binaries/installers" with a sentence stating otto does
     not ship `.deb`/`.rpm` packages, matching `RELEASING.md:79-80`'s wording:
     `otto does not ship .deb/.rpm packages — Linux installs go through the shell installer or the
     platform tarball.`

  2. **Phase 4 step 12, item 6** (the `.deb`/`.rpm` auto-attach item with the broken
     `gh workflow run "Package (deb/rpm)"` re-run command): delete the entire numbered item.

  3. **Renumber** the following item (previously item 7, "Verify the release") to item 6.

  4. **That item's text** ("...confirm all expected platform archives/installers plus `.deb`/`.rpm`
     are attached."): drop the `.deb`/`.rpm` clause → "...confirm all expected platform
     archives/installers are attached."

  5. **Phase 5 intro paragraph** ("...published as GitHub Release assets (plus `.deb`/`.rpm`
     packages) — there is..."): drop the parenthetical → "...published as GitHub Release assets —
     there is...".

  6. **Step 15 "Packaging/dist config" out-of-band item** (currently: "if
     `[workspace.metadata.dist]` or `.github/workflows/release*.yml` / `package-linux.yml` changed"):
     narrow to name only `[workspace.metadata.dist]` and `release.yml` → "if
     `[workspace.metadata.dist]` or `.github/workflows/release.yml` changed".

  7. **Step 16 release verification** (currently: "Confirm the expected platform
     archives/installers are attached, plus `.deb`/`.rpm` once `package-linux.yml` finishes. If any
     platform artifact is missing or the workflow failed, treat it as a Phase 5 failure — do not
     close the loop on a partially-published release."): replace with verification against the real
     cargo-dist asset set. New text:
     ```
     Confirm the expected 14-asset cargo-dist release set is attached — the same set v0.29.0,
     v0.28.1 and v0.28.0 each carry: `dist-manifest.json`, `sha256.sum`, the source tarball plus its
     `.sha256`, both installer scripts (`otto-installer.sh`, `otto-installer.ps1`), and the four
     platform archives plus their `.sha256` files (`otto-aarch64-apple-darwin.tar.gz`,
     `otto-aarch64-unknown-linux-gnu.tar.gz`, `otto-x86_64-unknown-linux-gnu.tar.gz`,
     `otto-x86_64-pc-windows-msvc.zip`). If any of these is missing or the release build failed,
     treat it as a Phase 5 failure — do not close the loop on a partially-published release.
     ```

  Do not touch anything else in the file — the divergence this task produces must be exactly these
  six edits, nothing stylistic.

- [ ] **Step 3: Verify the canonical edit is complete.**
  ```bash
  grep -niE '\.deb|\.rpm|package-linux' .github/skills/otto-development/SKILL.md
  ```
  Must return nothing (exit 1).

- [ ] **Step 4: Mirror the identical six edits into `.claude/skills/otto-development/SKILL.md`.**
  Find the corresponding text (same prose, offset by the port's earlier adaptations — expect the
  release-conventions row around line 214, item 6 around line 836, etc., but locate by content, not
  assumed line number) and apply the same replacements verbatim — this section carries no
  Claude-Code-specific adaptation, so do not introduce any Claude-Code-specific wording here that
  the canonical doesn't have.

- [ ] **Step 5: Verify the port edit is complete.**
  ```bash
  grep -niE '\.deb|\.rpm|package-linux' .claude/skills/otto-development/SKILL.md
  ```
  Must return nothing (exit 1).

- [ ] **Step 6: Confirm `agent-prompts.md` needs no edit** (per the spec's Assumptions):
  ```bash
  grep -niE '\.deb|\.rpm|package-linux' .github/skills/otto-development/agent-prompts.md \
    .claude/skills/otto-development/agent-prompts.md
  ```
  Must return nothing.

- [ ] **Step 7: Cross-check `RELEASING.md` and `CLAUDE.md`** (expected no-op per the issue):
  ```bash
  grep -niE '\.deb|\.rpm|package-linux' RELEASING.md CLAUDE.md
  ```
  Expect `RELEASING.md:79-80`'s existing correct statement, plus (harmlessly) two `CLAUDE.md` hits
  that are substring matches on the word "debugging", not real `.deb`/`.rpm`/`package-linux`
  references — read each hit rather than asserting the grep is silent. If `CLAUDE.md` has any
  *genuine* `.deb`/`.rpm`/packaging claim, stop and report it — the plan did not anticipate that and
  it needs a decision, not a silent edit.

- [ ] **Step 8: Regenerate the port-parity record.**
  ```bash
  bash .github/scripts/check-claude-skill-ports.sh --update
  ```
  Confirm it reports rewriting `.github/skills/otto-development/claude-port/SKILL.md.diff` (and no
  other file — `agent-prompts.md`'s record should be unchanged since Step 6 found nothing to edit
  there).

- [ ] **Step 9: Verify the check passes.**
  ```bash
  bash .github/scripts/check-claude-skill-ports.sh
  ```
  Must exit 0.

- [ ] **Step 10: Eyeball the regenerated record for stale text.**
  ```bash
  grep -niE '\.deb|\.rpm|package-linux' .github/skills/otto-development/claude-port/SKILL.md.diff
  ```
  Any hit here must appear only as part of a `-`/`+` line pair proving the *same* old/new text on
  both sides (i.e., no divergence introduced by this change) — not as surviving stale content. Given
  Steps 3 and 5 already confirmed both files are clean, this should return nothing at all.

- [ ] **Step 11: Public-interface note.** No SPP wire type, tool schema, plugin ABI, slash command,
  env var, or on-disk format touched — Non-Negotiable Rule 6 is not engaged. No `CHANGELOG.md`
  interface note owed by this task.

- [ ] **Step 12: Host-swap / streaming invariants — vacuously satisfied.** No `crates/otto/src/app.rs`
  or `tui.rs` touched, so the host-swap `RwLock` rule is not engaged. No streaming provider path
  touched, so the `ProgressDispatcher` forwarder-abort pattern is not engaged.

- [ ] **Step 13: Format and commit.** No Rust changed, so `cargo fmt --all` is a no-op here but run
  it anyway for consistency with house style:
  ```bash
  cargo fmt --all
  git add .github/skills/otto-development/SKILL.md \
          .claude/skills/otto-development/SKILL.md \
          .github/skills/otto-development/claude-port/SKILL.md.diff
  git commit -m "docs: drop stale .deb/.rpm verification from otto-development"
  ```

## Task 2: Cut the release (notes only — performed per Phase 4 step 12, not in this PR)

**Files:** none in this PR.

- [ ] **Step 1:** This PR does **not** bump `workspace.package.version` and does **not** add a
  `CHANGELOG.md` section — that happens in the dedicated release PR after this merges, per
  Non-Negotiable Rule 8 / Phase 4 step 12. Re-read `workspace.package.version` at cut time (it may
  have moved past `0.29.0` if another PR merged first) and cut the next PATCH from whatever it then
  reads. The `CHANGELOG.md` entry: a `Fixed` bullet describing that `otto-development` no longer
  requires verifying `.deb`/`.rpm` artifacts or a `package-linux.yml` run that no longer exists.

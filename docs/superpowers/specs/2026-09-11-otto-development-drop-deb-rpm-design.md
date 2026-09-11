# Drop stale `.deb`/`.rpm` verification from `otto-development` — design

Date: 2026-09-11
Status: IMPLEMENTED
Source: savvagent/otto#141
Related: `01bb3f1` ("release: drop the .deb/.rpm Linux packages (#109)"), `RELEASING.md:78-80`

## Problem

`.github/skills/otto-development/SKILL.md` still tells a release run to verify `.deb`/`.rpm`
artifacts and a `.github/workflows/package-linux.yml` workflow, both of which were deliberately
removed in `01bb3f1`. `.github/workflows/package-linux.yml` no longer exists in the repo, and no
workflow named `Package (deb/rpm)` exists for `gh workflow run` to target.

This is not a cosmetic staleness issue. Phase 5 step 16 states: "Confirm the expected platform
archives/installers are attached, plus `.deb`/`.rpm` once `package-linux.yml` finishes. If any
platform artifact is missing or the workflow failed, treat it as a Phase 5 failure — do not close
the loop on a partially-published release." Stop & Escalate condition 11 makes a Phase 5 failure a
halt. Since the `.deb`/`.rpm` artifacts and the workflow that built them no longer exist, this
condition is permanently true: every otherwise-successful release triggers a false escalation.

`RELEASING.md:78-80` already states the correct, current behavior: "Confirm all expected platform
archives and installers are attached. otto does not ship `.deb`/`.rpm` packages — Linux installs go
through the shell installer or the platform tarball." Per `otto-development`'s own precedence rule
("when [`CLAUDE.md`] disagrees with the current code ... the code wins" — `SKILL.md`'s "Why this
shape" section, which extends the same reasoning to any of this repo's own docs lagging the code
they describe), the skill is the document that needs to change here, not the other way round: the
actual release pipeline (code) and `RELEASING.md` already agree that `.deb`/`.rpm` packages aren't
shipped, and the skill document is the one that's stale.

## Approach

Remove all `.deb`/`.rpm`/`package-linux.yml` references from the canonical
`.github/skills/otto-development/SKILL.md`, mirror the same edits into the Claude Code port at
`.claude/skills/otto-development/SKILL.md` (this section of the skill carries no Claude-Code-specific
adaptation — canonical and port are byte-identical prose here, just offset by the ~38 lines the
earlier "How dispatch works" adaptation adds — so the edit is a verbatim copy, not a re-adaptation),
then regenerate the `claude-port/SKILL.md.diff` expected-diff record with
`bash .github/scripts/check-claude-skill-ports.sh --update`.

Seven references, addressed as six edits (two are adjacent lines in the same sentence):

1. **Release conventions table row** (canonical `SKILL.md:209`, port `:214`) — currently claims
   `package-linux.yml` "attaches `.deb`/`.rpm` automatically afterward." Replace with a statement
   that otto does not ship `.deb`/`.rpm` packages, matching `RELEASING.md`'s wording so the two
   documents agree verbatim rather than just in substance.
2. **Phase 4 step 12, item 6** (canonical `:798-800`, port `:836-838`) — the whole item describes
   automatic `.deb`/`.rpm` attachment and gives a manual re-run command
   (`gh workflow run "Package (deb/rpm)" -f tag=vX.Y.Z`) for a workflow that has never existed under
   that name (confirmed: `.github/workflows/package-linux.yml` is absent from the tree and no such
   name appears in `gh workflow list`). Delete the item outright — not correct it — and renumber the
   following item (current item 7, "Verify the release") down to item 6.
3. **Phase 4 step 12, item 7 / renumbered 6** (canonical `:801-802`, port `:839-840`) — "confirm all
   expected platform archives/installers plus `.deb`/`.rpm` are attached." Drop the `.deb`/`.rpm`
   clause; the sentence still needs to say what to confirm, so it becomes "confirm all expected
   platform archives/installers are attached."
4. **Phase 5 intro paragraph** (canonical `:827`, port `:865`) — "published as GitHub Release assets
   (plus `.deb`/`.rpm` packages)". Drop the parenthetical.
5. **Step 15 out-of-band checklist, "Packaging/dist config" item** (canonical `:866-868`, port
   `:904-906`) — currently triggers on `[workspace.metadata.dist]` OR
   `.github/workflows/release*.yml` OR `package-linux.yml` changing. Narrow the trigger to
   `[workspace.metadata.dist]` or `release.yml` only, per the acceptance criteria's explicit
   instruction that this item name only those two surfaces.
6. **Step 16 release verification** (canonical `:881-883`, port `:919-921`) — "Confirm the expected
   platform archives/installers are attached, plus `.deb`/`.rpm` once `package-linux.yml` finishes.
   If any platform artifact is missing or the workflow failed, treat it as a Phase 5 failure." Replace
   with verification against the actual cargo-dist release set: the 14 assets that v0.29.0, v0.28.1
   and v0.28.0 each carry (four platform archives + their `.sha256` files, both installer scripts,
   `dist-manifest.json`, `sha256.sum`, and the source tarball + its `.sha256`). Keep the
   "missing artifact → Phase 5 failure" enforcement — that half of the sentence is correct and
   load-bearing (Stop & Escalate condition 11 still needs *something* to point at) — just point it at
   the real asset list instead of a deleted workflow.

None of Non-Negotiable Rules 1-9 or the Load-Bearing Invariants are touched: this changes only
release-verification prose, not the workflow steps that produce a release, the review trio, the
worktree/PR discipline, or any Rust code.

## Scope

**In:**
- The six edits above in `.github/skills/otto-development/SKILL.md` (canonical).
- The identical six edits mirrored verbatim into `.claude/skills/otto-development/SKILL.md` (port).
- Regenerating `.github/skills/otto-development/claude-port/SKILL.md.diff` via
  `bash .github/scripts/check-claude-skill-ports.sh --update`.
- Cross-checking `RELEASING.md` and `CLAUDE.md` for any remaining `.deb`/`.rpm` claim (expected to be
  a no-op per the issue — `RELEASING.md:79` is already correct, and `CLAUDE.md` carries no genuine
  `.deb`/`.rpm`/`package-linux` claim; a raw `grep -niE 'deb|rpm|package-linux' CLAUDE.md` does return
  two hits, but both are substring matches on the word "debugging" — not real references, so the
  check must read the hits rather than assert the grep is silent).

**Out:**
- `agent-prompts.md` — no `.deb`/`.rpm`/`package-linux.yml` reference exists there (verified by
  grep); it needs no edit and no port mirroring for this change.
- The other three known defects in this skill body (#133 input hardening, #137 the lost read-only
  guarantee, #138 the release-batching rule-vs-practice conflict) — distinct issues, not touched
  here.
- Any change to `.github/workflows/`, `Cargo.toml`, or actual release mechanics. The workflow that
  built `.deb`/`.rpm` packages was already removed in `01bb3f1`; this change only stops the skill
  document from asking an agent to verify output that workflow no longer produces.
- Reviving Linux `.deb`/`.rpm` packaging. The issue notes the last four `package-linux.yml` runs
  failed before removal — out of scope, and not this change's concern either way.

## Public-interface changes

**None.** This is a `.md` skill-document edit plus a generated `.diff` record. No SPP wire type, no
`ProviderHandler`/`ProviderClient` method, no tool MCP schema, no plugin ABI surface, no slash
command, no env var, no on-disk transcript/keyring format, and no Rust code is touched
(Non-Negotiable Rule 6 is not engaged).

## Assumptions

- **The release-conventions row (edit 1) should match `RELEASING.md`'s wording exactly**, not just
  convey the same fact in different words — the two documents describing the same non-fact
  identically reduces the chance either drifts back out of sync later.
- **Item 6 (the broken manual re-run command) is deleted, not merged into another item.** The issue
  is explicit that there is "nothing to trigger" — folding a corrected version of it into the
  "Verify the release" item would just reintroduce a `.deb`/`.rpm` mention under a different heading.
- **The renumbering in step 12 (item 7 → 6) is mechanical** — no other item in the numbered list
  references item 6 or item 7 by number, so renumbering has no other ripple (verified by reading the
  full step 12 list in both files).
- **Step 16's "Confirm N assets" replacement enumerates the 14-asset list literally** rather than
  pointing at `RELEASING.md` by reference, matching how the rest of Phase 5 (step 15) already
  enumerates concrete commands and file surfaces inline rather than by pointer — the skill is meant
  to be self-contained per-step.
- **`agent-prompts.md` needs no edit.** Confirmed via
  `grep -niE '\.deb|\.rpm|package-linux' .github/skills/otto-development/agent-prompts.md
  .claude/skills/otto-development/agent-prompts.md` returning nothing in either file.
- **This is fast-path-adjacent but not fast-pathed**, per the task instructions explicitly calling
  for the full spec → plan → implement → PR → review → merge → release lifecycle; the spec and plan
  are kept intentionally small to match the actual size of a six-edit, two-file, no-code change.

## Goal & Success Criteria

An agent running `otto-development` to cut a release no longer escalates over `.deb`/`.rpm`
artifacts or a `package-linux.yml` run, and instead verifies the release set that `cargo-dist`
actually produces.

- All seven stale `.deb`/`.rpm`/`package-linux.yml` references are gone from the canonical
  `SKILL.md` (verified by `grep -niE '\.deb|\.rpm|package-linux' .github/skills/otto-development/SKILL.md`
  returning nothing).
- The port carries the identical edits, with the check script's regenerated record proving the
  divergence is unchanged in kind (still only the host-mechanism adaptation, no new drift).
- Step 16 verifies the real 14-asset cargo-dist set.
- The Phase 5 "Packaging/dist config" out-of-band item names only `[workspace.metadata.dist]` and
  `release.yml`.
- `bash .github/scripts/check-claude-skill-ports.sh` exits 0.
- `RELEASING.md` and `CLAUDE.md` confirmed to already be correct (no edit needed there).

## Error Handling & Edge Cases

- **A future `cargo-dist` config change alters the asset count/names** — out of scope here; the
  14-asset list is documented as the current baseline (v0.29.0/v0.28.1/v0.28.0), not asserted as
  permanent. A later drift is a future issue, the same class of staleness this change fixes now.
- **The check script's `--update` mode is run before the mirrored edit lands in the port** — would
  regenerate a record that still shows the stale text as part of the "intended" divergence. Mitigated
  procedurally: edit canonical, then port, then run `--update`, in that order, and diff the
  regenerated record by eye before committing to confirm no `.deb`/`.rpm` text survives in it.

## Risks & Open Questions

- **None identified.** This is a bounded, mechanical prose edit with an unambiguous correct wording
  already established by `RELEASING.md`, in a file with no runtime behavior to regress.

# Rename project from otto to otto — design

Date: 2026-09-07
Status: IMPLEMENTED
Related: `savvagent/otto#51`

## Problem

The GitHub repository has already been renamed in place from `savvagent/otto` to
`savvagent/otto`, but the codebase itself still uses `otto` as the crate-name prefix, the
binary name, the on-disk config directory (`~/.otto/`), the project-context filename
(`OTTO.md`), the keyring service name, every `OTTO_*` environment variable, the WIT
package name (`otto:plugin@0.1.0`), and throughout docs/CI/locales. `otto` currently
appears in ~383 tracked files (excluding `Cargo.lock`/`target`). Issue #51 requires a coordinated
sweep renaming all of it to `otto`, with no compatibility shims, because there are no external
users yet.

## Goal & Success Criteria

Every `otto`-derived crate, binary, env var, on-disk path, keyring service, WIT package, and
identifier is renamed to its `otto` equivalent, and the workspace builds and tests green throughout.

- `cargo build --workspace --all-targets` and `cargo test --workspace` pass after the sweep.
- No tracked source file (excluding historical `CHANGELOG.md` entries and `Cargo.lock`, which is
  regenerated) contains the string `otto` (case-insensitive) except where it names the GitHub
  org handle `otto` (which is unchanged — only the repo name changed).
- A from-scratch launch of the renamed binary creates `~/.otto/` and can connect a provider.
- The WIT package rename (`otto:plugin@0.1.0`) round-trips through at least one example plugin.

## Approach

This is a mechanical, coordinated rename executed as git mv + text substitution across the tree,
landed as one PR (the four "layers" in the issue are implementation order within a single PR/commit
sequence here, not four separate PRs, to keep the workspace buildable at every commit and avoid
review overhead multiplying four-fold for a single mechanical sweep):

1. **Cargo workspace**: `git mv crates/otto crates/otto`; `git mv crates/otto-{protocol,mcp,host,plugin,plugin-wit,plugin-wasm,fence,canvas} crates/otto-*`. Update package
   `name` in each moved crate's `Cargo.toml`, the root `Cargo.toml` `[workspace] members`,
   `default-members`, `[workspace.package] repository`, and every `[workspace.dependencies]` entry.
   Update every `use otto_*` / `otto_*::` reference across the tree (`crates/`, `examples/`,
   `docs/`) to `otto_*`.
2. **Binaries**: rename `[[bin]]` targets and `src/bin/otto-*.rs` files in `crates/otto/Cargo.toml`
   (`otto` → `otto`, `otto-tool-{fs,lsp,bash,grep,web}` → `otto-tool-*`, `otto-anthropic`
   → `otto-anthropic`, etc.).
3. **User-facing surfaces**: `~/.otto/` → `~/.otto/`, `OTTO.md` → `OTTO.md`, keyring service
   `otto` → `otto`, every `OTTO_*` env var → `OTTO_*`, WIT package `otto:plugin@0.1.0`
   → `otto:plugin@0.1.0` (plugin fixtures + the two WIT guard workflows), installer URLs.
4. **Docs, CI, locales**: sweep `README.md`, `PRD.md`, `CLAUDE.md`, `RELEASING.md`, `SECURITY.md`,
   `CHANGELOG.md` (only the `[Unreleased]` section and going forward — historical dated entries are
   left as the historical record of what shipped under the old name), `Justfile`, `bacon.toml`,
   `release-plz.toml`, `crates/otto-protocol/SPEC.md`, `docs/**` (including
   `docs/superpowers/specs` and `docs/superpowers/plans`, rewritten per the issue's decision 5),
   `.github/workflows/*.yml`, `.github/skills/otto-development/` → `otto-development`,
   `.claude/skills/tui-engineer/SKILL.md`, and every locale catalog under `crates/otto/locales/`.

No dual-read fallback, no deprecation shim, no `~/.otto` → `~/.otto` migration path — per the
issue's settled decision 2, this is a hard rename.

## Scope

**In:**
- Every item enumerated in issue #51's Scope section (Cargo workspace, binaries, user-facing
  surfaces, docs/CI/locales).
- Updating the local `origin` git remote to `https://github.com/savvagent/otto.git` (developer-machine
  configuration, not a repo file, so this is done once locally and mentioned in the PR description
  for other developers to replicate — it is not itself a commit).

**Out:**
- Renaming the GitHub org handle `otto` (stays as-is per issue decision 1).
- `provider-*` / `tool-*` crate directory names (they keep their names per issue scope item 1; only
  their `otto-*` path/version dependency references change).
- Any behavioral change to the turn loop, provider transport split, or tool dispatch — this is a
  naming-only sweep.
- A version bump beyond whatever the next release naturally carries (issue decision 4: no bump for
  the rename itself).

## Public-interface changes

**Breaking, comprehensively.** Every public interface this repo has is renamed: the on-disk config
directory, project-context filename, keyring service name, every environment variable, the WIT
plugin package, and every binary name. Per issue decision 2 there is no migration/compat shim — old
names are deleted outright. This must be called out in `CHANGELOG.md`'s `[Unreleased]` section as a
breaking rename and ships in the next release's version per the repo's SemVer convention (a rename of
this magnitude, touching binaries/env vars/on-disk formats, warrants a MINOR bump at release-cut
time, though issue decision 4 says the rename itself does not force an out-of-cycle bump — it rides
the next release regardless of size).

## Premise corrections

- None — the issue's decisions section is unusually explicit and settled; this spec follows it
  directly rather than second-guessing it.

## Assumptions

- Land as one PR rather than four, because the workspace must build at every commit in CI and a
  4-PR split for a purely mechanical, single-session rename adds review overhead without added
  safety; the "layers" become ordered commits within the PR instead.
- `docs/superpowers/specs/` and `docs/superpowers/plans/` (including this very spec and its
  sibling plan) are rewritten in place per issue decision 5 — historical entries that already shipped
  keep their `Status: IMPLEMENTED` and are edited in place to say `otto` instead of `otto`,
  since the issue treats these as living docs, not a frozen historical record.
- `CHANGELOG.md`'s already-dated, already-released entries (anything above `[Unreleased]`) are left
  referring to `otto` as it existed at the time, because those are a historical record of what
  shipped under the old name at that version — rewriting shipped changelog history would be
  misleading. Only the `[Unreleased]` section and the new entry this rename ships under use `otto`.
- The GitHub org handle `otto` (e.g. `github.com/savvagent/otto`) is never touched — only the
  repo name portion of URLs changes from `otto` to `otto`.
- `Cargo.lock` is regenerated via `cargo check --workspace` rather than hand-edited.

## Error Handling & Edge Cases

- A stray `otto` string surviving in a non-historical file (e.g. a comment, a locale string, a
  workflow step) is a regression this sweep must catch via a final case-insensitive grep sweep across
  tracked files excluding `Cargo.lock`, `target/`, and dated `CHANGELOG.md` entries.
- The WIT package rename is an ABI break for anything built against `otto:plugin` — moot with no
  external plugin authors (issue's own note), but the example plugins must still build and load
  against the new `otto:plugin@0.1.0` package to prove the rename didn't silently break the plugin
  ABI machinery itself.
- Renaming `crates/otto` to `crates/otto` changes `default-members` and the binary the TUI
  spawns at runtime (`otto-tool-fs`); `cargo build` (bare, matching `default-members`) must succeed,
  not just `cargo build --workspace`.

## Spec critique corrections (round 1)

- The plugin ABI rename is broader than the WIT package alone: `crates/otto-plugin-wasm`'s
  manifest format requires a `[plugin].otto` key (`ManifestPlugin::otto`), and its
  discovery module exposes public `SourceScope::{ProjectOtto,UserOtto}` variants. Both are
  part of the plugin's public interface and must rename to `otto` alongside the WIT package, with
  every example/fixture manifest (`otto = "^0.18"` etc.) updated to `otto = "^0.18"`.
- The committed `.wasm` test fixtures under `crates/otto-plugin-wasm/tests/fixtures/*.wasm` are
  compiled artifacts embedding the old WIT world/interface names. They must be rebuilt from
  `tests/fixtures-src/` after the WIT package/manifest key rename and re-committed, not left as
  stale binaries — otherwise the plugin-wasm adapter test suite silently keeps testing the old ABI
  name.
- Hardcoded GitHub repository identifiers for the self-update and in-app changelog features
  (`crates/otto/src/plugin/builtin/self_update/{check,apply}.rs`'s `REPO_NAME` constant and
  release-API URL, `crates/otto/src/plugin/builtin/changelog/fetch.rs`'s raw-changelog URL) are
  a runtime-visible surface, not just documentation — these must change from
  `savvagent/otto` to `savvagent/otto` or `/update` and the in-app changelog viewer will
  keep pointing at the old repository.
- The final tracked-file grep sweep is broadened to **all tracked files**, explicitly including
  `.gitignore` (which references old crate directory paths in its own comments/patterns) and every
  tracked file under `.claude/**` (not just `tui-engineer/SKILL.md`).
- Verification of the provider-transport-split env var rename (`OTTO_PROVIDER_URL` →
  `OTTO_PROVIDER_URL`) is made explicit rather than relying on compilation alone: confirm the
  in-process path is selected when unset, and the MCP-over-HTTP path is selected when set, matching
  existing test coverage for that env var under its new name.
- The `CHANGELOG.md` historical-entry exception is implemented by excluding only already-dated
  release sections from the final grep sweep, not the whole file — the `[Unreleased]` section must
  still be caught by the sweep if it accidentally retains an old-name reference.

## Risks & Open Questions

- 383 files is large enough that a single missed reference is likely on the first pass; the plan
  budgets an explicit final-sweep verification task rather than trusting the mechanical
  git-mv/sed pass alone.
- Locale catalogs (`crates/otto/locales/*.toml`) must stay in sync across all languages — a
  scripted substitution is safer here than manual per-file editing.
- This spec does not itself cut the release; per this skill's Non-Negotiable Rule 8 a dedicated
  release PR follows this PR's merge, per `RELEASING.md`.

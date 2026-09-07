# rename-savvagent-to-otto Implementation Plan

**Goal:** Rename every `savvagent`-derived crate, binary, on-disk path, keyring service, env var,
WIT package, and identifier to its `otto` equivalent across the whole tree, with no compatibility
shims, landing as one coordinated PR that keeps `cargo build --workspace --all-targets` and
`cargo test --workspace` green at each task boundary.

**Architecture:** This is a naming-only sweep — no turn-loop, host-swap, provider-transport, or
tool-dispatch behavior changes. `crates/savvagent` becomes `crates/otto` (the TUI binary + its
`[[bin]]` companions); `crates/savvagent-{protocol,mcp,host,plugin,plugin-wit,plugin-wasm,fence,canvas}`
become `crates/otto-*`; `provider-*`/`tool-*` crate directories are untouched but their
`savvagent-*` path/version dependency references change. User-facing surfaces (`~/.savvagent/` →
`~/.otto/`, `SAVVAGENT.md` → `OTTO.md`, keyring service, `SAVVAGENT_*` env vars, the WIT package
`savvagent:plugin@0.1.0` → `otto:plugin@0.1.0`) and docs/CI/locales follow in later tasks once the
workspace itself compiles under the new crate names.

**Tech Stack:** Rust 2024 Cargo workspace, `git mv` for history-preserving renames, scripted
case-sensitive text substitution (`sed`) for the ~383-file sweep, `cargo check`/`cargo test`/
`cargo clippy`/`cargo fmt` for verification, `wasm-tools`/the existing WIT toolchain for the plugin
ABI rename, GitHub CLI for issue/PR/release workflow.

**Spec:** `docs/superpowers/specs/2026-09-07-rename-savvagent-to-otto-design.md` — read it first.
This plan implements it exactly.

**Release line:** `v0.24.0` (MINOR bump per the repo's pre-1.0 convention — this is a breaking
rename of every public interface the repo has, per Non-Negotiable Rule 6, even though issue #51
decision 4 says the rename itself doesn't force an out-of-cycle release; it ships as the next
release's content).

**Branch:** `rename/savvagent-to-otto`

## File Map

**Renamed directories (git mv, history preserved)**
- `crates/savvagent` → `crates/otto`
- `crates/savvagent-protocol` → `crates/otto-protocol`
- `crates/savvagent-mcp` → `crates/otto-mcp`
- `crates/savvagent-host` → `crates/otto-host`
- `crates/savvagent-plugin` → `crates/otto-plugin`
- `crates/savvagent-plugin-wit` → `crates/otto-plugin-wit`
- `crates/savvagent-plugin-wasm` → `crates/otto-plugin-wasm`
- `crates/savvagent-fence` → `crates/otto-fence`
- `crates/savvagent-canvas` → `crates/otto-canvas`
- `crates/otto/src/bin/savvagent-{anthropic,gemini,openai,tool-bash,tool-fs,tool-grep,tool-lsp,tool-web}.rs`
  → `crates/otto/src/bin/otto-{anthropic,gemini,openai,tool-bash,tool-fs,tool-grep,tool-lsp,tool-web}.rs`
- `.github/skills/savvagent-development/` → `.github/skills/otto-development/`

**Modified files (representative; the sweep touches ~383 tracked files total)**
- Root `Cargo.toml` — workspace `members`, `default-members`, `[workspace.package] repository`,
  every `[workspace.dependencies]` local-crate entry and `[[bin]]`-adjacent references.
- Every moved crate's own `Cargo.toml` — `name`, and any `savvagent-*` path dependency.
- Every `use savvagent_*` / `savvagent_*::` Rust import across `crates/`, `examples/`.
- `crates/otto-host/src/{config,default_prompt,permissions,project,session}.rs`,
  `crates/otto-plugin-wasm/src/discovery.rs` — `~/.savvagent/`, `SAVVAGENT.md`, keyring service,
  `SAVVAGENT_*` env vars.
- `crates/otto-plugin-wit/wit/*.wit` — WIT package rename.
- `.github/workflows/*.yml` (all 7) — image/job references, WIT guard workflows.
- `README.md`, `PRD.md`, `CLAUDE.md`, `RELEASING.md`, `SECURITY.md`, `CHANGELOG.md` (`[Unreleased]`
  only), `Justfile`, `bacon.toml`, `release-plz.toml`, `crates/otto-protocol/SPEC.md`.
- `docs/**` including `docs/superpowers/specs/*`, `docs/superpowers/plans/*`,
  `docs/plugins/authoring.md`.
- `crates/otto/locales/{en,es,hi,pt}.toml`.
- `.claude/skills/tui-engineer/SKILL.md`.

## Task 1: Rename Cargo workspace crates and fix all imports

**Files:** all crate directories under `crates/`, root `Cargo.toml`, every `.rs` file with a
`savvagent_*` `use`/path reference.

- [ ] Baseline: run `cargo build --workspace --all-targets` on the current tree and confirm it is
      green before making any change (guards against attributing a pre-existing failure to this
      task).
- [ ] `git mv crates/savvagent crates/otto`
- [ ] `git mv crates/savvagent-protocol crates/otto-protocol`
- [ ] `git mv crates/savvagent-mcp crates/otto-mcp`
- [ ] `git mv crates/savvagent-host crates/otto-host`
- [ ] `git mv crates/savvagent-plugin crates/otto-plugin`
- [ ] `git mv crates/savvagent-plugin-wit crates/otto-plugin-wit`
- [ ] `git mv crates/savvagent-plugin-wasm crates/otto-plugin-wasm`
- [ ] `git mv crates/savvagent-fence crates/otto-fence`
- [ ] `git mv crates/savvagent-canvas crates/otto-canvas`
- [ ] In each moved crate's `Cargo.toml`, change `name = "savvagent*"` to `name = "otto*"`.
- [ ] In the root `Cargo.toml`, update `[workspace] members` and `default-members` to the new
      `crates/otto*` paths, `[workspace.package] repository` to
      `https://github.com/savvagent/otto`, and every `[workspace.dependencies]` local-crate entry
      (`savvagent-plugin`, `savvagent-plugin-wit`, `savvagent-plugin-wasm`, `savvagent-protocol`,
      `savvagent-mcp`, `savvagent-host`, `savvagent-fence`, `savvagent-canvas`) to its `otto-*` name
      and `path`.
  - Note: `crates/savvagent-host/tests/fixtures/resource-tool` and
    `crates/tool-lsp/tests/fixtures/fake-lsp` workspace-member paths move with the `otto-host`
    rename (the former); the latter is untouched (`tool-lsp` keeps its name).
- [ ] Sweep every `.rs` file for `use savvagent_` / `savvagent_protocol::` / `savvagent_mcp::` /
      `savvagent_host::` / `savvagent_plugin` (and `_wit`/`_wasm` variants) /
      `savvagent_fence::` / `savvagent_canvas::` and replace the crate-name segment with `otto_*`
      (Rust crate names use underscores, so `savvagent-host` → `otto_host` in `use` paths). Use a
      scripted `grep -rl` + `sed -i 's/savvagent_/otto_/g'` pass across `crates/` and `examples/`,
      then hand-verify with `cargo check --workspace` — sed cannot distinguish "this identifier
      happens to contain savvagent_" from "this is a crate path", so re-check compile errors
      surface any place the substitution over- or under-fired.
- [ ] `cargo check --workspace` until clean; this regenerates `Cargo.lock` — do not hand-edit it.
- [ ] `cargo build --workspace --all-targets` — must succeed.
- [ ] Public-interface note: this task alone is not yet user-visible (crate names are an internal
      Rust concern, not a documented public interface) — no CHANGELOG entry needed for this task in
      isolation; the entry is added once Task 3 lands the user-visible surface renames.
- [ ] `git add -A && git commit -m "otto: rename savvagent-* crates to otto-*"`

## Task 2: Rename binaries

**Files:** `crates/otto/Cargo.toml` (`[[bin]]` targets), `crates/otto/src/bin/savvagent-*.rs` files.

- [ ] `git mv crates/otto/src/bin/savvagent-anthropic.rs crates/otto/src/bin/otto-anthropic.rs`
      (repeat for gemini, openai, tool-bash, tool-fs, tool-grep, tool-lsp, tool-web).
- [ ] In `crates/otto/Cargo.toml`, rename the `[[bin]] name = "savvagent"` entry (the main TUI
      binary) to `"otto"` and update its `path` if explicit; rename every `[[bin]]` entry pointing
      at the moved `src/bin/*.rs` files to match (`otto-anthropic`, `otto-gemini`, `otto-openai`,
      `otto-tool-bash`, `otto-tool-fs`, `otto-tool-grep`, `otto-tool-lsp`, `otto-tool-web`).
- [ ] Grep the tree for any remaining reference to the old binary names as strings (not just crate
      paths) — e.g. `HostConfig::with_tool` wiring in `crates/otto/src/main.rs` that resolves
      `savvagent-tool-fs` via `$PATH`, and any test that spawns a binary by its old name.
- [ ] `cargo build --workspace --all-targets` — confirm every renamed `[[bin]]` target builds and
      the binary artifacts appear under `target/debug/` with the new names
      (`ls target/debug/otto target/debug/otto-tool-fs` etc.).
- [ ] Public-interface note: binary names are user-facing (installer scripts, `$PATH` entries,
      `SAVVAGENT_TOOL_*_BIN` overrides depend on them) — flag as part of the Task 3 breaking-change
      CHANGELOG entry, not duplicated here.
- [ ] `git add -A && git commit -m "otto: rename savvagent-* binaries to otto-*"`

## Task 3: Rename user-facing surfaces (env vars, on-disk paths, keyring, WIT package)

**Files:** `crates/otto-host/src/{config,default_prompt,permissions,project,session}.rs`,
`crates/otto-plugin-wasm/src/discovery.rs`, `crates/otto-plugin-wit/wit/*.wit`,
`.github/workflows/wit-dep-guard.yml`, `.github/workflows/wit-portability-guard.yml`, any test
fixture under `examples/plugin-hello-*` referencing the WIT package, `CHANGELOG.md`.

- [ ] Grep for every `SAVVAGENT_` env var reference (`SAVVAGENT_HOME`, `SAVVAGENT_MODEL`,
      `SAVVAGENT_PROJECT_DIR`, `SAVVAGENT_PROVIDER_URL`, `SAVVAGENT_KEYRING_SERVICE`,
      `SAVVAGENT_AGENT_MAX_DEPTH`, `SAVVAGENT_NO_UPDATE_CHECK`,
      `SAVVAGENT_ALLOW_MISSING_LIVE_KEYS`, `SAVVAGENT_TOOL_*_BIN`/`_ROOT`,
      `SAVVAGENT_{ANTHROPIC,GEMINI,OPENAI}_LISTEN`, `SAVVAGENT_BRAVE_API_KEY`,
      `SAVVAGENT_SEARXNG_URL`, `SAVVAGENT_TURN_ID__`) across `crates/`, `docs/`, `README.md`,
      `.env`/`.env.local` examples and replace `SAVVAGENT_` → `OTTO_`.
- [ ] Grep for `.savvagent` (config dir path construction, e.g. `dirs::home_dir().join(".savvagent")`)
      and replace with `.otto`, in every crate that constructs the config/state directory path
      (`otto-host`, `otto-plugin-wasm`, `crates/otto`).
- [ ] Grep for `SAVVAGENT.md` (project-context filename) and replace with `OTTO.md` in
      `otto-host`'s project-context loader and every doc/comment referencing it.
- [ ] Grep for the keyring service string `"savvagent"` (distinct from crate/binary names — this is
      the literal string passed to the OS keyring API) and replace with `"otto"` in `otto-host`'s
      credential-storage code and `/connect`/`/mcp` writers.
- [ ] Rename the WIT package in all five `crates/otto-plugin-wit/wit/*.wit` files:
      `package savvagent:plugin@0.1.0;` → `package otto:plugin@0.1.0;`, and update every `use
      savvagent:plugin/...` import within those same files.
- [ ] Update `examples/plugin-hello-{interactive,provider,static}` (or wherever their WIT bindings
      reference the old package) to the new `otto:plugin@0.1.0` package.
- [ ] Update `.github/workflows/wit-dep-guard.yml` and `.github/workflows/wit-portability-guard.yml`
      for the new WIT package name wherever they reference it literally.
- [ ] Rename the plugin manifest's public `[plugin].savvagent` key to `[plugin].otto` in
      `crates/otto-plugin-wasm/src/manifest.rs` (`ManifestPlugin::savvagent` field → `::otto`,
      including its doc comment and `validate_version_range` call site), and update every example
      and test-fixture manifest string in that file and under `examples/plugin-hello-*` from
      `savvagent = "^0.18"` (etc.) to `otto = "^0.18"`.
- [ ] Rename the public `SourceScope::{ProjectSavvagent,UserSavvagent}` enum variants in
      `crates/otto-plugin-wasm/src/discovery.rs` to `{ProjectOtto,UserOtto}` and update every call
      site and test assertion referencing them.
- [ ] Rebuild the committed `.wasm` test fixtures under
      `crates/otto-plugin-wasm/tests/fixtures/*.wasm` from their sources in
      `crates/otto-plugin-wasm/tests/fixtures-src/*` now that the WIT package and manifest key have
      changed, and re-commit the rebuilt binaries — do not leave the old compiled artifacts in
      place. Run the plugin-wasm adapter test suite (`cargo test -p otto-plugin-wasm`) to confirm
      they load correctly under the new ABI names.
- [ ] Rename the hardcoded GitHub repository identifiers used by the self-update and in-app
      changelog features: `crates/otto/src/plugin/builtin/self_update/apply.rs`'s `REPO_NAME`
      constant and the release-download URL assertion, `crates/otto/src/plugin/builtin/self_update/check.rs`'s
      release-API URL constant, and `crates/otto/src/plugin/builtin/changelog/fetch.rs`'s raw-changelog
      URL constant and its test assertion — all from `savvagent/savvagent-cli` to `savvagent/otto`.
- [ ] Verify the provider-transport-split env var rename explicitly (not just via compilation):
      confirm existing tests covering `SAVVAGENT_PROVIDER_URL` (now `OTTO_PROVIDER_URL`) still pass
      under the new name — unset selects the in-process `InProcessProviderClient` path, set selects
      the MCP-over-HTTP `rmcp` Streamable HTTP path.
- [ ] Update installer URL references (`.../savvagent/savvagent-cli/releases/latest/download/
      savvagent-installer.sh` and `.ps1`) to `.../savvagent/otto/releases/latest/download/
      otto-installer.sh` in `README.md` and any install script under the repo.
- [ ] Rebuild and run the plugin loader test path: `cargo build --workspace --all-targets`, then
      build at least one example plugin (`cargo build -p plugin-hello-static` or the example's own
      build command) and confirm it still loads under the new WIT package name — run its existing
      test/example harness, don't just compile it.
- [ ] `cargo test --workspace` — full pass, confirming project-context loading
      (`OTTO.md`), config-dir path construction (`~/.otto/`), and keyring code paths' unit tests
      (which mock the keyring, not the real OS keyring) still pass under the new names.
- [ ] Public-interface change (breaking, comprehensive): add a `CHANGELOG.md` `[Unreleased]` entry
      under a `### Changed` heading describing the full rename (config dir, project-context file,
      keyring service, env vars, WIT package, binaries) as a breaking change with no migration
      path, matching Non-Negotiable Rule 6's requirement to name breaking changes explicitly.
- [ ] `git add -A && git commit -m "otto: rename user-facing surfaces (env vars, paths, keyring, WIT package) to otto"`

## Task 4: Sweep docs, CI, locales, and skills; final verification

**Files:** `README.md`, `PRD.md`, `CLAUDE.md`, `RELEASING.md`, `SECURITY.md`, `Justfile`,
`bacon.toml`, `release-plz.toml`, `crates/otto-protocol/SPEC.md`, `docs/**`,
`.github/workflows/*.yml` (all 7), `.github/skills/savvagent-development/` →
`.github/skills/otto-development/`, `.claude/skills/tui-engineer/SKILL.md`,
`crates/otto/locales/{en,es,hi,pt}.toml`.

- [ ] `git mv .github/skills/savvagent-development .github/skills/otto-development` and update the
      skill's own internal self-references (repo name, `savvagent/savvagent-cli` → `savvagent/otto`,
      crate names, branch/commit-scope examples) throughout `SKILL.md` and `agent-prompts.md`.
- [ ] Sweep `README.md`, `PRD.md`, `CLAUDE.md`, `RELEASING.md`, `SECURITY.md`,
      `crates/otto-protocol/SPEC.md`, `Justfile`, `bacon.toml`, `release-plz.toml` for every
      remaining `savvagent`/`SAVVAGENT` occurrence (repo name, crate names, binary names, paths, env
      vars, keyring service) and replace with the `otto` equivalent. Preserve the GitHub org handle
      `savvagent` unchanged (only the repo-name portion of URLs changes:
      `savvagent/savvagent-cli` → `savvagent/otto`).
- [ ] Sweep `docs/**` (including `docs/plugins/authoring.md`, `docs/superpowers/specs/*`,
      `docs/superpowers/plans/*`, excluding this plan's and this task's own file, which are already
      correct) for `savvagent`/`SAVVAGENT` occurrences and rewrite in place, per issue decision 5 —
      these are living docs, not a frozen historical record. Already-`IMPLEMENTED` specs/plans keep
      their `Status: IMPLEMENTED` header; only the body text changes.
- [ ] Sweep all 7 `.github/workflows/*.yml` files for `savvagent` occurrences (binary names, crate
      names, cache keys, artifact names, WIT package references not already handled in Task 3) and
      replace with `otto` equivalents.
- [ ] Sweep `.claude/skills/tui-engineer/SKILL.md` for `savvagent` occurrences and replace.
- [ ] Sweep every other tracked file under `.claude/**` (e.g. `.claude/skills/rust-engineer/SKILL.md`)
      for `savvagent` occurrences and replace, not just `tui-engineer/SKILL.md`.
- [ ] Sweep `.gitignore` for `savvagent`-derived path references (crate directory paths, generated
      file comments) and update them to the new `otto-*` crate directory names.
- [ ] Sweep `crates/otto/locales/{en,es,hi,pt}.toml` for every string mentioning "savvagent",
      `~/.savvagent/...`, `savvagent-tool-*`, or `SAVVAGENT_TOOL_*_BIN` and update all four locale
      catalogs consistently (script the substitution rather than hand-editing four files
      independently, to avoid the catalogs drifting out of sync).
- [ ] Update `CHANGELOG.md`'s `[Unreleased]` section (only — leave every already-dated, released
      entry above it referring to `savvagent` as the historical record of what shipped under that
      name) with any additional docs/CI/locale-specific notes, if not already fully covered by
      Task 3's entry.
- [ ] Final verification sweep: `grep -ril "savvagent" --include="*" . | grep -v -E
      '(^\./target/|^\./\.git/|Cargo\.lock)'` (case-insensitive) across the whole tree, excluding
      only `target/`, `.git/`, and `Cargo.lock` — must return zero hits except (a) the intentional
      `savvagent` GitHub org handle, spot-checked rather than blanket-flagged, and (b) dated,
      already-released `CHANGELOG.md` sections (verify by inspecting each `CHANGELOG.md` hit
      individually: a hit inside the `[Unreleased]` section is a real miss and must be fixed; a hit
      inside a dated `## X.Y.Z - YYYY-MM-DD` section is expected historical record).
- [ ] `cargo build --workspace --all-targets` — final green build.
- [ ] `cargo test --workspace --no-fail-fast` — final green test run.
- [ ] `cargo clippy --workspace --all-targets` — no new warnings (`RUSTFLAGS=-D warnings` in CI).
- [ ] `cargo fmt --all --check` — clean.
- [ ] `cargo test -p otto-host --test cross_vendor_history --no-fail-fast` — host-plumbing surface
      changed (project-context filename, config paths), so this regression suite runs explicitly.
- [ ] Out-of-band verification note (executed at Phase 5, not as a task here, but flagged for that
      phase): a from-scratch launch (`cargo run -p otto`) must create `~/.otto/` and connect a
      provider; this is the closest thing to a smoke test for the on-disk path rename and is not
      covered by `cargo test`.
- [ ] Update the local `origin` git remote to `https://github.com/savvagent/otto.git` (a
      developer-machine config change, not a commit — call this out again in the PR description for
      other contributors to replicate).
- [ ] `git add -A && git commit -m "docs: rename savvagent references to otto across docs, CI, locales, skills"`

## Task 5: Release note

The version bump, `CHANGELOG.md` heading rename (`[Unreleased]` → `## X.Y.Z - YYYY-MM-DD`), tag,
and GitHub Release are cut via a dedicated release PR per `RELEASING.md`, opened immediately after
this plan's PR merges to `main` (savvagent-development skill Non-Negotiable Rule 8 / Phase 4 step
12). No version bump or CHANGELOG heading change happens in this PR itself — only the `[Unreleased]`
breaking-change entry added in Task 3.

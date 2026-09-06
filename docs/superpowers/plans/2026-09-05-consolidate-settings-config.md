# consolidate-settings-config Implementation Plan

**Goal:** Consolidate the persisted language, theme, and self-update settings into `~/.savvagent/config.toml`, removing the standalone `language.toml` and `theme.toml` files while keeping existing runtime behavior intact except for the intentional new default update polling interval from 2 hours to 300 seconds.

**Architecture:** `crates/savvagent/src/config_file.rs` already owns the typed `ConfigFile` schema plus `load_or_default` / `save`. This plan extends that schema with three new optional sections and then rewires the language catalog, theme catalog, and self-update plugin to use the shared config file instead of separate persistence helpers or hardcoded defaults. The TUI/GUI bootstrap and effect-application call sites keep their existing responsibilities; only their storage backend changes. No host/provider/tool transport boundary changes are in scope, and the work must not alter the host-swap `RwLock` discipline.

**Tech Stack:** Rust 2024, Serde/TOML in `config_file.rs`, ratatui theme catalog types, async tokio timing in `internal:self-update`, and existing home-directory test helpers (`HOME_LOCK`, `HomeGuard`).

**Spec:** `docs/superpowers/specs/2026-09-05-consolidate-settings-config-design.md` — read it first. This plan implements it exactly.

**Release line:** `v0.22.0` (next MINOR after `0.21.0`, because removing `~/.savvagent/language.toml` and `~/.savvagent/theme.toml` is a breaking on-disk configuration change under the repo’s pre-1.0 SemVer convention).

**Branch:** `savvagent/consolidate-settings-config-toml`

## File Map

**New files**
- None.

**Modified files**
- `crates/savvagent/src/config_file.rs` — add `language`, `theme`, and `update` sections plus unit coverage for defaults/round-trips/preservation behavior.
- `crates/savvagent/src/plugin/builtin/language/catalog.rs` — replace standalone language-file persistence with `ConfigFile`-backed helpers and update tests.
- `crates/savvagent/src/plugin/builtin/themes/catalog.rs` — replace standalone theme-file persistence with `ConfigFile`-backed helpers and update tests.
- `crates/savvagent/src/plugin/builtin/self_update/mod.rs` — source default disabled/interval values from `ConfigFile`, preserve env/CLI overrides, and update timing/config tests.
- `crates/savvagent/src/app.rs` — keep language/theme persistence call sites but update any doc comments/tests that still name the removed files.
- `crates/savvagent/src/main.rs` — load the initial locale from the shared config-backed helper.
- `crates/savvagent/src/plugin/effects.rs` — update tests/assertions that currently expect `language.toml` writes.
- `README.md` — document the new `config.toml` sections and remove references to `language.toml` / `theme.toml`.

## Task 1: Extend `ConfigFile` for language/theme/update settings

**Files:**
- Modify: `crates/savvagent/src/config_file.rs`

- [ ] Add failing tests in `crates/savvagent/src/config_file.rs` for the new defaults and round-trip behavior: default `language.code = "en"`, default `theme.name = "dark"`, default `update.periodic_interval_secs = 300`, default `update.disabled = false`, plus a test that saving one section preserves existing `startup` / `migration` fields. Expected result: the new tests fail because the sections do not exist yet.
- [ ] Run `cargo test -p savvagent --lib config_file -- --nocapture`. Expected result: failure in the new config-file tests for missing fields/types.
- [ ] Implement `LanguageSection`, `ThemeSection`, and `UpdateSection` in `crates/savvagent/src/config_file.rs`, wire them into `ConfigFile`, and add any helper methods needed to expose the effective periodic interval safely (`0` falls back to the default `300`). Keep all new sections `#[serde(default)]` so existing `config.toml` files remain readable.
- [ ] Run `cargo test -p savvagent --lib config_file -- --nocapture`. Expected result: the config-file tests pass.
- [ ] Public-interface check: record in the task ledger that this is the breaking on-disk config change — new optional `config.toml` sections are additive within `config.toml`, but removing standalone `language.toml` / `theme.toml` is breaking and must be called out in the PR body and later release PR changelog.
- [ ] Host-swap/RwLock check: not applicable — no `app.rs` / `tui.rs` lock-bearing async path touched.
- [ ] ProgressDispatcher check: not applicable — no streaming provider path touched.
- [ ] Format and commit: `cargo fmt --all` then `git commit -m "savvagent: extend config.toml with language, theme, and update sections"`.

## Task 2: Move language/theme persistence to the shared config file

**Files:**
- Modify: `crates/savvagent/src/plugin/builtin/language/catalog.rs`
- Modify: `crates/savvagent/src/plugin/builtin/themes/catalog.rs`
- Modify: `crates/savvagent/src/app.rs`
- Modify: `crates/savvagent/src/main.rs`
- Modify: `crates/savvagent/src/plugin/effects.rs`

- [ ] Add/adjust failing tests in `crates/savvagent/src/plugin/builtin/language/catalog.rs`, `crates/savvagent/src/plugin/builtin/themes/catalog.rs`, and `crates/savvagent/src/plugin/effects.rs` so they assert writes land in `config.toml`, not `language.toml` / `theme.toml`, and that loading preserves locale/theme semantics. Expected result: tests fail while the old file-specific helpers are still in use.
- [ ] Run `cargo test -p savvagent --lib plugin::builtin::language::catalog plugin::builtin::themes::catalog plugin::effects -- --nocapture`. Expected result: the updated tests fail because runtime code still reads/writes the removed standalone files.
- [ ] Replace the dedicated `LanguageConfig`/`config_path`/`load`/`save` helpers in `crates/savvagent/src/plugin/builtin/language/catalog.rs` with `ConfigFile`-backed equivalents. Preserve locale detection precedence (`saved config` → `LC_ALL` → `LC_MESSAGES` → `LANG` → `en`) and unsupported-code fallback semantics.
- [ ] Replace the dedicated `ThemeConfig`/`config_path`/`load[_from_path]`/`save[_to_path]` helpers in `crates/savvagent/src/plugin/builtin/themes/catalog.rs` with `ConfigFile`-backed equivalents, preserving the current `Theme` slug serde contract.
- [ ] Update `crates/savvagent/src/app.rs`, `crates/savvagent/src/main.rs`, and `crates/savvagent/src/plugin/effects.rs` doc comments/tests/assertions so they name `config.toml` and verify `persist=false` leaves the shared config file absent or unchanged as appropriate.
- [ ] Run `cargo test -p savvagent --lib plugin::builtin::language::catalog plugin::builtin::themes::catalog plugin::effects app -- --nocapture`. Expected result: targeted tests pass and no runtime code references `language.toml` / `theme.toml` anymore.
- [ ] Public-interface check: confirm the slash-command surface is unchanged (`/language`, `/theme` still exist), but the on-disk persistence surface is the deliberate breaking change already captured in the spec.
- [ ] Host-swap/RwLock check: this task touches `app.rs` but only updates synchronous persistence/doc-comment paths; verify no `.await` is introduced while holding any `Arc<RwLock<...>>` read guard.
- [ ] ProgressDispatcher check: not applicable — no streaming provider path touched.
- [ ] Format and commit: `cargo fmt --all` then `git commit -m "savvagent: move language and theme persistence into config.toml"`.

## Task 3: Make self-update config-driven and refresh docs

**Files:**
- Modify: `crates/savvagent/src/plugin/builtin/self_update/mod.rs`
- Modify: `README.md`

- [ ] Add/adjust failing tests in `crates/savvagent/src/plugin/builtin/self_update/mod.rs` covering: config-driven default interval, `update.disabled = true` forcing `UpdateState::Disabled`, `SAVVAGENT_NO_UPDATE_CHECK` / `--no-update-check` overriding config, and `periodic_interval_secs = 0` falling back to `300`. Expected result: tests fail with the current hardcoded `PERIODIC_INTERVAL` / env-only disable logic.
- [ ] Run `cargo test -p savvagent --lib plugin::builtin::self_update -- --nocapture`. Expected result: failure in the new self-update config tests.
- [ ] Refactor `crates/savvagent/src/plugin/builtin/self_update/mod.rs` so the plugin reads `ConfigFile` for `update.periodic_interval_secs` and `update.disabled`, retains the existing test-only interval override seam, and keeps env/CLI opt-out as the highest-precedence override. The intentional behavior change from 2 hours to 300 seconds must be explicit in code comments/tests.
- [ ] Update `README.md` to remove standalone `theme.toml` / `language.toml` references, document `[language]`, `[theme]`, and `[update]` under `~/.savvagent/config.toml`, and document that `SAVVAGENT_NO_UPDATE_CHECK` remains an override over file configuration.
- [ ] Run `cargo test -p savvagent --lib plugin::builtin::self_update -- --nocapture`. Expected result: self-update tests pass.
- [ ] Public-interface check: confirm `SAVVAGENT_NO_UPDATE_CHECK` and `--no-update-check` remain supported, note the default polling interval change from 2 hours to 300 seconds, and note again that the removed standalone files are the breaking on-disk change to surface in the PR/release notes.
- [ ] Host-swap/RwLock check: not applicable — no `app.rs` / `tui.rs` async lock path touched.
- [ ] ProgressDispatcher check: not applicable — no streaming provider path touched.
- [ ] Format and commit: `cargo fmt --all` then `git commit -m "savvagent: read update settings from config.toml"`.

## Task 4: Full validation and release handoff note

**Files:**
- Modify only if needed to fix validation findings from Tasks 1–3.

- [ ] Run `cargo build --workspace --all-targets`. Expected result: success.
- [ ] Run `cargo test --workspace`. Expected result: success.
- [ ] Run `cargo clippy --workspace --all-targets` with `RUSTFLAGS=-D warnings`. Expected result: success with no warnings.
- [ ] Run `cargo fmt --all --check`. Expected result: success.
- [ ] Public-interface check: verify the feature PR summary explicitly states that `~/.savvagent/language.toml` and `~/.savvagent/theme.toml` were removed in favor of `~/.savvagent/config.toml`, and that the release PR must add the breaking-change `CHANGELOG.md` entry for `v0.22.0`.
- [ ] Host-swap/RwLock check: verify no task introduced an `.await` while any `Arc<RwLock<Option<Arc<Host>>>>` read guard is held.
- [ ] ProgressDispatcher check: verify no streaming-provider path was touched, so no forwarder-abort change was needed.
- [ ] Format and commit: if validation fixes were needed, run `cargo fmt --all` and `git commit -m "savvagent: fix post-validation findings"`; otherwise record in the task ledger that no extra code commit was required.
- [ ] Release handoff note: the dedicated post-merge release PR must bump `workspace.package.version` and all internal `workspace.dependencies` versions to `0.22.0`, move the breaking-change note into `CHANGELOG.md`, pass fmt/clippy/test again, then tag and publish `v0.22.0` per `RELEASING.md`.

# consolidate-settings-config Implementation Plan

**Goal:** Consolidate the persisted language, theme, and self-update settings into `~/.otto/config.toml`, removing the standalone `language.toml` and `theme.toml` files while keeping existing runtime behavior intact except for the intentional new default update polling interval from 2 hours to 300 seconds.

**Architecture:** `crates/otto/src/config_file.rs` already owns the typed `ConfigFile` schema plus `load_or_default` / `save`. This plan extends that schema with three new optional sections and then rewires the language catalog, theme catalog, and self-update plugin to use the shared config file instead of separate persistence helpers or hardcoded defaults. The TUI/GUI bootstrap and effect-application call sites keep their existing responsibilities; only their storage backend changes. No host/provider/tool transport boundary changes are in scope, and the work must not alter the host-swap `RwLock` discipline.

**Tech Stack:** Rust 2024, Serde/TOML in `config_file.rs`, ratatui theme catalog types, async tokio timing in `internal:self-update`, and existing home-directory test helpers (`HOME_LOCK`, `HomeGuard`).

**Spec:** `docs/superpowers/specs/2026-09-05-consolidate-settings-config-design.md` — read it first. This plan implements it exactly.

**Release line:** `v0.23.0` for the runtime/config consolidation, followed by `v0.23.1` for the changelog-traceability follow-up after the dedicated release-note correction PR landed on `main`.

**Branch:** `otto/consolidate-settings-config-toml`

## File Map

**New files**
- None.

**Modified files**
- `crates/otto/src/config_file.rs` — add `language`, `theme`, and `update` sections plus unit coverage for defaults/round-trips/preservation behavior.
- `crates/otto/src/plugin/builtin/language/catalog.rs` — replace standalone language-file persistence with `ConfigFile`-backed helpers and update tests.
- `crates/otto/src/plugin/builtin/themes/catalog.rs` — replace standalone theme-file persistence with `ConfigFile`-backed helpers and update tests.
- `crates/otto/src/plugin/builtin/self_update/mod.rs` — source default disabled/interval values from `ConfigFile`, preserve env/CLI overrides, and update timing/config tests.
- `crates/otto/src/app.rs` — keep language/theme persistence call sites but update any doc comments/tests that still name the removed files.
- `crates/otto/src/main.rs` — load the initial locale from the shared config-backed helper.
- `crates/otto/src/plugin/effects.rs` — update tests/assertions that currently expect `language.toml` writes.
- `README.md` — document the new `config.toml` sections and remove references to `language.toml` / `theme.toml`.

## Task 1: Extend `ConfigFile` for language/theme/update settings

**Files:**
- Modify: `crates/otto/src/config_file.rs`

- [x] Add failing tests in `crates/otto/src/config_file.rs` for the new defaults and round-trip behavior: default `language.code = "en"`, default `theme.name = "dark"`, default `update.periodic_interval_secs = 300`, default `update.disabled = false`, plus a test that saving one section preserves existing `startup` / `migration` fields, and a malformed shared `[theme]` entry falls back to defaults through `ConfigFile::load_or_default`. Expected result: the new tests fail because the sections/helpers do not exist yet.
- [x] Run `cargo test -p otto --bin otto config_file::tests -- --nocapture`. Expected result: failure in the new config-file tests for missing fields/types.
- [x] Implement `LanguageSection`, `ThemeSection`, and `UpdateSection` in `crates/otto/src/config_file.rs`, wire them into `ConfigFile`, and add any helper methods needed to expose the effective periodic interval safely (`0` falls back to the default `300`). Keep all new sections `#[serde(default)]` so existing `config.toml` files remain readable.
- [x] Run `cargo test -p otto --bin otto config_file::tests -- --nocapture`. Expected result: the config-file tests pass.
- [x] Public-interface check: record in the task ledger that this is the breaking on-disk config change — new optional `config.toml` sections are additive within `config.toml`, but removing standalone `language.toml` / `theme.toml` is breaking and must be called out in the PR body and later release PR changelog.
- [x] Host-swap/RwLock check: not applicable — no `app.rs` / `tui.rs` lock-bearing async path touched.
- [x] ProgressDispatcher check: not applicable — no streaming provider path touched.
- [x] Format and commit: `cargo fmt --all` then `git commit -m "otto: extend config.toml with language, theme, and update sections"`.

## Task 2: Move language/theme persistence to the shared config file

**Files:**
- Modify: `crates/otto/src/plugin/builtin/language/catalog.rs`
- Modify: `crates/otto/src/plugin/builtin/themes/catalog.rs`
- Modify: `crates/otto/src/app.rs`
- Modify: `crates/otto/src/main.rs`
- Modify: `crates/otto/src/plugin/effects.rs`

- [x] Add/adjust failing tests in `crates/otto/src/plugin/builtin/language/catalog.rs`, `crates/otto/src/plugin/builtin/themes/catalog.rs`, and `crates/otto/src/plugin/effects.rs` so they assert writes land in `config.toml`, not `language.toml` / `theme.toml`, that loading preserves locale/theme semantics, and that an invalid shared `[theme]` entry defaults cleanly through the shared config-file path. Expected result: tests fail while the old file-specific helpers are still in use.
- [x] Run `cargo test -p otto --bin otto plugin::builtin::language::catalog::tests -- --nocapture && cargo test -p otto --bin otto plugin::builtin::themes::catalog::tests -- --nocapture && cargo test -p otto --bin otto plugin::effects::tests::set_active_locale_persist_true_switches_rust_i18n_and_writes_file -- --nocapture && cargo test -p otto --bin otto app::tests::persist_language_writes_file_and_pushes_note -- --nocapture && cargo test -p otto --bin otto app::tests::persist_theme_writes_config_and_pushes_note -- --nocapture`. Expected result: at least one command fails because runtime code still reads/writes the removed standalone files.
- [x] Replace the dedicated `LanguageConfig`/`config_path`/`load`/`save` helpers in `crates/otto/src/plugin/builtin/language/catalog.rs` with `ConfigFile`-backed equivalents. Preserve locale detection precedence (`saved config` → `LC_ALL` → `LC_MESSAGES` → `LANG` → `en`) and unsupported-code fallback semantics.
- [x] Replace the dedicated `ThemeConfig`/`config_path`/`load[_from_path]`/`save[_to_path]` helpers in `crates/otto/src/plugin/builtin/themes/catalog.rs` with `ConfigFile`-backed equivalents, preserving the current `Theme` slug serde contract.
- [x] Update `crates/otto/src/app.rs`, `crates/otto/src/main.rs`, and `crates/otto/src/plugin/effects.rs` doc comments/tests/assertions so they name `config.toml` and verify `persist=false` leaves the shared config file absent or unchanged as appropriate.
- [x] Run `cargo test -p otto --bin otto plugin::builtin::language::catalog::tests -- --nocapture && cargo test -p otto --bin otto plugin::builtin::themes::catalog::tests -- --nocapture && cargo test -p otto --bin otto plugin::effects::tests::set_active_locale_persist_true_switches_rust_i18n_and_writes_file -- --nocapture && cargo test -p otto --bin otto plugin::effects::tests::set_active_locale_persist_false_does_not_write_file -- --nocapture && cargo test -p otto --bin otto app::tests::persist_language_writes_file_and_pushes_note -- --nocapture && cargo test -p otto --bin otto app::tests::persist_theme_writes_config_and_pushes_note -- --nocapture`. Expected result: targeted tests pass and no runtime code references `language.toml` / `theme.toml` anymore.
- [x] Public-interface check: confirm the slash-command surface is unchanged (`/language`, `/theme` still exist), but the on-disk persistence surface is the deliberate breaking change already captured in the spec.
- [x] Host-swap/RwLock check: this task touches `app.rs` but only updates synchronous persistence/doc-comment paths; verify no `.await` is introduced while holding any `Arc<RwLock<...>>` read guard.
- [x] ProgressDispatcher check: not applicable — no streaming provider path touched.
- [x] Format and commit: `cargo fmt --all` then `git commit -m "otto: move language and theme persistence into config.toml"`.

## Task 3: Make self-update config-driven and refresh docs

**Files:**
- Modify: `crates/otto/src/plugin/builtin/self_update/mod.rs`
- Modify: `README.md`

- [x] Add/adjust failing tests in `crates/otto/src/plugin/builtin/self_update/mod.rs` covering: config-driven default interval, `update.disabled = true` forcing `UpdateState::Disabled`, `OTTO_NO_UPDATE_CHECK` / `--no-update-check` overriding config, and `periodic_interval_secs = 0` falling back to `300`. Expected result: tests fail with the current hardcoded `PERIODIC_INTERVAL` / env-only disable logic.
- [x] Run `cargo test -p otto --bin otto plugin::builtin::self_update::tests -- --nocapture`. Expected result: failure in the new self-update config tests.
- [x] Refactor `crates/otto/src/plugin/builtin/self_update/mod.rs` so the plugin reads `ConfigFile` for `update.periodic_interval_secs` and `update.disabled`, retains the existing test-only interval override seam, and keeps env/CLI opt-out as the highest-precedence override. The intentional behavior change from 2 hours to 300 seconds must be explicit in code comments/tests.
- [x] Update `README.md` to remove standalone `theme.toml` / `language.toml` references, document `[language]`, `[theme]`, and `[update]` under `~/.otto/config.toml`, and document that `OTTO_NO_UPDATE_CHECK` remains an override over file configuration.
- [x] Run `cargo test -p otto --bin otto plugin::builtin::self_update::tests -- --nocapture`. Expected result: self-update tests pass.
- [x] Public-interface check: confirm `OTTO_NO_UPDATE_CHECK` and `--no-update-check` remain supported, note the default polling interval change from 2 hours to 300 seconds, and note again that the removed standalone files are the breaking on-disk change to surface in the PR/release notes.
- [x] Host-swap/RwLock check: not applicable — no `app.rs` / `tui.rs` async lock path touched.
- [x] ProgressDispatcher check: not applicable — no streaming provider path touched.
- [x] Format and commit: `cargo fmt --all` then `git commit -m "otto: read update settings from config.toml"`.

## Task 4: Full validation and release handoff note

**Files:**
- Modify only if needed to fix validation findings from Tasks 1–3.

- [x] Run `cargo build --workspace --all-targets`. Expected result: success.
- [x] Run `cargo test --workspace`. Expected result: success.
- [x] Run `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets`. Expected result: success with no warnings.
- [x] Run `cargo fmt --all --check`. Expected result: success.
- [x] Public-interface check: verify the feature PR summary explicitly states that `~/.otto/language.toml` and `~/.otto/theme.toml` were removed in favor of `~/.otto/config.toml`, and that the release PR added the breaking-change `CHANGELOG.md` entry for `v0.23.0`, followed by the `v0.23.1` traceability patch release.
- [x] Host-swap/RwLock check: verify no task introduced an `.await` while any `Arc<RwLock<Option<Arc<Host>>>>` read guard is held.
- [x] ProgressDispatcher check: verify no streaming-provider path was touched, so no forwarder-abort change was needed.
- [x] Format and commit: if validation fixes were needed, run `cargo fmt --all` and `git commit -m "otto: fix post-validation findings"`; otherwise record in the task ledger that no extra code commit was required.
- [x] Release handoff note: completed via dedicated release PRs `#47` (`v0.23.0`) and `#48` (`v0.23.1` follow-up traceability patch), with version bumps, `CHANGELOG.md` updates, validation, and published tags per `RELEASING.md`.

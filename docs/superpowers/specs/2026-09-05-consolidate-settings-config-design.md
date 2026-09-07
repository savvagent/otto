# Consolidate language, theme, and update settings into config.toml — design

Date: 2026-09-05
> **Status:** IMPLEMENTED
Related: `savvagent/otto#21`

## Problem

`otto` currently stores user-facing settings across three unrelated surfaces:

- `~/.otto/config.toml` already holds sectioned startup and migration state via `ConfigFile` (`crates/otto/src/config_file.rs:31-104`).
- Language selection persists to a dedicated `~/.otto/language.toml` via ad hoc helpers in `crates/otto/src/plugin/builtin/language/catalog.rs:78-154`.
- Theme selection persists to a dedicated `~/.otto/theme.toml` via parallel helpers in `crates/otto/src/plugin/builtin/themes/catalog.rs:224-271`.
- Self-update behavior mixes configuration sources: `OTTO_NO_UPDATE_CHECK` still controls the disable path and a hardcoded `PERIODIC_INTERVAL` of two hours drives the periodic loop in `crates/otto/src/plugin/builtin/self_update/mod.rs:76-92` and `:138-227`.

That split makes the on-disk story harder to document, duplicates load/save logic, and prevents the update loop from being configured through the same typed config surface the rest of startup already uses.

Issue #21 asks to consolidate these knobs into `~/.otto/config.toml`, explicitly removing the old per-feature TOML files with no back-compat/migration because the project has no users yet.

## Goal & Success Criteria

Move language, theme, and update settings into the existing `ConfigFile` as first-class optional sections, then update the language/theme/update code paths and docs so `config.toml` is the only persisted home-directory settings file for those surfaces.

Success criteria:
- `ConfigFile` supports new `[language]`, `[theme]`, and `[update]` sections with defaults and round-trip tests.
- `/language` and `/theme` load and persist their selections through `ConfigFile`; no runtime code writes or reads `~/.otto/language.toml` or `~/.otto/theme.toml` anymore.
- `internal:self-update` reads its periodic interval and default-disabled flag from `ConfigFile`, while still honoring `OTTO_NO_UPDATE_CHECK` as an override for CI/scripting.
- README/docs describe the new config layout, and the dedicated post-merge release PR updates `CHANGELOG.md` with the removal of the standalone language/theme files as a breaking on-disk-config change.
- Workspace build, test, clippy, and fmt checks pass after the change.

## Approach

1. **Extend `ConfigFile` in place.** Add `LanguageSection`, `ThemeSection`, and `UpdateSection` to `crates/otto/src/config_file.rs`, each behind `#[serde(default)]` so missing sections remain additive for existing `config.toml` readers. Mirror the file’s existing style: plain structs, explicit default helpers where needed, one `load_or_default` entry point, and focused unit tests in the same file.
2. **Route language persistence through `ConfigFile`.** Replace `LanguageConfig`, `config_path()`, `load()`, and `save()` in `crates/otto/src/plugin/builtin/language/catalog.rs` with helpers that read/write `ConfigFile::default_path()`. Keep locale detection precedence (`saved config` → `LC_ALL` → `LC_MESSAGES` → `LANG` → `en`) and unsupported-code fallback semantics, but make the saved source `config.toml` instead of `language.toml`.
3. **Route theme persistence through `ConfigFile`.** Replace the dedicated `ThemeConfig`, `config_path()`, `load[_from_path]`, and `save[_to_path]` helpers in `crates/otto/src/plugin/builtin/themes/catalog.rs` with `ConfigFile`-backed equivalents. Preserve the current `Theme` serde contract (slug strings, loud parse failures for unknown values), but scope it under `[theme]` rather than a standalone file.
4. **Update app/bootstrap call sites.** `App::new` currently seeds `active_theme` by calling the theme catalog loader (`crates/otto/src/app.rs:995`), `build_app_with_host` seeds `active_language` via `detect_initial()` (`crates/otto/src/main.rs:287`), and `App::persist_language` / `persist_config` call the old language/theme writers (`crates/otto/src/app.rs:1945-1979`). Those call sites stay, but their underlying helpers move to `ConfigFile`.
5. **Make self-update config-driven.** Replace the hardcoded `PERIODIC_INTERVAL` and default-disabled decision in `crates/otto/src/plugin/builtin/self_update/mod.rs` with values loaded from `ConfigFile`. The plugin should interpret config first, then let `OTTO_NO_UPDATE_CHECK` and `--no-update-check` force-disable regardless of file contents. Tests that currently inject a custom periodic interval should keep that seam so the async timing tests remain fast and deterministic.
6. **Update docs and tests for the new on-disk contract.** Remove README references to `language.toml` and `theme.toml`, document the new `config.toml` sections and update overrides, and replace tests that asserted the old files existed. The breaking-file-removal note for `CHANGELOG.md` lands in the dedicated post-merge release PR rather than this feature branch.

## Scope

**In:**
- `crates/otto/src/config_file.rs`
- `crates/otto/src/plugin/builtin/language/catalog.rs`
- `crates/otto/src/plugin/builtin/themes/catalog.rs`
- `crates/otto/src/plugin/builtin/self_update/mod.rs`
- Direct call sites/tests/docs that mention the old per-feature files (`app.rs`, `plugin/effects.rs`, README, CHANGELOG, any nearby tests)

**Out:**
- Changing startup-provider or migration behavior in `config.toml`
- Adding migration/back-compat code to read old `language.toml` / `theme.toml`
- Removing the `OTTO_NO_UPDATE_CHECK` env var or `--no-update-check` CLI flag
- Any provider/host/tool transport changes, plugin ABI changes, or TUI host-swap behavior changes

## Public-interface changes

**Breaking on-disk configuration change.** This change adds new optional sections to `~/.otto/config.toml` (additive within that file), but it also **removes** the previously-documented standalone `~/.otto/language.toml` and `~/.otto/theme.toml` persistence files. Per the workflow’s Non-Negotiable Rule 6, removing those files is a deliberate public-interface change to the on-disk configuration surface and must be:

- named in this spec and in the implementation plan,
- called out to the architecture reviewer,
- recorded in `CHANGELOG.md`, and
- reflected in the release version as a **MINOR** bump under this repo’s pre-1.0 SemVer convention.

The update configuration change is otherwise additive: `[update]` introduces file-backed defaults while retaining `OTTO_NO_UPDATE_CHECK` / `--no-update-check` as higher-precedence overrides.

No SPP wire format, MCP tool schema, plugin ABI, slash-command name, transcript format, or keyring format changes are intended.

## Premise corrections

1. **`ConfigFile` already has the right persistence primitives.** The issue’s requested `LanguageSection` / `ThemeSection` / `UpdateSection` additions fit the existing `load_or_default` + `save` model in `crates/otto/src/config_file.rs:78-103`; no new config framework or migration subsystem is needed.
2. **Language and theme persistence are currently parallel but not identical.** Language uses a custom `LanguageConfig` and atomic rename through `*.toml.tmp` (`language/catalog.rs:78-151`), while theme uses `load_from_path` / `save_to_path` helpers and direct writes (`themes/catalog.rs:224-280`). Consolidation should standardize both onto `ConfigFile` rather than preserving those differences.
3. **The self-update disable path is broader than just the env var.** The code also honors `--no-update-check` through `OPT_OUT_CLI_FLAG` in `self_update/mod.rs:79-111`. The file-backed `update.disabled` setting must coexist with both overrides rather than replacing them.

## Assumptions

- **Default language key name will be `code`.** Rationale: it matches the issue’s proposal and the existing catalog uses `Language::code` everywhere.
- **Default theme key name will be `name`, not `slug`.** Rationale: the issue explicitly proposes `name`, while `Theme` already serializes as the stable slug string, so `name = "tokyo-night"` still stores the actual theme identifier without inventing a second meaning.
- **Default update interval will be `300` seconds.** Rationale: the issue’s proposed schema shows `periodic_interval_secs = 300`; with no user migration burden, adopting that new default is acceptable and makes the new knob immediately visible instead of preserving the old hidden two-hour constant.
- **`update.disabled = true` should disable both the background check and `/update` exactly like today’s opt-out mechanisms.** Rationale: the existing plugin models “disabled” as a full short-circuit to `UpdateState::Disabled`, so config should map to the same semantics.
- **Persisting language/theme via `ConfigFile` does not need cross-process file locking.** Rationale: the existing dedicated-file writers also do not coordinate across processes; this change is a storage consolidation, not a new concurrency protocol.

## Error Handling & Edge Cases

- Missing `config.toml` must still yield defaults with no warning, exactly like current `ConfigFile::load_or_default` behavior.
- Invalid `config.toml` should continue to fall back to defaults with a warning from `ConfigFile::load_or_default`; language/theme callers must not add a second inconsistent parse path.
- An unsupported language code stored under `[language]` should behave like today’s unsupported `language.toml` value: ignore it and continue to env detection rather than crashing startup.
- An unknown theme name stored under `[theme]` should keep today’s loud parse behavior inside the config file load path; because `ConfigFile::load_or_default` falls back on parse errors, the net runtime behavior becomes “warn and use default theme.”
- `update.periodic_interval_secs = 0` should not spin a hot loop. The implementation should treat `0` as invalid and fall back to the section default (`300` seconds), and test that exact behavior explicitly.
- When config says updates are enabled but env/CLI says disabled, the override wins.
- When language/theme writers persist one section, they must preserve unrelated existing sections (`startup`, `migration`, the other new settings) instead of rewriting them away.

## Risks & Open Questions

- **Behavior change risk:** moving the default update interval from 2 hours to 5 minutes increases background check frequency. This is intentional per the assumption above, but it should be highlighted in the feature PR summary and in the release PR changelog entry.
- **Config parse blast radius:** because all sections share one `ConfigFile`, a malformed theme entry can now cause the whole file to fall back to defaults. That matches current `ConfigFile` semantics, but tests should pin the expected warnings/defaulting behavior.
- **Atomic write parity:** the language writer currently uses write-then-rename, while `ConfigFile::save` writes directly. If preserving atomicity for settings writes matters, that improvement should be made centrally in `ConfigFile::save` rather than per-feature.

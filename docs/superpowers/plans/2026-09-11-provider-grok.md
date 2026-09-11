# provider-grok Implementation Plan

**Goal:** Add Grok as a sixth built-in provider (`grok-4` / `grok-4-fast`), selectable from
`/connect`, `/model`, `/use`, and the `@provider:model` routing prefix, by mirroring
`provider-deepseek`'s shape end-to-end — a new library crate, a built-in provider plugin, and the
same registration call sites every other built-in provider touches.

**Architecture:** `crates/provider-grok` is a new library crate implementing `ProviderHandler`
(`otto-mcp`) against xAI's OpenAI-compatible legacy Chat Completions endpoint
(`https://api.x.ai/v1/chat/completions`) — no new transport, no turn-loop change, no host-swap
`RwLock` interaction. `crates/otto/src/plugin/builtin/provider_grok/mod.rs` is a `Plugin` +
`BuiltinProviderPlugin` shim (mirrors `provider_deepseek/mod.rs`) that reads `XAI_API_KEY` from the
OS keyring, builds an `InProcessProviderClient`, and registers with the host's provider pool exactly
like every other built-in. Five call sites wire the new plugin into the existing catalogs:
`plugin/mod.rs`'s `ProviderEntry` vec, `main.rs`'s startup `try_provider!` macro call plus two
reconnect `match` arms, `providers.rs`'s `PROVIDERS` const (drives the `/connect` picker), and
`migration.rs`'s `KNOWN_PROVIDERS` (first-launch keyring scan). No changes to `otto-host`,
`otto-protocol`, or any other provider crate.

**Tech Stack:** Rust 2024, `reqwest` + `axum` + `rmcp` (same as `provider-deepseek`/`provider-openai`),
`otto-mcp`'s `ProviderHandler`/`InProcessProviderClient`, GitHub CLI for issue/PR/release workflow.

**Spec:** `docs/superpowers/specs/2026-09-11-provider-grok-design.md` — read it first, including
the Assumptions section (provider id `"grok"`, `XAI_API_KEY` key hint, `GROK_BASE_URL`/
`OTTO_GROK_LISTEN` otto-side env vars, the `grok-4`/`grok-4-fast` model catalog, and the
image/imagine/voice `list_models` filter). This plan implements it exactly.

**Release line:** `v0.30.0` (MINOR — new feature, additive per Non-Negotiable Rule 6; current
version is `0.29.1`)

**Branch:** `provider-grok` (already created in this worktree — matches the flat branch-naming
precedent set by `provider-deepseek`'s PR #58, not the `<area>/<slug>` form; do not rename it)

## File Map

**New files**
- `crates/provider-grok/Cargo.toml`
- `crates/provider-grok/src/lib.rs`
- `crates/provider-grok/src/api.rs`
- `crates/provider-grok/src/translate.rs`
- `crates/provider-grok/src/stream.rs`
- `crates/provider-grok/src/mcp.rs`
- `crates/provider-grok/tests/integration.rs`
- `crates/otto/src/bin/otto-grok.rs`
- `crates/otto/src/plugin/builtin/provider_grok/mod.rs`
- `docs/superpowers/specs/2026-09-11-provider-grok-design.md` (already committed)
- `docs/superpowers/plans/2026-09-11-provider-grok.md` (this file)

**Modified files**
- `Cargo.toml` (root) — add `crates/provider-grok` to `members` and `workspace.dependencies`.
- `crates/otto/Cargo.toml` — add `provider-grok` dependency and the `otto-grok` `[[bin]]` target.
  (No `cargo-deb`/`cargo-generate-rpm` asset-list edits — that packaging metadata does not exist in
  this repo's current `crates/otto/Cargo.toml`; it was removed per
  `docs/superpowers/plans/otto-development-drop-deb-rpm`.)
- `crates/otto/src/plugin/builtin/mod.rs` — register the new `provider_grok` submodule.
- `crates/otto/src/plugin/mod.rs` — add `ProviderEntry::new(...ProviderGrokPlugin::new())` and
  update `register_builtins_pr8_complete`'s hardcoded provider/registry counts.
- `crates/otto/src/main.rs` — startup `try_provider!` call + two reconnect `match` arms (three edit
  sites), each with its accompanying `use` import.
- `crates/otto/src/providers.rs` — add the `"grok"` `ProviderSpec` entry.
- `crates/otto/src/migration.rs` — add `"grok"` to `KNOWN_PROVIDERS`.
- `crates/otto/src/plugin/effects.rs` — update `build_connect_candidates` test's expected id set.
- `README.md` — provider count prose, workspace-map table (two rows), routing-example enumeration.

## Task 1: Scaffold the `provider-grok` library crate

**Files:**
- Create: `crates/provider-grok/Cargo.toml`, `src/lib.rs`, `src/api.rs`, `src/translate.rs`,
  `src/stream.rs`, `src/mcp.rs`, `tests/integration.rs`
- Modify: `Cargo.toml` (root)

- [ ] Read `crates/provider-deepseek/{Cargo.toml,src/api.rs,src/translate.rs,src/stream.rs,
      src/mcp.rs,src/lib.rs,tests/integration.rs}` in full — these are the files to mirror.
- [ ] Create `crates/provider-grok/Cargo.toml`: same `[package]` shape as
      `provider-deepseek/Cargo.toml` (`name = "provider-grok"`, `description = "xAI Grok Chat
      Completions API as a Otto SPP provider"`, `[lib] name = "provider_grok" path = "src/lib.rs"`),
      identical `[dependencies]`/`[dev-dependencies]` block (`otto-protocol`, `otto-mcp`,
      `otto-fence`, `anyhow`, `async-trait`, `axum`, `bytes`, `dotenvy`, `futures`, `reqwest`,
      `rmcp`, `serde`, `serde_json`, `thiserror`, `tokio`, `tracing`, `tracing-subscriber`; dev:
      `tokio` with `macros`/`rt-multi-thread`, `axum` with `json`).
- [ ] Copy `provider-deepseek/src/api.rs` to `provider-grok/src/api.rs` verbatim (Grok's
      chat-completion request/response schema is identical to OpenAI's/DeepSeek's) — only update
      doc comments that say "DeepSeek" to say "Grok"/"xAI".
- [ ] Copy `provider-deepseek/src/stream.rs` to `provider-grok/src/stream.rs` verbatim, updating
      doc comments only. This is a plain SSE-to-`StreamEvent` adapter with no `ProgressDispatcher`
      involvement (that pattern lives in `otto-host`, confirmed during spec review) — no forwarder
      logic to preserve here beyond the existing copy-verbatim correctness.
- [ ] Copy `provider-deepseek/src/translate.rs` to `provider-grok/src/translate.rs`, renaming
      `request_to_deepseek`/`response_from_deepseek` to `request_to_grok`/`response_from_grok` (and
      updating any internal type names that reference renamed `api::` items — types keep their
      shape, only public fn names change). Do **not** add a `"thinking"`/reasoning-effort field to
      the outgoing request — per the spec's Scope > Out, this crate never enables a reasoning mode.
- [ ] Copy `provider-deepseek/src/mcp.rs` to `provider-grok/src/mcp.rs`, renaming
      `DeepSeekMcpServer` to `GrokMcpServer` and updating its `from_shared` constructor's type bound
      to `GrokProvider`. Update this file's embedded `#[cfg(test)]` module's mock fixture ids from
      DeepSeek's (`deepseek-v4-flash`, `deepseek-v4-pro`) to Grok's (`grok-4`, `grok-4-fast`) and its
      mock route path to match Task 1's `CHAT_COMPLETIONS_PATH`/`list_models` route below.
- [ ] Write `crates/provider-grok/src/lib.rs` by copying `provider-deepseek/src/lib.rs` and
      substituting: `DeepSeekProvider`/`DeepSeekProviderBuilder` → `GrokProvider`/
      `GrokProviderBuilder`; `DEFAULT_BASE_URL = "https://api.x.ai/v1"`; `CHAT_COMPLETIONS_PATH =
      "/chat/completions"`; `DEFAULT_MODEL = "grok-4"`; `DEEPSEEK_API_KEY` → `XAI_API_KEY`;
      `list_models`'s `GET /models` stays `GET /models` (relative to `DEFAULT_BASE_URL`, i.e.
      `GET https://api.x.ai/v1/models`) but add a filter excluding any returned model id containing
      the substring `"image"`, `"imagine"`, or `"voice"` (case-insensitive) — per spec Assumption 5,
      xAI's catalog mixes non-chat model families into the same id namespace; `OTTO_DEEPSEEK_LISTEN`/
      `DEFAULT_LISTEN` → `OTTO_GROK_LISTEN` / `"127.0.0.1:8791"`; `DEEPSEEK_BASE_URL` →
      `GROK_BASE_URL`; doc comments updated to say Grok/xAI. Keep
      `translate::request_to_grok`/`response_from_grok` call sites matching this task's renamed
      `translate.rs` functions. Keep the module doc's crate-layout list, `#![forbid(unsafe_code)]`,
      `#![warn(missing_docs)]`, and error-mapping helpers (`map_reqwest_error`, `http_status_error`,
      `parse_error_response`) unchanged in shape.
- [ ] Adjust the copied `list_models_tests` module (in `lib.rs`, same as `provider-deepseek`'s):
      keep the HTTP-401 error-propagation test and the default-model-id present/absent tests using
      `grok-4`/`grok-4-fast` mock fixture ids; add a new test asserting the image/imagine/voice
      filter — mock server returns a mixed list (e.g. `grok-4`, `grok-4-fast`, `grok-imagine-image`,
      `grok-voice-think-fast`) and `list_models()` must return only the two chat-capable ids.
- [ ] Copy `provider-deepseek/tests/integration.rs` to `provider-grok/tests/integration.rs`,
      updating provider type names and mock fixture ids to `grok-4`/`grok-4-fast`; the mock
      chat-completions path stays `/chat/completions` (same as DeepSeek's, no `/v1` prefix on the
      mock route since `DEFAULT_BASE_URL` already carries `/v1`).
- [ ] Add `crates/provider-grok` to root `Cargo.toml`'s `[workspace] members` (alphabetical slot:
      after `crates/provider-gemini`, before `crates/provider-local`) and to
      `workspace.dependencies` (`provider-grok = { path = "crates/provider-grok", version =
      "0.29.1" }` — matching every other workspace-dependency entry's *current* pinned version, not
      the next release's; `provider-grok/Cargo.toml`'s own `[package] version.workspace = true`
      also resolves to `0.29.1` at this point, so `path` + `version` stay consistent. The bump to
      `0.30.0` happens workspace-wide in Task 5's follow-up release PR, not here — pinning this one
      new entry ahead of that bump would be a `path`+`version` mismatch Cargo rejects at resolve
      time), same alphabetical slot.
- [ ] Run targeted validation: `cargo test -p provider-grok`. Expect all unit + integration tests
      to pass.
- [ ] Run `cargo build -p provider-grok --all-targets` to confirm the new crate compiles cleanly on
      its own before wiring it into `crates/otto`.
- [ ] Public-interface check: this task adds a new library crate and new optional env vars
      (`XAI_API_KEY`, `GROK_BASE_URL`, `OTTO_GROK_LISTEN`) — additive only, per the spec's
      Non-Negotiable Rule 6 analysis. No existing interface changes shape.
- [ ] Host-swap `RwLock` check: not applicable — this task touches no `app.rs`/`tui.rs` code.
- [ ] ProgressDispatcher check: the `ProgressDispatcher` forwarder-abort pattern lives in
      `crates/otto-host/src/provider.rs`'s `RmcpProviderClient` (the opt-in MCP-over-HTTP transport
      path), not in any provider crate's `stream.rs`. This task does not touch `crates/otto-host` at
      all, so the invariant is preserved by construction; confirm `git diff --stat` for this task's
      commit shows no changes under `crates/otto-host/`.
- [ ] Format and commit: `cargo fmt --all` then
      `git commit -m "provider-grok: add xAI Grok Chat Completions provider crate"`.

## Task 2: Add the built-in `ProviderGrokPlugin` provider shim

**Files:**
- Create: `crates/otto/src/plugin/builtin/provider_grok/mod.rs`
- Modify: `crates/otto/src/plugin/builtin/mod.rs`
- Modify: `crates/otto/Cargo.toml`
- Create: `crates/otto/src/bin/otto-grok.rs`

- [ ] Read `crates/otto/src/plugin/builtin/provider_deepseek/mod.rs` in full — this is the file to
      mirror.
- [ ] Write failing tests first in a new `crates/otto/src/plugin/builtin/provider_grok/mod.rs`:
      copy `provider_deepseek/mod.rs`'s five `#[cfg(test)] mod tests` functions
      (`no_creds_emits_prompt_api_key`, `handle_slash_with_stored_key_skips_modal`,
      `handle_slash_with_rekey_flag_opens_modal_even_when_client_exists`,
      `manifest_declares_provider_and_slash`, `render_slot_marks_active_provider`), renaming
      `ProviderDeepSeekPlugin` → `ProviderGrokPlugin` and the keyring/provider-id string
      `"deepseek"` → `"grok"` throughout. These will fail to compile until the production code
      below exists.
- [ ] Implement the production code in the same file: `PLUGIN_ID = "internal:provider-grok"`,
      `PROVIDER_ID = "grok"`, `DISPLAY_NAME = "xAI Grok"`; `ProviderGrokPlugin` struct, `new()`,
      `#[cfg(test)] with_test_client`, `#[cfg(test)] set_active_for_render`; `capabilities()`
      returning two `ModelCapabilities` entries (`grok-4` — "Grok 4", `supports_vision: true`,
      `supports_audio: false`, `context_window: 256_000`, `CostTier::Standard`; `grok-4-fast` —
      "Grok 4 Fast", `supports_vision: true`, `supports_audio: false`, `context_window:
      2_000_000`, `CostTier::Cheap`), default model id `"grok-4"`; `try_build_registration`
      (keyring read → `GrokProvider::builder()` → `InProcessProviderClient` → `build_dynamic_caps`
      → `ProviderRegistration` with aliases `"grok"` → `grok-4` and `"grok-fast"` → `grok-4-fast`);
      `try_connect_from_keyring`; the `Plugin` impl (`manifest`, `handle_slash`, `on_event`,
      `render_slot`) and `BuiltinProviderPlugin::take_client`, all copied from
      `provider_deepseek/mod.rs`'s shape with the constants above substituted.
- [ ] Register the new submodule in `crates/otto/src/plugin/builtin/mod.rs` (add `pub(crate) mod
      provider_grok;` alongside the existing module declarations, in the same
      alphabetical/insertion-order convention already used there).
- [ ] Add `provider-grok = { workspace = true }` to `crates/otto/Cargo.toml`'s `[dependencies]`
      (alphabetical slot, after `provider-gemini`, before `provider-local`) — required before the
      plugin module above will compile.
- [ ] Add a `[[bin]] name = "otto-grok" path = "src/bin/otto-grok.rs"` target to
      `crates/otto/Cargo.toml`, inserted after the `otto-deepseek` bin target.
- [ ] Create `crates/otto/src/bin/otto-grok.rs` as a 10-line shim copied from
      `otto-deepseek.rs`, calling `provider_grok::run()`.
- [ ] Run targeted tests: `cargo test -p otto -- provider_grok::`. Expect all five tests to pass.
- [ ] Run `cargo build -p otto --bin otto-grok` to confirm the new shim binary compiles and links
      against the new crate.
- [ ] Public-interface check: a new built-in provider plugin id (`internal:provider-grok`) and a
      new shipped binary (`otto-grok`) — additive.
- [ ] Host-swap `RwLock` check: not applicable — this plugin module holds no `Arc<RwLock<...>>`
      state of its own; it only constructs a client and hands it to the runtime via
      `BuiltinProviderPlugin::take_client`, same as every existing provider plugin.
- [ ] ProgressDispatcher check: not applicable at this layer — already verified at the crate level
      in Task 1.
- [ ] Format and commit: `cargo fmt --all` then
      `git commit -m "otto: add ProviderGrokPlugin built-in provider shim"`.

## Task 3: Wire Grok into every provider-catalog call site

**Files:**
- Modify: `crates/otto/src/plugin/mod.rs`
- Modify: `crates/otto/src/main.rs`
- Modify: `crates/otto/src/providers.rs`
- Modify: `crates/otto/src/migration.rs`
- Modify: `crates/otto/src/plugin/effects.rs`

- [ ] Write/adjust the failing test first in `crates/otto/src/plugin/effects.rs`: extend the
      `build_connect_candidates` test's `for expected in [...]` array to include `"grok"` alongside
      `"anthropic"`, `"gemini"`, `"openai"`, `"deepseek"`, `"local"`.
- [ ] Run the targeted test before implementation: `cargo test -p otto -- build_connect_candidates`
      (or the exact test function name in that module). Expect failure — `"grok"` is not yet a
      connect candidate.
- [ ] In `crates/otto/src/plugin/mod.rs`, add
      `ProviderEntry::new(builtin::provider_grok::ProviderGrokPlugin::new())` to the `Vec` that
      currently registers every other built-in — insert it immediately after the
      `provider_deepseek` entry.
- [ ] In that same file's `register_builtins_pr8_complete` test, update the hardcoded counts this
      new entry invalidates (confirmed current values by reading the live file at plan time):
      add `"internal:provider-grok"` to the `provider_ids` expected-id array (alongside
      `"internal:provider-anthropic"`, `"internal:provider-openai"`, `"internal:provider-gemini"`,
      `"internal:provider-local"`, `"internal:provider-deepseek"`); change
      `assert_eq!(set.providers.len(), 5)` to `6`; change `assert_eq!(reg.len(), 35, "registry
      should have 29 non-provider + 5 provider + 1 hook plugin")` to `36` with the message updated
      to "29 non-provider + 6 provider + 1 hook plugin"; change `assert_eq!(reg.provider_count(), 5,
      "registry should have 5 provider plugins")` to `6` with the message updated to "6 provider
      plugins"; append a line to the preceding comment block noting "the Grok provider shim adds a
      6th provider plugin; total registry size is 29 + 6 + 1 = 36."
- [ ] In `crates/otto/src/main.rs`'s `bootstrap_pool_host`, add `provider_grok::ProviderGrokPlugin`
      to the `use crate::plugin::builtin::{...}` import list and add
      `try_provider!(ProviderGrokPlugin::new(), "xAI Grok", "grok");` immediately after the
      `ProviderDeepSeekPlugin` line.
- [ ] In both of `crates/otto/src/main.rs`'s reconnect `match` statements (the ones containing
      `"deepseek" => ProviderDeepSeekPlugin::new().try_build_registration().await,`), add the
      corresponding `use` import for `ProviderGrokPlugin` in that function's scope and a `"grok" =>
      ProviderGrokPlugin::new().try_build_registration().await,` arm immediately after each existing
      `"deepseek"` arm.
- [ ] In `crates/otto/src/providers.rs`, add a `ProviderSpec` entry to the `PROVIDERS` const,
      inserted after the `"deepseek"` entry and before the `"local"` entry:
      ```rust
      ProviderSpec {
          id: "grok",
          display_name: "xAI Grok",
          api_key_env: "XAI_API_KEY",
          default_model: "grok-4",
          api_key_required: true,
      },
      ```
- [ ] In `crates/otto/src/migration.rs`, add `"grok"` to the `KNOWN_PROVIDERS` const array, after
      `"deepseek"`.
- [ ] Run targeted validation after implementation: `cargo test -p otto -- build_connect_candidates`
      (expect pass); `cargo test -p otto -- register_builtins_pr8_complete` (expect pass with the
      updated counts); `cargo test -p otto -- migration::` (expect existing migration tests still
      pass — `KNOWN_PROVIDERS` growing by one entry must not change any fixed-count assertion; if
      any test hardcodes the provider-list length, update it alongside this change); `cargo test -p
      otto -- providers::` (expect `effective_providers_includes_builtins` to still pass with six
      built-ins).
- [ ] Grep for any other hardcoded provider-id enumeration that might need the same addition:
      `rg -n '"anthropic".*"gemini".*"openai"|KNOWN_PROVIDERS|PROVIDERS: &\[' crates/otto/src` —
      confirm every match found is already covered by this task's edits, or extend this task's edit
      list if a new call site turns up.
- [ ] Run full workspace tests: `cargo test --workspace`. Expect all tests to pass, including
      `provider-grok`'s own suite from Task 1 and `provider_grok`'s plugin tests from Task 2.
- [ ] Public-interface check: additive only — a new provider id surfaces in `/connect`, `/model`,
      `/use`, and `@provider:model` routing; no existing provider id, tool schema, or slash command
      changes shape.
- [ ] Host-swap `RwLock` check: `main.rs`'s `bootstrap_pool_host` and reconnect match arms run
      before any `Arc<RwLock<Option<Arc<Host>>>>` guard is held (pre-host-construction /
      pool-registration paths) — verify no `.await` in this task's diff executes while holding such
      a guard; these edits are synchronous list/match additions plus an already-`.await`-based
      `try_build_registration()` call using the exact same pattern as every sibling provider already
      in that code path.
- [ ] Run full required validation: `cargo build --workspace --all-targets`; `cargo test
      --workspace`; `cargo clippy --workspace --all-targets`; `cargo fmt --all --check`. Expect all
      four commands to pass with `RUSTFLAGS=-D warnings` cleanliness preserved.
- [ ] Format and commit: `cargo fmt --all` then
      `git commit -m "otto: register xAI Grok across the provider catalog"`.

## Task 4: Document Grok in README.md

**Files:**
- Modify: `README.md`

- [ ] Update the Install section's provider-count prose (README lines ~14-19 at plan time): "Each
      release ships one archive per platform containing ten binaries ... and four standalone
      provider MCP servers (`otto-anthropic`, `otto-gemini`, `otto-openai`, `otto-deepseek`)" becomes
      "... containing **eleven** binaries ... and **five** standalone provider MCP servers
      (`otto-anthropic`, `otto-gemini`, `otto-openai`, `otto-deepseek`, `otto-grok`)". Confirm the
      exact current wording by reading the live file first — do not assume it still says "ten"/"four"
      if an intervening change has already shifted the count.
- [ ] Update the `crates/otto` workspace-map table row (README ~line 56 at plan time): "All ten
      shipping binaries (`otto` TUI plus the `otto-tool-{fs,bash,grep,lsp,web}` tool shims and the
      `otto-{anthropic,gemini,openai,deepseek}` provider shims)" becomes "All **eleven** shipping
      binaries (... and the `otto-{anthropic,gemini,openai,deepseek,grok}` provider shims)".
- [ ] Add a `crates/provider-grok` row to the workspace-map table immediately after the
      `crates/provider-deepseek` row: `| [\`crates/provider-grok\`](crates/provider-grok) | xAI Grok
      Chat Completions, same shape (OpenAI-compatible wire format). |`.
- [ ] Grep for any provider-id enumeration in prose/examples that lists all built-ins (not
      single-provider illustrative examples): `rg -n 'anthropic.*gemini.*openai|gemini.*openai.*local|deepseek'
      README.md` — add a `grok` mention to each true enumeration found; leave single-provider
      illustrative examples unchanged since they only need one example provider, not an exhaustive
      list.
- [ ] Grep for a "standalone provider" binary-invocation section or env-var table that enumerates
      `otto-openai`/`otto-deepseek` and their env vars: `rg -n 'otto-deepseek|DEEPSEEK_BASE_URL|OTTO_DEEPSEEK_LISTEN'
      README.md` — add the corresponding `otto-grok` / `XAI_API_KEY` / `GROK_BASE_URL` /
      `OTTO_GROK_LISTEN` row(s) to any such table/section found, and correct any "N shipping
      binaries" count in the same table if this addition changes it.
- [ ] No test command applies to a documentation-only task; visually diff the rendered table
      (`git diff README.md`) to confirm no unrelated row was altered.
- [ ] Format and commit: `git commit -m "docs: document the xAI Grok provider in README"` (no
      `cargo fmt` needed — Markdown only).

## Task 5: Record the release follow-through required after merge

**Files:**
- No feature-branch code changes; this task records the mandatory release follow-through.

- [ ] Note in the feature PR body and release handoff that a dedicated release PR must be opened
      immediately after merge to bump `workspace.package.version` and all internal
      `workspace.dependencies` versions (including the new `provider-grok` entry) to `0.30.0`, add a
      `CHANGELOG.md` entry describing the new xAI Grok provider as an added feature, run the
      required validation (`cargo build --workspace --all-targets`, `cargo test --workspace`,
      `cargo clippy --workspace --all-targets`, `cargo fmt --all --check`), and ship the `v0.30.0`
      tag per `RELEASING.md`.

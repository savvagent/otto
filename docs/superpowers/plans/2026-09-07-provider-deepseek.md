# provider-deepseek Implementation Plan

**Goal:** Add DeepSeek as a fifth built-in provider (`deepseek-v4-flash` / `deepseek-v4-pro`),
selectable from `/connect`, `/model`, `/use`, and the `@provider:model` routing prefix, by mirroring
`provider-openai`'s shape end-to-end — a new library crate, a built-in provider plugin, and the
same registration call sites every other built-in provider touches.

**Architecture:** `crates/provider-deepseek` is a new library crate implementing `ProviderHandler`
(`otto-mcp`) against DeepSeek's OpenAI-compatible `POST /chat/completions` endpoint — no new
transport, no turn-loop change, no host-swap `RwLock` interaction. `crates/otto/src/plugin/builtin
/provider_deepseek/mod.rs` is a `Plugin` + `BuiltinProviderPlugin` shim (mirrors
`provider_openai/mod.rs`) that reads `DEEPSEEK_API_KEY` from the OS keyring, builds an
`InProcessProviderClient`, and registers with the host's provider pool exactly like every other
built-in. Five call sites wire the new plugin into the existing catalogs: `plugin/mod.rs`'s
`ProviderEntry` vec, `main.rs`'s startup `try_provider!` macro call plus two reconnect `match`
arms, `providers.rs`'s `PROVIDERS` const (drives the `/connect` picker), and `migration.rs`'s
`KNOWN_PROVIDERS` (first-launch keyring scan). No changes to `otto-host`, `otto-protocol`, or any
other provider crate.

**Tech Stack:** Rust 2024, `reqwest` + `axum` + `rmcp` (same as `provider-openai`), `otto-mcp`'s
`ProviderHandler`/`InProcessProviderClient`, GitHub CLI for issue/PR/release workflow.

**Spec:** `docs/superpowers/specs/2026-09-07-provider-deepseek-design.md` — read it first,
including the "Known Spec Review Notes" section (repo naming is `otto`, not `savvagent`; this
worktree is based on `origin/main` which already has the completed rename). This plan implements
it exactly.

**Release line:** `v0.25.0` (MINOR — new feature, additive per Non-Negotiable Rule 6)

**Branch:** `provider/provider-deepseek` (already created as `provider-deepseek` in this worktree —
branch naming below matches the existing worktree branch name; do not rename it)

## File Map

**New files**
- `crates/provider-deepseek/Cargo.toml`
- `crates/provider-deepseek/src/lib.rs`
- `crates/provider-deepseek/src/api.rs`
- `crates/provider-deepseek/src/translate.rs`
- `crates/provider-deepseek/src/stream.rs`
- `crates/provider-deepseek/src/mcp.rs`
- `crates/provider-deepseek/tests/integration.rs`
- `crates/otto/src/bin/otto-deepseek.rs`
- `crates/otto/src/plugin/builtin/provider_deepseek/mod.rs`
- `docs/superpowers/specs/2026-09-07-provider-deepseek-design.md` (already committed)
- `docs/superpowers/plans/2026-09-07-provider-deepseek.md` (this file)

**Modified files**
- `Cargo.toml` (root) — add `crates/provider-deepseek` to `members` and `workspace.dependencies`.
- `crates/otto/Cargo.toml` — add `provider-deepseek` dependency, `otto-deepseek` `[[bin]]`, and
  `otto-deepseek` rows in both the `cargo deb` and `cargo generate-rpm` asset lists.
- `crates/otto/src/plugin/builtin/mod.rs` — register the new `provider_deepseek` submodule.
- `crates/otto/src/plugin/mod.rs` — add `ProviderEntry::new(...ProviderDeepSeekPlugin::new())`.
- `crates/otto/src/main.rs` — startup `try_provider!` call + two reconnect `match` arms (three
  edit sites), each with its accompanying `use` import.
- `crates/otto/src/providers.rs` — add the `"deepseek"` `ProviderSpec` entry.
- `crates/otto/src/migration.rs` — add `"deepseek"` to `KNOWN_PROVIDERS`.
- `crates/otto/src/plugin/effects.rs` — update `build_connect_candidates` test's expected id set.
- `README.md` — provider count prose, workspace-map table (two rows), routing-example enumeration.

## Task 1: Scaffold the `provider-deepseek` library crate

**Files:**
- Create: `crates/provider-deepseek/Cargo.toml`, `src/lib.rs`, `src/api.rs`, `src/translate.rs`,
  `src/stream.rs`, `src/mcp.rs`, `tests/integration.rs`
- Modify: `Cargo.toml` (root)

- [ ] Read `crates/provider-openai/{Cargo.toml,src/api.rs,src/translate.rs,src/stream.rs,
      src/mcp.rs,src/lib.rs,tests/integration.rs}` in full — these are the files to mirror.
- [ ] Create `crates/provider-deepseek/Cargo.toml`: same `[package]` shape as
      `provider-openai/Cargo.toml` (`name = "provider-deepseek"`, `description = "DeepSeek Chat
      Completions API as a Otto SPP provider"`, `[lib] name = "provider_deepseek" path =
      "src/lib.rs"`), identical `[dependencies]`/`[dev-dependencies]` block (`otto-protocol`,
      `otto-mcp`, `otto-fence`, `anyhow`, `async-trait`, `axum`, `bytes`, `dotenvy`, `futures`,
      `reqwest`, `rmcp`, `serde`, `serde_json`, `thiserror`, `tokio`, `tracing`,
      `tracing-subscriber`; dev: `tokio` with `macros`/`rt-multi-thread`, `axum` with `json`).
- [ ] Copy `provider-openai/src/api.rs` to `provider-deepseek/src/api.rs` verbatim (DeepSeek's
      chat-completion request/response schema is identical to OpenAI's) — only update doc
      comments that say "OpenAI" to say "DeepSeek".
- [ ] Copy `provider-openai/src/stream.rs` to `provider-deepseek/src/stream.rs` verbatim, updating
      doc comments only. Verify (and preserve) any `JoinHandle::abort()` /
      `ProgressDispatcher`-forwarder pattern present in the source file byte-for-byte in behavior.
- [ ] Copy `provider-openai/src/translate.rs` to `provider-deepseek/src/translate.rs`, renaming
      `request_to_openai`/`response_from_openai` to `request_to_deepseek`/
      `response_from_deepseek` (and updating any internal type names that reference `api::` items
      renamed in the previous steps — types themselves keep their shape, only the module's public
      fn names change to avoid confusingly-OpenAI-branded names in a DeepSeek crate). Do **not**
      add a `"thinking"` or `reasoning_effort` field to the outgoing request struct/serialization —
      per the spec's Non-goals, this crate never enables DeepSeek's thinking mode.
- [ ] Copy `provider-openai/src/mcp.rs` to `provider-deepseek/src/mcp.rs`, renaming
      `OpenAiMcpServer` to `DeepSeekMcpServer` and updating its `from_shared` constructor's type
      bound to `DeepSeekProvider`.
- [ ] Write `crates/provider-deepseek/src/lib.rs` by copying `provider-openai/src/lib.rs` and
      substituting: `OpenAiProvider`/`OpenAiProviderBuilder` → `DeepSeekProvider`/
      `DeepSeekProviderBuilder`; `DEFAULT_BASE_URL = "https://api.deepseek.com"`;
      `CHAT_COMPLETIONS_PATH = "/chat/completions"` (no `/v1` prefix); `DEFAULT_MODEL =
      "deepseek-v4-flash"`; `OPENAI_API_KEY` → `DEEPSEEK_API_KEY`; `list_models`'s `GET /v1/models`
      → `GET /models`, and remove the `gpt-`/`o1-`/`o3-`/`o4-` prefix filter — return every model
      DeepSeek's endpoint lists unfiltered; `OTTO_OPENAI_LISTEN`/`DEFAULT_LISTEN` → `OTTO_DEEPSEEK_LISTEN` / `127.0.0.1:8790`; `OPENAI_BASE_URL` → `DEEPSEEK_BASE_URL`; doc comments
      updated to say DeepSeek. Keep `translate::request_to_deepseek`/`response_from_deepseek` call
      sites matching Task 1's renamed `translate.rs` functions. Keep the module doc's crate-layout
      list, `#![forbid(unsafe_code)]`, `#![warn(missing_docs)]`, and error-mapping helpers
      (`map_reqwest_error`, `http_status_error`, `parse_error_response`) unchanged in shape.
- [ ] Adjust the copied `list_models_tests` module (moved into `lib.rs`, same as
      `provider-openai`'s): since DeepSeek's catalog has no non-chat models to filter, rewrite the
      three tests to assert `list_models` returns every id the mock server lists unfiltered (drop
      the `text-embedding-3-small`/`whisper-1` filtering-out assertions; keep the HTTP-401
      error-propagation test and the default-model-id present/absent tests, using
      `deepseek-v4-flash`/`deepseek-v4-pro` ids in the mock fixtures instead of `gpt-4o-mini`/
      `o1-mini`).
- [ ] Copy `provider-openai/tests/integration.rs` to `provider-deepseek/tests/integration.rs`,
      updating provider type names and the mock chat-completions path to
      `/chat/completions` (no `/v1` prefix) to match this crate's `CHAT_COMPLETIONS_PATH`.
- [ ] Add `crates/provider-deepseek` to root `Cargo.toml`'s `[workspace] members` (alphabetical
      slot: after `crates/provider-anthropic`, before `crates/provider-gemini`) and to
      `workspace.dependencies` (`provider-deepseek = { path = "crates/provider-deepseek", version
      = "0.24.0" }`, same alphabetical slot).
- [ ] Run targeted validation: `cargo test -p provider-deepseek`. Expect all unit + integration
      tests to pass.
- [ ] Run `cargo build -p provider-deepseek --all-targets` to confirm the new crate compiles
      cleanly on its own before wiring it into `crates/otto`.
- [ ] Public-interface check: this task adds a new library crate and a new optional env var
      (`DEEPSEEK_API_KEY`, `DEEPSEEK_BASE_URL`, `OTTO_DEEPSEEK_LISTEN`) — additive only, per the
      spec's Non-Negotiable Rule 6 analysis. No existing interface changes shape.
- [ ] Host-swap `RwLock` check: not applicable — this task touches no `app.rs`/`tui.rs` code.
- [ ] ProgressDispatcher check: `stream.rs` is a verbatim copy of `provider-openai`'s SSE consumer;
      confirm by diff (`diff crates/provider-openai/src/stream.rs
      crates/provider-deepseek/src/stream.rs`, ignoring doc-comment-only lines) that no forwarder-
      abort logic was dropped or altered.
- [ ] Format and commit: `cargo fmt --all` then
      `git commit -m "provider-deepseek: add DeepSeek Chat Completions provider crate"`.

## Task 2: Add the built-in `ProviderDeepSeekPlugin` provider shim

**Files:**
- Create: `crates/otto/src/plugin/builtin/provider_deepseek/mod.rs`
- Modify: `crates/otto/src/plugin/builtin/mod.rs`
- Modify: `crates/otto/Cargo.toml`
- Create: `crates/otto/src/bin/otto-deepseek.rs`

- [ ] Read `crates/otto/src/plugin/builtin/provider_openai/mod.rs` in full — this is the file to
      mirror.
- [ ] Write failing tests first in a new
      `crates/otto/src/plugin/builtin/provider_deepseek/mod.rs`: copy
      `provider_openai/mod.rs`'s five `#[cfg(test)] mod tests` functions
      (`no_creds_emits_prompt_api_key`, `handle_slash_with_stored_key_skips_modal`,
      `handle_slash_with_rekey_flag_opens_modal_even_when_client_exists`,
      `manifest_declares_provider_and_slash`, `render_slot_marks_active_provider`), renaming
      `ProviderOpenAiPlugin` → `ProviderDeepSeekPlugin` and the keyring/provider-id string
      `"openai"` → `"deepseek"` throughout. These will fail to compile until the production code
      below exists.
- [ ] Implement the production code in the same file: `PLUGIN_ID =
      "internal:provider-deepseek"`, `PROVIDER_ID = "deepseek"`, `DISPLAY_NAME = "DeepSeek"`;
      `ProviderDeepSeekPlugin` struct, `new()`, `#[cfg(test)] with_test_client`,
      `#[cfg(test)] set_active_for_render`; `capabilities()` returning two
      `ModelCapabilities` entries (`deepseek-v4-flash` — "DeepSeek V4 Flash", no vision, no audio,
      128,000 context window, `CostTier::Cheap`; `deepseek-v4-pro` — "DeepSeek V4 Pro", no vision,
      no audio, 128,000 context window, `CostTier::Standard`), default model id
      `"deepseek-v4-flash"`; `try_build_registration` (keyring read → `DeepSeekProvider::builder()`
      → `InProcessProviderClient` → `build_dynamic_caps` → `ProviderRegistration` with aliases
      `"deepseek"` → `deepseek-v4-flash` and `"deepseek-pro"` → `deepseek-v4-pro`);
      `try_connect_from_keyring`; the `Plugin` impl (`manifest`, `handle_slash`, `on_event`,
      `render_slot`) and `BuiltinProviderPlugin::take_client`, all copied from
      `provider_openai/mod.rs`'s shape with the constants above substituted.
- [ ] Register the new submodule in `crates/otto/src/plugin/builtin/mod.rs` (add `pub(crate) mod
      provider_deepseek;` alongside the existing `provider_openai`/`provider_gemini` module
      declarations, in the same alphabetical/insertion-order convention already used there).
- [ ] Add `provider-deepseek = { workspace = true }` to `crates/otto/Cargo.toml`'s
      `[dependencies]` (alphabetical slot, after `provider-anthropic`, before `provider-gemini`) —
      required before the plugin module above will compile.
- [ ] Add a `[[bin]] name = "otto-deepseek" path = "src/bin/otto-deepseek.rs"` target to
      `crates/otto/Cargo.toml`, inserted after the `otto-openai` bin target.
- [ ] Create `crates/otto/src/bin/otto-deepseek.rs` as a 10-line shim copied from
      `otto-openai.rs`, calling `provider_deepseek::run()`.
- [ ] Add `otto-deepseek` rows to both `crates/otto/Cargo.toml`'s `[package.metadata.deb]`
      `assets` list and `[package.metadata.generate-rpm]` `assets` list, matching the existing
      `otto-anthropic`/`otto-gemini`/`otto-openai` row shape, inserted after the `otto-openai` row
      in each list.
- [ ] Run targeted tests: `cargo test -p otto -- provider_deepseek::`. Expect all five tests to
      pass.
- [ ] Run `cargo build -p otto --bin otto-deepseek` to confirm the new shim binary compiles and
      links against the new crate.
- [ ] Public-interface check: a new built-in provider plugin id (`internal:provider-deepseek`) and
      a new shipped binary (`otto-deepseek`) — additive.
- [ ] Host-swap `RwLock` check: not applicable — this plugin module holds no `Arc<RwLock<...>>`
      state of its own; it only constructs a client and hands it to the runtime via
      `BuiltinProviderPlugin::take_client`, same as every existing provider plugin.
- [ ] ProgressDispatcher check: not applicable at this layer — already verified at the crate level
      in Task 1.
- [ ] Format and commit: `cargo fmt --all` then
      `git commit -m "otto: add ProviderDeepSeekPlugin built-in provider shim"`.

## Task 3: Wire DeepSeek into every provider-catalog call site

**Files:**
- Modify: `crates/otto/src/plugin/mod.rs`
- Modify: `crates/otto/src/main.rs`
- Modify: `crates/otto/src/providers.rs`
- Modify: `crates/otto/src/migration.rs`
- Modify: `crates/otto/src/plugin/effects.rs`

- [ ] Write/adjust the failing test first in `crates/otto/src/plugin/effects.rs`: extend the
      `build_connect_candidates` test's `for expected in [...]` array to include `"deepseek"`
      alongside `"anthropic"`, `"gemini"`, `"openai"`, `"local"`.
- [ ] Run the targeted test before implementation: `cargo test -p otto -- build_connect_candidates`
      (or the exact test function name in that module). Expect failure — `"deepseek"` is not yet a
      connect candidate.
- [ ] In `crates/otto/src/plugin/mod.rs`, add
      `ProviderEntry::new(builtin::provider_deepseek::ProviderDeepSeekPlugin::new())` to the `Vec`
      that currently registers `provider_anthropic`, `provider_openai`, `provider_gemini`,
      `provider_local` — insert it immediately after the `provider_openai` entry.
- [ ] In `crates/otto/src/main.rs`'s `bootstrap_pool_host`, add
      `provider_deepseek::ProviderDeepSeekPlugin` to the `use crate::plugin::builtin::{...}`
      import list and add `try_provider!(ProviderDeepSeekPlugin::new(), "DeepSeek", "deepseek");`
      immediately after the `ProviderOpenAiPlugin` line.
- [ ] In both of `crates/otto/src/main.rs`'s reconnect `match` statements (the ones currently
      containing `"openai" => ProviderOpenAiPlugin::new().try_build_registration().await,` near
      the file's `/connect <id>` and reconnect-on-model-switch paths), add the corresponding `use`
      import for `ProviderDeepSeekPlugin` in that function's scope and a `"deepseek" =>
      ProviderDeepSeekPlugin::new().try_build_registration().await,` arm immediately after each
      existing `"openai"` arm.
- [ ] In `crates/otto/src/providers.rs`, add a `ProviderSpec` entry to the `PROVIDERS` const,
      inserted after the `"openai"` entry and before the `"local"` entry:
      ```rust
      ProviderSpec {
          id: "deepseek",
          display_name: "DeepSeek",
          api_key_env: "DEEPSEEK_API_KEY",
          default_model: "deepseek-v4-flash",
          api_key_required: true,
      },
      ```
- [ ] In `crates/otto/src/migration.rs`, add `"deepseek"` to the `KNOWN_PROVIDERS` const array,
      after `"openai"`.
- [ ] Run targeted validation after implementation: `cargo test -p otto -- build_connect_candidates`
      (expect pass); `cargo test -p otto -- migration::` (expect existing migration tests still
      pass — `KNOWN_PROVIDERS` growing by one entry must not change any fixed-count assertion; if
      any test hardcodes the four-provider list length, update it alongside this change);
      `cargo test -p otto -- providers::` (expect `effective_providers_includes_builtins` to still
      pass with five built-ins).
- [ ] Grep for any other hardcoded provider-id enumeration that might need the same addition:
      `rg -n '"anthropic".*"gemini".*"openai"|KNOWN_PROVIDERS|PROVIDERS: &\[' crates/otto/src` —
      confirm every match found is already covered by this task's edits, or extend this task's
      edit list if a new call site turns up.
- [ ] Run full workspace tests: `cargo test --workspace`. Expect all tests to pass, including
      `provider-deepseek`'s own suite from Task 1 and `provider_deepseek`'s plugin tests from
      Task 2.
- [ ] Public-interface check: additive only — a new provider id surfaces in `/connect`, `/model`,
      `/use`, and `@provider:model` routing; no existing provider id, tool schema, or slash command
      changes shape.
- [ ] Host-swap `RwLock` check: `main.rs`'s `bootstrap_pool_host` and reconnect match arms run
      before any `Arc<RwLock<Option<Arc<Host>>>>` guard is held (these are pre-host-construction /
      pool-registration paths) — verify no `.await` in this task's diff executes while holding
      such a guard; these edits are synchronous list/match additions plus an
      already-`.await`-based `try_build_registration()` call using the exact same pattern as the
      three sibling providers already in that code path.
- [ ] Run full required validation: `cargo build --workspace --all-targets`; `cargo test
      --workspace`; `cargo clippy --workspace --all-targets`; `cargo fmt --all --check`. Expect all
      four commands to pass with `RUSTFLAGS=-D warnings` cleanliness preserved.
- [ ] Format and commit: `cargo fmt --all` then
      `git commit -m "otto: register DeepSeek across the provider catalog"`.

## Task 4: Document DeepSeek in README.md

**Files:**
- Modify: `README.md`

- [ ] Update the provider-count prose from "three standalone provider MCP servers" to "four
      standalone provider MCP servers".
- [ ] Update the `crates/otto` workspace-map table row to list
      `otto-{anthropic,gemini,openai,deepseek}` instead of `otto-{anthropic,gemini,openai}`.
- [ ] Add a `crates/provider-deepseek` row to the workspace-map table immediately after the
      `crates/provider-openai` row: `| [\`crates/provider-deepseek\`](crates/provider-deepseek) |
      DeepSeek Chat Completions, same shape (OpenAI-compatible wire format). |`.
- [ ] Grep for any provider-id enumeration in prose/examples that lists all built-ins (not
      single-provider illustrative examples): `rg -n 'anthropic.*gemini.*openai|gemini.*openai.*local'
      README.md` — add a `deepseek` mention to each true enumeration found; leave single-provider
      illustrative examples (e.g. `@gemini explain this`) unchanged since they only need one
      example provider, not an exhaustive list.
- [ ] No test command applies to a documentation-only task; visually diff the rendered table
      (`git diff README.md`) to confirm no unrelated row was altered.
- [ ] Format and commit: `git commit -m "docs: document the DeepSeek provider in README"` (no
      `cargo fmt` needed — Markdown only).

## Task 5: Record the release follow-through required after merge

**Files:**
- No feature-branch code changes; this task records the mandatory release follow-through.

- [ ] Note in the feature PR body and release handoff that a dedicated release PR must be opened
      immediately after merge to bump `workspace.package.version` and all internal
      `workspace.dependencies` versions (including the new `provider-deepseek` entry) to
      `0.25.0`, add a `CHANGELOG.md` entry describing the new DeepSeek provider as an added
      feature, run the required validation (`cargo build --workspace --all-targets`, `cargo test
      --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --all --check`), and ship
      the `v0.25.0` tag per `RELEASING.md`.

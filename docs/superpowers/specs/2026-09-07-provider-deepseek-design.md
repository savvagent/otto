# provider-deepseek Design

> **Status:** DRAFT

**Ref:** savvagent/otto#57 — "Add provider for DeepSeek"

## Problem

Otto ships built-in providers for Anthropic, Google Gemini, OpenAI, and local Ollama. DeepSeek
(`deepseek-chat` / `deepseek-reasoner`) is a popular, inexpensive hosted model family that is not
currently selectable via `/connect`. DeepSeek's `POST /chat/completions` endpoint is
wire-compatible with OpenAI's Chat Completions API (bearer auth, same request/response/SSE shape),
so this is an additive "one more provider" change, not a new transport or turn-loop concept.

## Goal

Add DeepSeek as a fifth built-in provider, selectable from `/connect`, `/model`, `/use`, and the
`@provider:model` routing prefix, following the exact shape of `provider-openai` (the closest
existing analog since DeepSeek's API is OpenAI-compatible).

## Non-goals

- No special handling of `deepseek-reasoner`'s `reasoning_content` field in the chat-completion
  response — it is out of scope for this change; a future spec can add reasoning-content surfacing
  if wanted. This change treats DeepSeek's SSE/JSON shape as a plain OpenAI-compatible responder.
- No changes to `Host`, `ToolRegistry`, the SPP wire format, or any other provider.
- No changes to the host-swap `RwLock` discipline or the `rmcp` `ProgressDispatcher` pattern beyond
  replicating the existing (correct) pattern already used by `provider-openai`/`provider-gemini`.

## Design

### New library crate: `crates/provider-deepseek`

Mirrors `crates/provider-openai` file-for-file:

- `src/api.rs` — typed mirror of DeepSeek's `POST /chat/completions` request/response shapes
  (identical schema to OpenAI's; copy `provider-openai`'s `api.rs` verbatim, only doc comments
  change).
- `src/translate.rs` — SPP ⇄ `api` type conversions (copy `provider-openai`'s, adjusted for the
  crate's own request/response types).
- `src/stream.rs` — SSE → SPP `StreamEvent` adapter (copy `provider-openai`'s verbatim; DeepSeek's
  SSE framing is identical to OpenAI's `data: {...}\n\n` / `data: [DONE]` shape).
- `src/mcp.rs` — `ProviderHandler` MCP server wrapper (`DeepSeekMcpServer`, mirrors
  `OpenAiMcpServer`).
- `src/lib.rs` — `DeepSeekProvider` / `DeepSeekProviderBuilder`, `DEFAULT_BASE_URL =
  "https://api.deepseek.com"`, `CHAT_COMPLETIONS_PATH = "/chat/completions"` (no `/v1` prefix —
  DeepSeek's endpoint is `https://api.deepseek.com/chat/completions`, confirmed against DeepSeek's
  published API reference), `DEFAULT_MODEL = "deepseek-chat"`. Builder reads `DEEPSEEK_API_KEY`
  from the environment when no explicit key is supplied — same pattern as
  `OpenAiProviderBuilder::build` reading `OPENAI_API_KEY`.
  - `list_models` calls `GET /models` (DeepSeek does not have OpenAI's `/v1/models` non-chat model
    clutter — its catalog only contains `deepseek-chat` and `deepseek-reasoner` — so no filtering
    by id prefix is needed, unlike `provider-openai`'s `gpt-`/`o1-`/`o3-`/`o4-` allowlist; return
    every model DeepSeek's endpoint lists).
  - `run()` reads `DEEPSEEK_API_KEY`, `OTTO_DEEPSEEK_LISTEN` (default
    `127.0.0.1:8790` — next free port after `provider-openai`'s `8789`), and `DEEPSEEK_BASE_URL`
    from the environment for the standalone binary.
- `tests/integration.rs` — mirrors `provider-openai`'s integration test, pointed at a local mock
  server.

`Cargo.toml`: same dependency set as `provider-openai`'s (`otto-protocol`, `otto-mcp`,
`otto-fence`, `anyhow`, `async-trait`, `axum`, `bytes`, `dotenvy`, `futures`, `reqwest`, `rmcp`,
`serde`, `serde_json`, `thiserror`, `tokio`, `tracing`, `tracing-subscriber`; dev-dependencies
`tokio` with `macros`/`rt-multi-thread`, `axum` with `json`).

### Workspace wiring

- `Cargo.toml` (root): add `crates/provider-deepseek` to `[workspace] members` and
  `workspace.dependencies` (`provider-deepseek = { path = "crates/provider-deepseek", version =
  "0.24.0" }`), alphabetically after `provider-anthropic`... before `provider-gemini` to match the
  existing alphabetical ordering (`anthropic`, `deepseek`, `gemini`, `local`, `openai`).
- `crates/otto/Cargo.toml`: add `provider-deepseek = { workspace = true }` dependency (alphabetical
  slot), a new `[[bin]] name = "otto-deepseek" path = "src/bin/otto-deepseek.rs"` target, and add
  `otto-deepseek` to both the `cargo deb` `assets` list and the `cargo generate-rpm` `assets` list
  (same three-column/four-field row shape as the existing `otto-anthropic`/`otto-gemini`/
  `otto-openai` rows).
- `crates/otto/src/bin/otto-deepseek.rs`: new 10-line shim calling `provider_deepseek::run()`,
  copied from `otto-openai.rs`.

### Built-in provider plugin: `crates/otto/src/plugin/builtin/provider_deepseek/mod.rs`

Mirrors `provider_openai/mod.rs` (which itself mirrors `provider_anthropic/mod.rs`):

- `PLUGIN_ID = "internal:provider-deepseek"`, `PROVIDER_ID = "deepseek"`, `DISPLAY_NAME =
  "DeepSeek"`.
- `ProviderDeepSeekPlugin` struct with the same `client: Option<Box<dyn ProviderClient>>` /
  `active: bool` shape, `new()`, `#[cfg(test)] with_test_client`, `#[cfg(test)]
  set_active_for_render`.
- `capabilities()` returns a static `ProviderCapabilities` with two models:
  - `deepseek-chat` — display "DeepSeek Chat", no vision, no audio, 64,000 context window (per
    DeepSeek's published API limits), `CostTier::Cheap`.
  - `deepseek-reasoner` — display "DeepSeek Reasoner", no vision, no audio, 64,000 context window,
    `CostTier::Standard`.
  - Default model id: `"deepseek-chat"`.
- `try_build_registration` — same keyring-read → builder → `InProcessProviderClient` →
  `build_dynamic_caps` → `ProviderRegistration` shape as `provider_openai`'s, with model aliases
  `"deepseek"` → `deepseek-chat` and `"deepseek-reasoner"` → `deepseek-reasoner`.
- `try_connect_from_keyring`, `Plugin` impl (`manifest`, `handle_slash`, `on_event`,
  `render_slot`), and `BuiltinProviderPlugin::take_client` — copied verbatim from
  `provider_openai`'s shape with the constants above substituted.
- Same test suite shape as `provider_openai/mod.rs` (`no_creds_emits_prompt_api_key`,
  `handle_slash_with_stored_key_skips_modal`,
  `handle_slash_with_rekey_flag_opens_modal_even_when_client_exists`,
  `manifest_declares_provider_and_slash`, `render_slot_marks_active_provider`), adjusted for the
  new plugin id/provider id/display name.

### Registration call sites

- `crates/otto/src/plugin/mod.rs` — add `ProviderEntry::new(builtin::provider_deepseek::
  ProviderDeepSeekPlugin::new())` to the same `Vec` that registers `provider_anthropic`,
  `provider_openai`, `provider_gemini`, `provider_local` (slot order: after `provider_openai`,
  before `provider_gemini`, matching this spec's overall "insert deepseek right after openai"
  convention — order does not affect behavior since `/connect` iterates by id lookup, not
  position, but keeping insertion order consistent across all four+one call sites avoids reviewer
  confusion).
- `crates/otto/src/main.rs` — three call sites:
  1. `bootstrap_pool_host`'s `use crate::plugin::builtin::{...}` import list and
     `try_provider!(ProviderDeepSeekPlugin::new(), "DeepSeek", "deepseek");` line, inserted after
     the `ProviderOpenAiPlugin` line.
  2. Two `match` statements (around the current lines 1722 and 2477) that map a provider id string
     to `<Plugin>::new().try_build_registration()` for reconnect/`/connect <id>` — add `"deepseek"
     => ProviderDeepSeekPlugin::new().try_build_registration().await,` alongside the existing
     `"openai"` arm (plus the corresponding `use` import in each function).
- `crates/otto/src/providers.rs` — add a `ProviderSpec` entry to the `PROVIDERS` const (drives the
  `/connect` picker list in `ui.rs` and `app.rs`, and `effects.rs`'s `build_connect_candidates`):
  ```rust
  ProviderSpec {
      id: "deepseek",
      display_name: "DeepSeek",
      api_key_env: "DEEPSEEK_API_KEY",
      default_model: "deepseek-chat",
      api_key_required: true,
  },
  ```
  Inserted after the `"openai"` entry, before `"local"`.
- `crates/otto/src/migration.rs` — add `"deepseek"` to `KNOWN_PROVIDERS` (keyring migration scan)
  after `"openai"`.
- `crates/otto/src/plugin/effects.rs` — the `build_connect_candidates` test asserting `["anthropic",
  "gemini", "openai", "local"]` are present must additionally assert `"deepseek"`.

### Documentation

- `README.md`:
  - The provider-count prose (currently "three standalone provider MCP servers") becomes "four
    standalone provider MCP servers".
  - The workspace-map table row for `crates/otto` lists `otto-{anthropic,gemini,openai,deepseek}`.
  - Add a `crates/provider-deepseek` row to the workspace-map table, alongside
    `provider-anthropic`/`provider-gemini`/`provider-openai`.
  - `/connect` and routing-example prose that enumerates provider ids gets a `deepseek` mention
    where the existing examples enumerate all built-ins (do not rewrite every unrelated example
    that only needs one illustrative provider).

## Public-interface impact (Non-Negotiable Rule 6)

**Additive only.** A new provider id (`"deepseek"`), a new `ProviderHandler` implementation, a new
optional env var (`DEEPSEEK_API_KEY`, `DEEPSEEK_BASE_URL`, `OTTO_DEEPSEEK_LISTEN`), and a new
shipped binary (`otto-deepseek`) are all additive per this repo's pre-1.0 convention. No existing
tool schema, slash command, SPP wire type, or on-disk format changes shape. No MINOR-vs-PATCH
ambiguity: this is a MINOR bump (new feature) per `CHANGELOG.md`'s convention, done in the
follow-up release PR per Non-Negotiable Rule 8.

## Load-bearing invariants checked

- **Everything is MCP-shaped:** `DeepSeekProvider` is just a `ProviderHandler`; no turn-loop or
  provider-selection logic leaks into `otto-host`. Linked in-process via `InProcessProviderClient`,
  same as every other built-in.
- **Host-swap `RwLock` rule:** not touched — this change adds a plugin/provider crate, not
  `app.rs`/`tui.rs` logic. `providers.rs`, `plugin/mod.rs`, `migration.rs`, `main.rs`'s
  `bootstrap_pool_host`/reconnect match arms are all synchronous list/match additions with no new
  `Arc<RwLock<...>>` interaction.
- **Provider transport split:** `DeepSeekProvider` goes through the same pool
  (`add_provider`/`remove_provider`/`set_active_provider`) as every other built-in; no ad hoc
  registry.
- **`rmcp` `ProgressDispatcher` forwarder-abort pattern:** `stream.rs` is a straight copy of
  `provider-openai`'s SSE consumer, which already implements this pattern correctly (verify during
  implementation that the copied file preserves the abort-on-completion behavior verbatim).
- **Secrets:** `DEEPSEEK_API_KEY` is read from env/keyring only, same as every other provider;
  never logged, never written to a transcript.

## Testing

- Unit tests in `provider-deepseek` (list_models filter/pass-through, HTTP error mapping) mirroring
  `provider-openai`'s `list_models_tests` module, adjusted for DeepSeek's unfiltered model list.
- `provider-deepseek/tests/integration.rs` mirrors `provider-openai/tests/integration.rs`.
- Plugin tests in `provider_deepseek/mod.rs` mirror `provider_openai/mod.rs`'s five tests.
- `crates/otto/src/plugin/effects.rs`'s `build_connect_candidates` test updated to expect
  `"deepseek"` in the picker candidate set.
- Full validation: `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo
  clippy --workspace --all-targets`, `cargo fmt --all --check`.

## Release

This PR does not itself bump the version. Per Non-Negotiable Rule 8 / `RELEASING.md`, a dedicated
release PR follows immediately after merge: bump `workspace.package.version` +
`workspace.dependencies` versions to the next MINOR (`0.25.0`), add a `CHANGELOG.md` entry, tag
`v0.25.0`.

# provider-grok Design

> **Status:** pending review

**Ref:** savvagent/otto#103 — "Add Grok as a provider" (issue body empty; scope established by this
spec per the otto-factory job brief for job-29)

## Problem

Otto ships built-in providers for Anthropic, Google Gemini, OpenAI, DeepSeek, and local Ollama.
xAI's Grok models are not currently selectable via `/connect`. xAI publishes a "legacy" Chat
Completions endpoint (`POST https://api.x.ai/v1/chat/completions`) that is wire-compatible with
OpenAI's Chat Completions API — same bearer auth, same request/response JSON shape, same
`data: {...}` / `data: [DONE]` SSE streaming framing (confirmed against xAI's published API
reference at spec time: `docs.x.ai/developers/model-capabilities/legacy/chat-completions`). xAI's
newer "Responses API" is now the vendor's preferred surface for new features, but the legacy Chat
Completions endpoint remains supported, and building against it lets this provider mirror the exact
shape `provider-openai`/`provider-deepseek` already use — no new translator concept, no new
transport.

## Goal

Add Grok as a sixth built-in provider, selectable from `/connect`, `/model`, `/use`, and the
`@provider:model` routing prefix, following the exact shape of `provider-deepseek` (itself a mirror
of `provider-openai`), since Grok's Chat Completions API is OpenAI-compatible.

## Scope

**In:**
- New library crate `crates/provider-grok` implementing `ProviderHandler` (`otto-mcp`) against
  xAI's legacy Chat Completions endpoint.
- A thin standalone `otto-grok` MCP-server binary (wire-protocol debugging path only, per
  `CLAUDE.md`'s in-process-by-default architecture).
- A built-in provider plugin (`crates/otto/src/plugin/builtin/provider_grok/mod.rs`) wiring Grok
  into the runtime provider pool exactly like every other built-in.
- Registration at every existing provider-catalog call site (`plugin/mod.rs`, `main.rs`'s startup
  + two reconnect match arms, `providers.rs`'s `PROVIDERS` const, `migration.rs`'s
  `KNOWN_PROVIDERS`).
- README documentation of the new provider.

**Out:**
- xAI's newer "Responses API" (stateful, server-side conversation persistence) — out of scope; this
  spec targets the stateless legacy Chat Completions endpoint only, matching every other built-in
  provider's request/response-per-turn shape. A future spec could add Responses API support if the
  legacy endpoint is ever deprecated.
- No changes to `Host`, `ToolRegistry`, the SPP wire format, or any other provider.
- No changes to the host-swap `RwLock` discipline or the `rmcp` `ProgressDispatcher` pattern beyond
  replicating the existing (correct) pattern already used by `provider-openai`/`provider-deepseek`.
- No "thinking"/extended-reasoning parameter support (mirrors the `provider-deepseek` non-goal for
  the same reason: avoiding the reasoning-content replay-through-tool-calls plumbing some
  reasoning-capable APIs require once tools are in play). If xAI's Chat Completions API exposes an
  opt-in reasoning field, this translator never sets it.

## Assumptions

Recorded because the issue carries no acceptance criteria — these are the scope decisions the job
brief asked to be made and documented:

1. **Provider id is `"grok"`**, not `"xai"` — matches the issue title ("Add Grok as a provider") and
   is the user-facing name. This mirrors the existing precedent that a provider's stable id need not
   match its vendor's own env var convention (the `"local"` provider's id doesn't match
   `OLLAMA_HOST`'s `OLLAMA_` prefix either). Keyring account is `grok` (service `otto`, per
   `CLAUDE.md`'s "account `<provider id>`" convention).
2. **`api_key_env` hint is `XAI_API_KEY`**, not `GROK_API_KEY` — xAI's own docs and SDKs
   consistently use this name, and the field is only a UI hint (`ProviderSpec` doc comment: "the env
   var the underlying SDK conventionally reads... never actually read or set it"), so matching the
   vendor's real convention is more useful to a user than inventing a `GROK_`-prefixed name.
3. **Base URL `https://api.x.ai/v1`, endpoint `/chat/completions`** (full path
   `https://api.x.ai/v1/chat/completions`) — confirmed live against xAI's published API reference at
   spec time. Override env var (for the standalone binary and any self-hosted proxy) is
   `GROK_BASE_URL`, matching this repo's `<PROVIDER_ID>_BASE_URL` convention (id-based, like
   `DEEPSEEK_BASE_URL`), not the vendor-based `XAI_` prefix used for the key hint — the base URL var
   is otto's own override knob, not a vendor SDK convention to mirror.
4. **Model catalog: `grok-4` (default) and `grok-4-fast`.** Chosen because both are attested in
   xAI's own current API-reference curl examples at spec time. Exact context-window sizes and
   modality support are best-available public documentation at spec time (same caveat
   `provider-deepseek`'s spec carried for its model catalog) and may need a follow-up PATCH release
   if xAI's published limits differ:
   - `grok-4` — display "Grok 4", vision-capable, no audio, 256,000-token context window,
     `CostTier::Standard`. Default model id.
   - `grok-4-fast` — display "Grok 4 Fast", vision-capable, no audio, 2,000,000-token context
     window, `CostTier::Cheap`.
   - Aliases: `"grok"` → `grok-4`, `"grok-fast"` → `grok-4-fast`.
5. **`list_models()` calls `GET /v1/models`** (OpenAI-compatible-layer convention; not explicitly
   documented by xAI at spec time the way OpenAI's/DeepSeek's `GET /models` is, but xAI's API is
   marketed as a drop-in-compatible surface and every other OpenAI-shaped provider in this repo
   implements it the same way). Unlike `provider-deepseek`'s unfiltered pass-through, this
   implementation filters out any listed model id containing `"image"`, `"imagine"`, or `"voice"`
   (xAI's catalog is known to mix image-/video-/voice-generation model ids into the same namespace
   as chat models) — a defensive substring filter, not a confirmed exhaustive exclusion list, so a
   future model family with a different non-chat naming pattern could still leak through; a
   follow-up issue can tighten this if that happens. If the endpoint is unavailable or returns an
   error, `list_models()` propagates the error like every other provider — it is not required for
   `/connect` (which reads the static `capabilities()` catalog above), so a broken `list_models()`
   never blocks connecting.
6. **Streaming support: full SSE**, mirroring `provider-openai`/`provider-deepseek`'s `stream.rs`
   verbatim (`data: {...}\n\n` chunks, `data: [DONE]` terminator) — xAI's legacy Chat Completions
   endpoint documents the same streaming shape.
7. **Standalone binary `otto-grok`, default listen `127.0.0.1:8791`** (next free port after
   `provider-deepseek`'s `8790`), env override `OTTO_GROK_LISTEN` — id-based, matching this repo's
   own `OTTO_<PROVIDER_ID>_LISTEN` convention (not vendor-based), since this is otto's own knob for
   its own standalone-binary debugging path.

## Design

### New library crate: `crates/provider-grok`

Mirrors `crates/provider-deepseek` file-for-file (itself a mirror of `provider-openai`):

- `src/api.rs` — typed mirror of the Chat Completions request/response shapes (identical schema to
  OpenAI's/DeepSeek's; copy verbatim, only doc comments change).
- `src/translate.rs` — SPP ⇄ `api` type conversions, functions renamed `request_to_grok` /
  `response_from_grok`. Never sets a reasoning/thinking field (see Scope > Out).
- `src/stream.rs` — SSE → SPP `StreamEvent` adapter, copied verbatim from `provider-deepseek`'s
  (identical framing).
- `src/mcp.rs` — `ProviderHandler` MCP server wrapper, `GrokMcpServer` (mirrors `DeepSeekMcpServer`).
- `src/lib.rs` — `GrokProvider` / `GrokProviderBuilder`, `DEFAULT_BASE_URL = "https://api.x.ai/v1"`,
  `CHAT_COMPLETIONS_PATH = "/chat/completions"`, `DEFAULT_MODEL = "grok-4"`. Builder reads
  `XAI_API_KEY` from the environment when no explicit key is supplied. `list_models` calls
  `GET /models` (relative to `DEFAULT_BASE_URL`, i.e. `GET https://api.x.ai/v1/models`) and applies
  the `image`/`imagine`/`voice` substring filter from Assumption 5. `run()` reads `XAI_API_KEY`,
  `OTTO_GROK_LISTEN` (default `127.0.0.1:8791`), and `GROK_BASE_URL` from the environment for the
  standalone binary.
- `tests/integration.rs` — mirrors `provider-deepseek`'s integration test against a local mock
  server.

`Cargo.toml`: identical dependency set to `provider-deepseek`'s (`otto-protocol`, `otto-mcp`,
`otto-fence`, `anyhow`, `async-trait`, `axum`, `bytes`, `dotenvy`, `futures`, `reqwest`, `rmcp`,
`serde`, `serde_json`, `thiserror`, `tokio`, `tracing`, `tracing-subscriber`; dev-dependencies
`tokio` with `macros`/`rt-multi-thread`, `axum` with `json`).

### Workspace wiring

- `Cargo.toml` (root): add `crates/provider-grok` to `[workspace] members` and
  `workspace.dependencies` (`provider-grok = { path = "crates/provider-grok", version = "0.30.0" }`
  — matching the MINOR bump this feature ships as, see Release below), alphabetical slot after
  `provider-gemini`, before `provider-local`.
- `crates/otto/Cargo.toml`: add `provider-grok = { workspace = true }` dependency (alphabetical
  slot) and a new `[[bin]] name = "otto-grok" path = "src/bin/otto-grok.rs"` target. (No
  `cargo-deb`/`cargo-generate-rpm` asset-list edits — that packaging metadata was removed from this
  repo per `docs/superpowers/plans/otto-development-drop-deb-rpm`; confirmed absent from
  `crates/otto/Cargo.toml` at spec time. `provider-deepseek`'s original spec predates that removal
  and is stale on this point — the code wins per this repo's own precedence rule.)
- `crates/otto/src/bin/otto-grok.rs`: 10-line shim calling `provider_grok::run()`, copied from
  `otto-deepseek.rs`.

### Built-in provider plugin: `crates/otto/src/plugin/builtin/provider_grok/mod.rs`

Mirrors `provider_deepseek/mod.rs`:

- `PLUGIN_ID = "internal:provider-grok"`, `PROVIDER_ID = "grok"`, `DISPLAY_NAME = "xAI Grok"`.
- `ProviderGrokPlugin` struct with the same `client`/`active` shape, `new()`,
  `#[cfg(test)] with_test_client`, `#[cfg(test)] set_active_for_render`.
- `capabilities()` returns the two-model static `ProviderCapabilities` catalog from Assumption 4,
  default model id `"grok-4"`.
- `try_build_registration` — same keyring-read → builder → `InProcessProviderClient` →
  `build_dynamic_caps` → `ProviderRegistration` shape as `provider_deepseek`'s, with model aliases
  from Assumption 4.
- `try_connect_from_keyring`, `Plugin` impl (`manifest`, `handle_slash`, `on_event`,
  `render_slot`), and `BuiltinProviderPlugin::take_client` — copied verbatim from
  `provider_deepseek`'s shape with the constants above substituted.
- Same test suite shape as `provider_deepseek/mod.rs`'s five tests, adjusted for the new plugin
  id/provider id/display name.

### Registration call sites

- `crates/otto/src/plugin/mod.rs` — add `ProviderEntry::new(builtin::provider_grok::
  ProviderGrokPlugin::new())` to the `Vec` that registers every other built-in (slot: after
  `provider_deepseek`, before `provider_gemini`, matching alphabetical-ish insertion precedent —
  actual position doesn't affect behavior since lookup is by id, but keeps a consistent order across
  every call site).
- `crates/otto/src/main.rs` — three call sites, same shape as `provider-deepseek`'s Task 3: the
  startup `try_provider!` macro call plus its `use` import, and a `"grok" =>
  ProviderGrokPlugin::new().try_build_registration().await,` arm in each of the two reconnect
  `match` statements plus their `use` imports.
- `crates/otto/src/providers.rs` — add a `ProviderSpec` entry to `PROVIDERS`:
  ```rust
  ProviderSpec {
      id: "grok",
      display_name: "xAI Grok",
      api_key_env: "XAI_API_KEY",
      default_model: "grok-4",
      api_key_required: true,
  },
  ```
- `crates/otto/src/migration.rs` — add `"grok"` to `KNOWN_PROVIDERS`.
- `crates/otto/src/plugin/effects.rs` — the `build_connect_candidates` test's expected-id array
  gains `"grok"`.
- Any other hardcoded provider-id enumeration found by grep during implementation (same
  verification step `provider-deepseek`'s Task 3 used) must be extended the same way.

### Documentation

- `README.md`: provider-count prose, the `crates/otto` workspace-map row (binaries list), a new
  `crates/provider-grok` workspace-map row, and any true all-providers enumeration in routing/
  `/connect` examples gain a `grok` mention (single-provider illustrative examples are left alone).

## Public-interface impact (Non-Negotiable Rule 6)

**Additive only.** A new provider id (`"grok"`), a new `ProviderHandler` implementation, new
optional env vars (`XAI_API_KEY`, `GROK_BASE_URL`, `OTTO_GROK_LISTEN`), and a new shipped binary
(`otto-grok`) are all additive per this repo's pre-1.0 convention. No existing tool schema, slash
command, SPP wire type, or on-disk format changes shape. MINOR bump per `CHANGELOG.md`'s convention,
done in the follow-up release PR per Non-Negotiable Rule 8.

## Load-bearing invariants checked

- **Everything is MCP-shaped:** `GrokProvider` is just a `ProviderHandler`; no turn-loop or
  provider-selection logic leaks into `otto-host`. Linked in-process via `InProcessProviderClient`,
  same as every other built-in.
- **Host-swap `RwLock` rule:** not touched — this change adds a plugin/provider crate, not
  `app.rs`/`tui.rs` logic. `providers.rs`, `plugin/mod.rs`, `migration.rs`, `main.rs`'s
  `bootstrap_pool_host`/reconnect match arms are all synchronous list/match additions with no new
  `Arc<RwLock<...>>` interaction; the one `.await` per call site (`try_build_registration().await`)
  runs before any host-swap guard is held, matching every sibling provider's existing call site.
- **Provider transport split:** `GrokProvider` goes through the same pool
  (`add_provider`/`remove_provider`/`set_active_provider`) as every other built-in; no ad hoc
  registry.
- **`rmcp` `ProgressDispatcher` forwarder-abort pattern:** lives in `crates/otto-host/src/
  provider.rs`'s `RmcpProviderClient` (the opt-in MCP-over-HTTP transport), not in any provider
  crate's `stream.rs` (confirmed during `provider-deepseek`'s spec review); this task does not touch
  `crates/otto-host` at all, so the invariant is preserved by construction.
- **Secrets:** `XAI_API_KEY` is read from env/keyring only, same as every other provider; never
  logged, never written to a transcript.

## Error Handling & Edge Cases

- HTTP error responses (401/403/429/5xx) from `https://api.x.ai/v1/chat/completions` map to
  `ProviderError` the same way `provider-deepseek`'s `http_status_error`/`parse_error_response`
  helpers do — no `unwrap()`, an actionable message for an LLM caller that has never read xAI's
  docs.
- `list_models()` failure (network error, unexpected shape, or the endpoint simply not existing as
  documented) propagates as a `ProviderError`; per Assumption 5 this never blocks `/connect`, which
  only reads the static `capabilities()` catalog.
- Missing `XAI_API_KEY` (no keyring entry, no env fallback): `try_build_registration` returns the
  same "missing credential" `ProviderRegistration` failure shape every other built-in returns,
  surfaced by the `/connect` picker's existing "prompt for API key" flow — no new error path.

## Risks & Open Questions

- xAI's public API reference does not, at spec time, document a `GET /models` endpoint as
  prominently as OpenAI's/DeepSeek's docs do (their docs point users at a console UI or a models
  doc page instead of an API-reference endpoint listing). This spec assumes the OpenAI-compatible
  surface still exposes it, per the vendor's own "drop-in compatible" positioning. If integration
  testing during implementation shows the endpoint 404s in practice, `list_models()` should still be
  implemented (matching the trait shape every sibling provider uses) but may return an error in
  production until confirmed — this does not block `/connect`, `/model`, or turn execution, all of
  which use the static `capabilities()` catalog instead.
- Exact context-window sizes and vision/audio modality support for `grok-4`/`grok-4-fast` are
  best-available public documentation at spec time (see Assumption 4) and may drift from xAI's
  actual current limits; a follow-up PATCH can correct the static catalog without any interface
  change.
- xAI's newer "Responses API" (stateful, out of scope per this spec) may eventually become the only
  supported surface if the legacy Chat Completions endpoint is deprecated; that would require a new
  spec, not a PATCH, since it changes the translator's request/response shape and potentially the
  turn-loop's statelessness assumption.

## Testing

- Unit tests in `provider-grok` (list_models filter behavior, HTTP error mapping) mirroring
  `provider-deepseek`'s `list_models_tests` module, adjusted for the image/imagine/voice filter from
  Assumption 5.
- `provider-grok/tests/integration.rs` mirrors `provider-deepseek/tests/integration.rs`.
- Plugin tests in `provider_grok/mod.rs` mirror `provider_deepseek/mod.rs`'s five tests.
- `crates/otto/src/plugin/effects.rs`'s `build_connect_candidates` test updated to expect `"grok"`
  in the picker candidate set, and `plugin/mod.rs`'s `register_builtins_pr8_complete` test's
  hardcoded provider/registry counts updated for a sixth provider.
- Full validation: `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo
  clippy --workspace --all-targets`, `cargo fmt --all --check`.

## Release

This PR does not itself bump the version. Per Non-Negotiable Rule 8 / `RELEASING.md`, a dedicated
release PR follows immediately after merge: bump `workspace.package.version` +
`workspace.dependencies` versions to the next MINOR (`0.30.0`, current is `0.29.1`), add a
`CHANGELOG.md` entry, tag `v0.30.0`.

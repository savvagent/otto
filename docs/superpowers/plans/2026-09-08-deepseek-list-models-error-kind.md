# deepseek-list-models-error-kind Implementation Plan

**Goal:** Fix `crates/provider-deepseek/src/lib.rs::list_models` so a non-2xx `/models` response is
classified by its actual HTTP status (mirroring `provider-anthropic`/`provider-gemini`/
`provider-openai`'s already-correct `status_to_error_kind` pattern) instead of being hardcoded to
`ErrorKind::Network`. This restores the already-shipped PR 85 `--rekey` recovery path
(`savvagent/otto#81`) for DeepSeek: a bad key currently gets waved through as "connected" via
`build_dynamic_caps`'s Network-is-transient fallback, so the user is never shown the `--rekey` hint
and later usage fails confusingly. Once `list_models` reports `Authentication` for a 401, the
existing (unmodified) connect-time rejection logic in `crates/otto` starts working correctly for
DeepSeek.

**Architecture:** The fix is entirely inside `provider-deepseek`, a library crate implementing
`ProviderHandler` (`otto-mcp`) — no turn-loop change, no host-swap `RwLock` interaction, no
provider-transport-split change, no streaming-path change (`list_models` is a plain GET, not SSE).
Extract the status→kind match already present in `parse_error_response` (used by `complete()`'s
error path) into a standalone `status_to_error_kind(status: u16) -> ErrorKind` function, and make
`http_status_error` (used only by `list_models`'s error path) call it instead of hardcoding
`ErrorKind::Network`.

**Tech Stack:** Rust 2024, `reqwest`, `otto-protocol::ErrorKind`, `axum` (test mock server, already
a dev-dependency), `cargo test -p provider-deepseek`.

**Spec:** `docs/superpowers/specs/2026-09-08-deepseek-list-models-error-kind-design.md` — read it
first. This plan implements it exactly.

**Release line:** `v0.26.4` (PATCH — bugfix, no new feature, no breaking change per this repo's
pre-1.0 SemVer convention)

**Branch:** `provider/deepseek-reconnect-rejected-key` (already created as the current worktree
branch — do not rename it)

## File Map

**Modified files**
- `crates/provider-deepseek/src/lib.rs` — add `status_to_error_kind`; reuse it in
  `parse_error_response` and `http_status_error`; correct `http_status_error`'s doc comment; fix
  the bug-encoding assertion in `list_models_propagates_http_failure`; add
  `list_models_401_maps_to_authentication`, `list_models_5xx_maps_to_overloaded`, and a
  `status_to_error_kind` table unit test.
- `docs/superpowers/specs/2026-09-08-deepseek-list-models-error-kind-design.md` (already committed)
- `docs/superpowers/plans/2026-09-08-deepseek-list-models-error-kind.md` (this file)

## Task 1: Classify `list_models`'s HTTP errors by status code

**Files:**
- Modify: `crates/provider-deepseek/src/lib.rs`

- [ ] Read `crates/provider-deepseek/src/lib.rs` in full (already done during spec research —
      confirm line numbers haven't drifted before editing: `list_models` ~130-153,
      `http_status_error` ~244-266, `parse_error_response` ~272-313,
      `list_models_propagates_http_failure` ~511-533).
- [ ] Write/adjust the failing test first: change the existing
      `list_models_propagates_http_failure` test's assertion from
      `assert!(matches!(err.kind, ErrorKind::Network), "kind: {:?}", err);` to
      `assert!(matches!(err.kind, ErrorKind::Authentication), "kind: {:?}", err);` (the mocked
      response in that test is already a 401 — only the assertion is wrong). Run
      `cargo test -p provider-deepseek list_models_propagates_http_failure` and confirm it now
      **fails** (proving the test exercises the current, unfixed bug).
- [ ] Add a private function above `http_status_error`:
      ```rust
      /// Map a DeepSeek HTTP response status to the SPP `ErrorKind` it
      /// represents. Shared by `parse_error_response` (`complete()`'s error
      /// path) and `list_models`'s error path so both report the same
      /// classification for the same status — previously `list_models`
      /// hardcoded `ErrorKind::Network` for every non-2xx response,
      /// misclassifying a bad API key (401) as a transient network error.
      fn status_to_error_kind(status: u16) -> ErrorKind {
          match status {
              400 => ErrorKind::InvalidRequest,
              401 => ErrorKind::Authentication,
              403 => ErrorKind::PermissionDenied,
              404 => ErrorKind::ModelNotFound,
              413 => ErrorKind::ContextLengthExceeded,
              429 => ErrorKind::RateLimited,
              500 | 502 | 503 | 504 => ErrorKind::Overloaded,
              _ => ErrorKind::Internal,
          }
      }
      ```
- [ ] Update `http_status_error` to build `kind: status_to_error_kind(status.as_u16())` instead of
      the hardcoded `kind: ErrorKind::Network`. Correct its doc comment from "Build a `Network`-kind
      `ProviderError` that surfaces the response body alongside the HTTP status." to something like
      "Build a `ProviderError` classified by HTTP status (see `status_to_error_kind`), surfacing the
      response body alongside it." Function signature and call site in `list_models` are otherwise
      unchanged.
- [ ] Update `parse_error_response`'s inline `match status.as_u16() { 400 => ..., ... }` block to
      instead read `let kind = status_to_error_kind(status.as_u16());` — pure refactor, no behavior
      change (this function already computed the correct mapping; now it shares the table instead
      of duplicating it).
- [ ] Run `cargo test -p provider-deepseek list_models_propagates_http_failure` and confirm it now
      **passes**.
- [ ] Add `list_models_401_maps_to_authentication` (mirroring
      `provider-anthropic/src/models.rs`'s and `provider-gemini/src/models.rs`'s equivalent test):
      mock a 401 with a JSON body containing an `"invalid api key"`-style message, call
      `list_models`, assert `matches!(err.kind, ErrorKind::Authentication)` and that
      `err.message.contains("invalid api key")` (or the mocked phrase) and
      `err.message.contains("401")`.
- [ ] Add `list_models_5xx_maps_to_overloaded`: mock a 503, call `list_models`, assert
      `matches!(err.kind, ErrorKind::Overloaded)`.
- [ ] Add a `status_to_error_kind` table unit test (plain `#[test]`, no mock server needed) that
      enumerates the full mapping: 400→InvalidRequest, 401→Authentication, 403→PermissionDenied,
      404→ModelNotFound, 413→ContextLengthExceeded, 429→RateLimited, 500/502/503/504→Overloaded,
      and one unmapped code (e.g. 418) → Internal.
- [ ] Run `cargo test -p provider-deepseek` (full crate) — all tests green, including the pre-
      existing `list_models_returns_all_listed_ids_unfiltered` and
      `list_models_default_model_id_none_when_default_missing` tests (unaffected by this change,
      since they exercise the 2xx path).
- [ ] Run `cargo test --workspace` — confirm no other crate's tests reference
      `provider-deepseek`'s `list_models` error-kind behavior in a way this change breaks (none
      expected; `crates/otto`'s `provider_deepseek/mod.rs` and `provider_common.rs` consume
      `ProviderError.kind` generically, with no DeepSeek-specific `Network`-vs-`Authentication`
      branching to update).
- [ ] Public-interface check: `status_to_error_kind` is a new private (non-`pub`) function; no
      change to `ProviderHandler`, `ProviderError`'s shape, the SPP wire format, any tool's MCP
      schema, the plugin ABI, a slash command, an env var, or the on-disk transcript/keyring format
      (Non-Negotiable Rule 6). This is additive/bugfix-only — record a one-line `CHANGELOG.md`
      "Fixed" entry in the release PR (Task 2), not here.
- [ ] Host-swap `RwLock` check: not applicable — no `crates/otto/src/app.rs` or
      `crates/otto/src/tui.rs` changes in this task.
- [ ] `ProgressDispatcher` forwarder-abort check: not applicable — `list_models` is a plain GET,
      not a streaming path; no `stream.rs` or SSE-consumer code touched.
- [ ] Run `cargo clippy -p provider-deepseek --all-targets` — clean.
- [ ] Format and commit: `cargo fmt --all` then
      `git commit -m "provider-deepseek: classify list_models HTTP errors by status code"`.

## Task 2: Cut the release

This work ships as `v0.26.4` (PATCH — bugfix). Per this repo's Non-Negotiable Rule 8, after this
plan's PR merges to `main`, open a dedicated release PR following `RELEASING.md`'s manual process:
bump `workspace.package.version` (and any internal `workspace.dependencies` version pins) to
`0.26.4` in the root `Cargo.toml`, run `cargo check --workspace` to refresh `Cargo.lock`, move
`## [Unreleased]` in `CHANGELOG.md` to `## 0.26.4 - YYYY-MM-DD` with a `Fixed` entry along the
lines of "DeepSeek `list_models` now classifies HTTP error responses (401/403/429/5xx) by status
code instead of always reporting a transient network error, so an invalid API key is correctly
rejected at connect time and the existing `--rekey` recovery hint (#81) works for DeepSeek", add a
fresh empty `## [Unreleased]` above it, validate with `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets`, `cargo test --workspace`, then open/merge that PR, tag
`v0.26.4`, and push the tag. The version bump and CHANGELOG entry happen in that release PR, not in
this task's commit.

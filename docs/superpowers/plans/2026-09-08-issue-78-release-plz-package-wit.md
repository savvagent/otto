# issue-78-release-plz-package-wit Implementation Plan

**Goal:** Make `otto-plugin-wasm` packageable under Cargo's isolated package-verification sandbox by giving its `wasmtime::component::bindgen!` call sites crate-local WIT inputs, while preserving `otto-plugin-wit` as the canonical dependency-light contract crate.

**Architecture:** The fix stays inside the plugin ABI packaging boundary. `crates/otto-plugin-wit` remains the canonical owner of the WIT contract and keeps its dependency-light shape. `crates/otto-plugin-wasm` becomes self-contained for package verification by vendoring the WIT tree and enforcing byte-for-byte sync from a local `build.rs`. No host turn-loop, provider-pool, tool-registry, slash-command, or streaming-provider changes are in scope.

**Tech Stack:** Rust 2024 workspace crates, `wasmtime::component::bindgen!`, Cargo package verification, std `fs`/`path` build-script checks, and existing GitHub Actions packaging constraints.

**Spec:** `docs/superpowers/specs/2026-09-08-issue-78-release-plz-package-wit-design.md` — read it first. This plan implements it exactly.

**Release line:** `v0.26.2`

**Branch:** `plugin/issue-78-release-plz-package-wit`

## File Map

**New files**
- `crates/otto-plugin-wasm/build.rs` — verifies the vendored WIT tree matches the canonical sibling copy when both are present.
- `crates/otto-plugin-wasm/wit/plugin-static.wit` — vendored canonical WIT world for package-local bindgen input.
- `crates/otto-plugin-wasm/wit/plugin-interactive.wit` — vendored canonical WIT world for package-local bindgen input.
- `crates/otto-plugin-wasm/wit/plugin-provider.wit` — vendored canonical WIT world for package-local bindgen input.
- `crates/otto-plugin-wasm/wit/shared.wit` — vendored shared plugin interface definitions.
- `crates/otto-plugin-wasm/wit/spp.wit` — vendored SPP-facing WIT definitions.

**Modified files**
- `crates/otto-plugin-wasm/src/lib.rs` — repoint `bindgen!` to crate-local `wit/` and document the vendoring rationale.
- `crates/otto-plugin-wasm/Cargo.toml` — include any package metadata needed for the new `build.rs`/`wit/` inputs.

## Task 1: Vendor the WIT tree and enforce sync with the canonical crate

**Files:**
- Create: `crates/otto-plugin-wasm/build.rs`
- Create: `crates/otto-plugin-wasm/wit/plugin-static.wit`
- Create: `crates/otto-plugin-wasm/wit/plugin-interactive.wit`
- Create: `crates/otto-plugin-wasm/wit/plugin-provider.wit`
- Create: `crates/otto-plugin-wasm/wit/shared.wit`
- Create: `crates/otto-plugin-wasm/wit/spp.wit`
- Modify: `crates/otto-plugin-wasm/Cargo.toml`

- [ ] Reproduce the current packaging breakage from the worktree with `cargo package --allow-dirty --workspace`. Expected result: packaging currently fails during `otto-plugin-wasm` verification because the bindgen macro cannot resolve the sibling `../otto-plugin-wit/wit` path inside Cargo's staged package sandbox.
- [ ] Add `crates/otto-plugin-wasm/wit/` as a byte-for-byte vendored copy of `crates/otto-plugin-wit/wit/`, and add `crates/otto-plugin-wasm/build.rs` that (a) emits `cargo::rerun-if-changed=wit`, (b) watches `../otto-plugin-wit/wit`, and (c) fails with actionable messages when the vendored files are missing or differ while the canonical sibling tree exists. Expected result: a normal workspace build now guards against WIT drift before compilation.
- [ ] Update `crates/otto-plugin-wasm/Cargo.toml` only if explicit package metadata is needed to keep `build.rs` and `wit/` in the packaged crate contents. Expected result: Cargo includes the vendored WIT tree in package inputs without changing `otto-plugin-wit` dependencies.
- [ ] Run `cargo check -p otto-plugin-wasm`. Expected result: the crate still builds in the normal workspace with the new build-script guard in place.
- [ ] Public-interface check: record in the task ledger that the plugin ABI WIT contents and generated Rust surface stay unchanged; only packaging/layout behavior changes.
- [ ] Host-swap/RwLock check: not applicable — no `crates/otto/src/app.rs` or `crates/otto/src/tui.rs` change.
- [ ] ProgressDispatcher check: not applicable — no streaming provider path touched.
- [ ] Format and commit: run `cargo fmt --all` and `git commit -m "otto-plugin-wasm: vendor package-local wit inputs"`.

## Task 2: Repoint bindgen and prove the isolated package build works

**Files:**
- Modify: `crates/otto-plugin-wasm/src/lib.rs`
- Modify only if Task 1 validation exposed missing package metadata: `crates/otto-plugin-wasm/Cargo.toml`

- [ ] Change all three `wasmtime::component::bindgen!` call sites in `crates/otto-plugin-wasm/src/lib.rs` from the sibling `../otto-plugin-wit/wit` path to the crate-local `wit` directory, and replace the surrounding comment with the packaging rationale and canonical-source note. Expected result: macro expansion no longer depends on a sibling crate path.
- [ ] Run `cargo check -p otto-plugin-wasm`. Expected result: the crate still builds from the workspace after the bindgen path change.
- [ ] Run `cargo package --allow-dirty --workspace`. Expected result: workspace packaging and verification complete successfully, including `otto-plugin-wasm`'s staged verification build.
- [ ] Inspect the staged package contents with `find target/package/otto-plugin-wasm-0.26.1/wit -maxdepth 1 -type f | sort`. Expected result: all five vendored `.wit` files are present inside Cargo's extracted `otto-plugin-wasm` package sandbox.
- [ ] Public-interface check: confirm no plugin ABI, SPP, MCP, slash-command, env-var, or on-disk format change was introduced; note in the ledger and PR body that this is a packageability fix only.
- [ ] Host-swap/RwLock check: not applicable.
- [ ] ProgressDispatcher check: not applicable.
- [ ] Format and commit: run `cargo fmt --all` and `git commit -m "otto-plugin-wasm: use vendored wit for bindgen"`.

## Task 3: Run full validation and prepare the release handoff

**Files:**
- Modify only if validation exposes issue-78-related regressions.

- [ ] Run `cargo build --workspace --all-targets`. Expected result: success.
- [ ] Run `cargo test --workspace`. Expected result: success.
- [ ] Run `cargo clippy --workspace --all-targets`. Expected result: success.
- [ ] Run `cargo fmt --all -- --check`. Expected result: success.
- [ ] Re-run `cargo package --allow-dirty --workspace` if any validation fix touched packaging inputs after Task 2. Expected result: still succeeds.
- [ ] Public-interface check: verify the PR summary says `otto-plugin-wasm` now vendors package-local WIT files for Cargo verification while `otto-plugin-wit` remains the canonical contract crate.
- [ ] Host-swap/RwLock check: verify no task introduced an `.await` while holding an `Arc<RwLock<Option<Arc<Host>>>>` guard.
- [ ] ProgressDispatcher check: verify no streaming-provider path was touched, so no forwarder-abort change was required.
- [ ] Format and commit: if validation fixes are needed, run `cargo fmt --all` and `git commit -m "otto-plugin-wasm: fix package validation findings"`; otherwise record in the task ledger that no extra code commit was required.

## Task 4: Note the mandatory post-merge release PR

**Files:**
- No code changes in this branch; this task documents the release handoff only.

- [ ] Record that after this PR merges, the orchestrating release flow must open a dedicated release PR that bumps `workspace.package.version` and internal `workspace.dependencies` versions in `Cargo.toml`, updates `CHANGELOG.md` for `v0.26.2`, runs the release validation commands from `RELEASING.md`, and publishes tag `v0.26.2`.
- [ ] Public-interface check: confirm the release PR needs only a PATCH bump because issue #78 is a bug fix with no public interface shape change.
- [ ] Host-swap/RwLock check: not applicable.
- [ ] ProgressDispatcher check: not applicable.
- [ ] Format and commit: no additional commit required unless the plan itself is revised.

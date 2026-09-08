# Make otto-plugin-wasm packageable in isolation — design

Date: 2026-09-08
> **Status:** pending review
Related: `savvagent/otto#78`

## Problem

`crates/otto-plugin-wasm/src/lib.rs` currently invokes `wasmtime::component::bindgen!` three times with `path: "../otto-plugin-wit/wit"`. That works in a normal workspace checkout, where `src/lib.rs` can walk up to the sibling `otto-plugin-wit` crate.

Cargo package verification breaks that assumption. During `cargo package --allow-dirty --workspace`, Cargo stages each crate in an isolated `target/package/<crate>-<version>/` directory and verifies it there. Inside the staged `otto-plugin-wasm` crate, the sibling `../otto-plugin-wit/wit` tree is absent, so the bindgen macro expands against a missing WIT directory and produces incomplete bindings. The isolated compile then fails with missing generated modules such as `static_world::otto` and `provider_world::otto`, which in turn keeps release-plz red on every `main` push.

The packaging failure is specific to the plugin ABI crate boundary: `otto-plugin-wit` is intentionally the dependency-light owner of the canonical WIT contract, while `otto-plugin-wasm` owns the host-side wasmtime bindings generated from that contract. The fix must preserve that boundary and make `otto-plugin-wasm` self-contained enough for Cargo's isolated package verification.

## Goal & Success Criteria

Make `otto-plugin-wasm` build correctly when Cargo verifies packaged crates in isolation, without weakening `otto-plugin-wit`'s leaf-crate dependency guard or changing the plugin ABI.

Success criteria:
- `crates/otto-plugin-wasm/src/lib.rs` no longer depends on a sibling `../otto-plugin-wit/wit` path at bindgen macro expansion time.
- `otto-plugin-wasm` carries the WIT inputs needed for `wasmtime::component::bindgen!` inside its own package contents so Cargo's staged verification build succeeds.
- Normal workspace development still treats `crates/otto-plugin-wit/wit/` as the canonical human-edited contract and detects drift if the vendored copy in `otto-plugin-wasm` diverges.
- `.github/workflows/wit-dep-guard.yml` continues to pass because `otto-plugin-wit` stays dependency-light and does not gain runtime deps such as `wasmtime`, `tokio`, `reqwest`, or `anyhow`.
- Validation directly exercises the previously failing isolation path, not only normal workspace builds.

## Approach

1. **Vendor the WIT tree into `otto-plugin-wasm`.** Add `crates/otto-plugin-wasm/wit/` containing byte-for-byte copies of the canonical `.wit` files from `crates/otto-plugin-wit/wit/`, then change all three `bindgen!` call sites in `crates/otto-plugin-wasm/src/lib.rs` to `path: "wit"`. Because the WIT files now live under the package root, Cargo includes them in the packaged tarball and the macro can still resolve them when the crate is rebuilt in `target/package/...`.
2. **Keep `otto-plugin-wit` canonical.** Add a small `build.rs` to `otto-plugin-wasm` that compares the local vendored `wit/` directory with the sibling `../otto-plugin-wit/wit/` tree when that sibling exists (normal workspace dev/CI builds). If a file is missing or differs byte-for-byte, fail the build with an actionable message telling the developer to resync the vendored copy. If the sibling path is absent, switch to a package-build fallback that validates the vendored file set and pinned normalized-content digests so packaged builds still fail early with targeted messages instead of opaque bindgen errors.
3. **Track WIT files in packaged inputs.** Rely on Cargo's default package contents for `build.rs` and crate-local source files, and only touch `crates/otto-plugin-wasm/Cargo.toml` if validation proves an explicit include list is required. The intended steady state is no manifest change.
4. **Document the rationale at the bindgen site.** Replace the current comment in `crates/otto-plugin-wasm/src/lib.rs` that justifies the sibling path with one explaining why the crate vendors a copy of the WIT tree: canonical editing still happens in `otto-plugin-wit`, but `otto-plugin-wasm` must be self-contained under `cargo package` sandboxing.
5. **Validate both paths.** Run the standard workspace build/test/lint suite plus a packaging-focused verification that actually rebuilds the staged package (either via `cargo package --allow-dirty --workspace` or by building the packaged `target/package/otto-plugin-wasm-<version>/` crate in isolation) to prove the missing-sibling failure is gone.

## Scope

**In:**
- `crates/otto-plugin-wasm/src/lib.rs`
- `crates/otto-plugin-wasm/build.rs`
- `crates/otto-plugin-wasm/wit/*.wit`
- Validation commands and any directly related documentation/comments needed to explain the packaging constraint

**Out:**
- Changing WIT schema contents or plugin ABI semantics
- Moving canonical ownership away from `otto-plugin-wit`
- Adding runtime dependencies to `otto-plugin-wit`
- Reworking release-plz itself or changing unrelated release automation
- Any host/provider/tool transport changes outside the packaging fix

## Public-interface changes

No public interface shape change is intended. The plugin ABI worlds, shared interfaces, crate names, and generated host binding types remain the same; only the source location used by `bindgen!` changes.

This is a packaging and build-layout fix, not an additive or breaking change to the SPP wire format, tool schemas, slash commands, env vars, transcript/keyring formats, or plugin ABI definitions.

## Premise corrections

1. **`otto-plugin-wit::WIT_DIR` already documents why a dependency constant cannot solve the macro path problem.** `crates/otto-plugin-wit/src/lib.rs` explicitly notes that proc-macro `path: "…"` arguments require a string literal at the macro callsite, so this issue is consistent with the current code comments rather than a surprise limitation.
2. **`cargo package -p otto-plugin-wasm` from the workspace root is not the authoritative repro.** On this workspace, that command fails earlier because `otto-plugin-wasm` depends on unpublished local crates such as `otto-mcp`. The relevant acceptance path is Cargo's workspace packaging verification / staged package build where those local packages are included together, or an equivalent manual build inside Cargo's extracted package sandbox.

## Assumptions

- **Vendoring the WIT files into `otto-plugin-wasm` is acceptable duplication when guarded by sync checks.** Rationale: Cargo package verification needs crate-local inputs, and the duplicate tree is small, static contract data.
- **Build-time drift detection is preferable to a test-only check.** Rationale: it fails earlier for developers and CI, and still naturally skips in the packaged sandbox where the sibling source tree does not exist.
- **The canonical edit location should remain `crates/otto-plugin-wit/wit/`.** Rationale: the existing crate/docs/workflow already establish that crate as the contract owner and keep runtime deps out of it.
- **No WIT file generation step is needed.** Rationale: the issue is path resolution during packaging, not schema synthesis; a checked-in synced copy is simpler and more transparent.
- **A comment-only documentation update in code is sufficient unless validation reveals another user-facing packaging doc gap.** Rationale: the failure mode is developer/CI-facing, not an end-user product-surface change.

## Error Handling & Edge Cases

- If the vendored `crates/otto-plugin-wasm/wit/` directory is missing a canonical file, `build.rs` should fail with a message naming the missing file.
- If a vendored file differs from the canonical sibling copy, `build.rs` should fail with an actionable diff-style message telling the developer to resync `otto-plugin-wasm/wit/` from `otto-plugin-wit/wit/`.
- If the sibling `../otto-plugin-wit/wit/` directory does not exist, `build.rs` must treat that as the expected packaged verification case and fall back to validating the vendored file set and pinned normalized-content digests instead of trying the sibling-tree comparison.
- The vendored tree must include every `.wit` file referenced transitively by the worlds (`shared.wit`, `spp.wit`, and the three world files), not only the top-level world entry points.
- Packaging validation should confirm both that the vendored files are present in the staged package and that the crate compiles there.

## Risks & Open Questions

- **Risk: developers may edit only the canonical WIT crate and forget to sync the vendored copy.** Mitigation: fail fast in `otto-plugin-wasm/build.rs` when both trees are present and differ.
- **Risk: package include/exclude defaults could accidentally omit `wit/` or `build.rs`.** Mitigation: inspect package contents during validation and add explicit includes only if Cargo's defaults are insufficient.
- **Open question:** whether the workspace-wide `cargo package --allow-dirty --workspace` command now completes end-to-end in this environment or still hits unrelated pre-existing publishability constraints. If unrelated packageability issues remain, the acceptance proof for this issue will be the isolated staged build for `otto-plugin-wasm`, plus the workspace command if it passes.

# plugin-wasm-parity-policy Implementation Plan

**Goal:** Record, in `CLAUDE.md`, that parity between `otto-plugin` (native) and `otto-plugin-wit`
(WASM guest ABI) is the goal for the plugin ABI's guest-callable behavioral surface and for
constructor ergonomics the guest side can express without a WIT change; enumerate the three
concrete gaps found while surveying the crates (`Screen::ghost_completion`, the
`StyledSpan`/`StyledLine` constructor ergonomics, and the entirely-unbridged `ContentRenderer`
surface); and point at the three tracking issues already filed for them (#165, #166, #167) so a
future changelog entry citing a WASM gap references an issue number instead of repeating "for
now." No Rust, no `.wit` file, and no runtime behavior changes in this PR — see the spec's "Why
the follow-ups are issues, not code" section for why closing #165/#166 with actual WIT/code changes
is deliberately out of scope here.

**Architecture:** One documentation edit, in one file (`CLAUDE.md`): a new "Plugin ABI (native vs.
WASM parity)" subsection under Architecture, plus three new rows in the existing Workspace map
table for `otto-plugin`, `otto-plugin-wit`, `otto-plugin-wasm` (currently absent from that table
entirely). No crate boundaries, no public interfaces, no runtime code touched.

**Tech Stack:** Markdown (`CLAUDE.md`). No build tooling involved in this task; `cargo
build`/`test`/`clippy` are run only to confirm the repo-wide "remain clean" acceptance criterion,
vacuously satisfied since no Rust changes.

**Spec:** `docs/superpowers/specs/2026-09-11-plugin-wasm-parity-policy-design.md` — read it first.
This plan implements it exactly.

**Release line:** at least PATCH (docs-only, no runtime behavior change, no public-interface
change, per `CHANGELOG.md`'s convention). Non-Negotiable Rule 8 permits batching this work's
release with whatever else is already merged and unreleased on `main` at cut time, and a batched
release's actual line is the highest bump required across the whole batch — re-read
`workspace.package.version` (currently `0.30.4` as of branch creation) and `gh release list` at cut
time rather than trusting this floor as the number it ships under.

**Branch:** `plugin/wasm-parity-policy`

## File Map

**New files**
- None (the design spec and this plan are already committed as of Phase 1/2).

**Modified files**
- `CLAUDE.md` — new "Plugin ABI (native vs. WASM parity)" subsection under Architecture; three new
  rows in the Workspace map table.
- `CHANGELOG.md` — a `Changed`/`Added` entry recording the policy decision, added at release-cut
  time (Task 2, performed per Phase 4 step 12, not in this PR's diff) — citing #165/#166/#167.

## Task 1: Add the Plugin ABI parity policy to CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

No Rust in this task, so no `cargo test -p <crate>` step. Verification is a targeted `grep` +
manual read, plus the repo-wide `cargo build`/`test`/`clippy` acceptance criterion (vacuous here,
run anyway per house style).

- [ ] **Step 1: Confirm the current state.** From the worktree root:
  ```bash
  grep -n "otto-plugin" CLAUDE.md
  ```
  Expect zero hits — `otto-plugin`/`otto-plugin-wit`/`otto-plugin-wasm` are currently entirely
  absent from `CLAUDE.md`'s Workspace map table and everywhere else in the file (confirmed while
  writing the spec). This is the baseline the edit below changes.

- [ ] **Step 2: Add three rows to the Workspace map table.** In `CLAUDE.md`, find the existing
  table under `## Workspace map (for navigation)`:
  ```
  | `crates/tool-fs` | `read_file` / `write_file` / `list_dir` / `glob` as a stdio MCP server. |
  ```
  Insert three new rows immediately after it (same table, same file):
  ```
  | `crates/otto-plugin` | The `Plugin`/`Screen`/`ContentRenderer` traits, `Effect`, `StyledSpan`/`StyledLine`, and every other WIT-portable type a native plugin author builds against. |
  | `crates/otto-plugin-wit` | The `.wit` contract (`plugin-static`/`plugin-interactive`/`plugin-provider` worlds, `shared.wit`) that WASM guests bind against. Dependency-light by design — see `crates/otto-plugin-wit/src/lib.rs`. |
  | `crates/otto-plugin-wasm` | Host-side `wasmtime::component::bindgen!` adapters (`adapter/{static_,interactive,provider}.rs`) that load a `.wasm` component and present it as a `Box<dyn Plugin>`/`Box<dyn Screen>` to the rest of the host. |
  ```

- [ ] **Step 3: Add the "Plugin ABI (native vs. WASM parity)" subsection.** In the same file,
  immediately after the existing `### \`rmcp\` ProgressDispatcher gotcha` subsection (the last
  subsection under `## Architecture`) and before `## Workspace map (for navigation)`, insert:
  ```markdown
  ### Plugin ABI (native vs. WASM parity)

  `otto-plugin` (native Rust trait surface), `otto-plugin-wit` (the WIT contract WASM guests bind
  against), and `otto-plugin-wasm` (the host-side wasmtime adapter) are one interface — the plugin
  ABI — per Non-Negotiable Rule 6 in the `otto-development` skill. Because `otto-plugin` is a plain
  Rust crate, it can grow constructors and defaulted trait methods freely; `otto-plugin-wit` is a
  WIT interface, where records can't carry associated functions and every new guest-callable
  capability needs an explicit interface addition plus a regenerated guest binding. Left
  unaddressed, the native surface drifts ahead of the WASM surface by default.

  **Parity is the goal** for the plugin ABI's guest-callable behavioral surface (trait methods a
  plugin implements to receive host callbacks — `Screen`, `Plugin`, hooks) and for constructor
  ergonomics on WIT-backed value types wherever the guest side can express them in ordinary Rust
  without a WIT change. Every `otto-plugin` addition to a surface already mirrored in
  `otto-plugin-wit` should land its WIT/guest-ergonomics counterpart in the same change, or record
  an issue number and the reason it doesn't (yet) — never a bare "for now" in `CHANGELOG.md`. A
  capability family with no WIT interface at all gets named and tracked explicitly rather than left
  to keep growing on the native side alone.

  Current gap (surveyed 2026-09-11, tracked in `savvagent/otto`):

  | Native surface | otto-plugin-wit counterpart | Tracked |
  |---|---|---|
  | `Screen::ghost_completion` (`otto-plugin/src/screen.rs`) | none — `screen-instance`'s WIT resource has `on-key`/`on-event`/`render`/`tips` only | #165 |
  | `StyledSpan`/`StyledLine` constructors (`otto-plugin/src/styled.rs`) | the WIT records exist; associated functions don't cross WIT at all | #166 |
  | `ContentRenderer` (`otto-plugin/src/content.rs`) / `Plugin::create_renderer` (`otto-plugin/src/plugin.rs`) — canvas content blocks | none — no `plugin-canvas.wit` world exists | #167 |

  A future `CHANGELOG.md` entry that ships a native-only `otto-plugin` addition cites the tracking
  issue for the WASM gap it opens, rather than "for now."
  ```

- [ ] **Step 4: Verify the edit landed.**
  ```bash
  grep -n "Plugin ABI (native vs. WASM parity)\|#165\|#166\|#167" CLAUDE.md
  ```
  Expect the subsection heading plus all three issue references present.
  ```bash
  grep -n "crates/otto-plugin" CLAUDE.md
  ```
  Expect the three new Workspace map rows plus every reference inside the new subsection.

- [ ] **Step 5: Public-interface note.** No SPP wire type, `ProviderHandler`/`ProviderClient`
  method, tool MCP schema, plugin ABI surface, slash command, env var, or on-disk transcript/
  keyring format is added, renamed, or removed by this task — it documents a policy about future
  changes to the plugin ABI, it does not change the ABI itself. Non-Negotiable Rule 6 is not
  engaged.

- [ ] **Step 6: Host-swap / streaming invariants — vacuously satisfied.** No
  `crates/otto/src/app.rs` or `crates/otto/src/tui.rs` touched, so the host-swap `RwLock` rule is
  not engaged. No streaming provider path touched, so the `ProgressDispatcher` forwarder-abort
  pattern is not engaged.

- [ ] **Step 7: Repo-wide clean-build check (vacuous, run anyway).**
  ```bash
  cargo build
  cargo test --workspace
  cargo clippy --workspace --all-targets
  ```
  Expect all three to pass unchanged — this task touches only `CLAUDE.md`, so this step's purpose
  is confirming the acceptance criterion literally, not catching a regression this task could
  cause.

- [ ] **Step 8: Format and commit.** No Rust changed, so `cargo fmt --all` is a no-op here but run
  it anyway for consistency with house style:
  ```bash
  cargo fmt --all
  git add CLAUDE.md
  git commit -m "docs: record plugin ABI native-vs-WASM parity policy"
  ```

## Task 2: Cut the release (notes only — performed per Phase 4 step 12, not in this PR)

**Files:** none in this PR.

- [ ] **Step 1:** This PR does **not** bump `workspace.package.version` and does **not** add a
  `CHANGELOG.md` section — that happens in the dedicated release PR after this merges, per
  Non-Negotiable Rule 8 / Phase 4 step 12. Re-read `workspace.package.version` at cut time (it may
  have moved past `0.30.4` if another PR merges first — check `gh release list` and recently-merged
  PRs against `main` for anything else sitting unreleased, and batch it in if so, enumerating every
  batched issue/PR in the release PR body) and cut the next PATCH (or higher, if something else in
  the batch requires it) from whatever it then reads. The `CHANGELOG.md` entry: an `Added` or
  `Changed` bullet stating that `CLAUDE.md` now records the plugin-ABI native-vs-WASM parity
  position (parity is the goal), enumerates the three current gaps, and cites #165/#166/#167 as
  their tracking issues — closing out `savvagent/otto#140`.

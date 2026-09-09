# issue-82-connect-picker-only Implementation Plan

**Goal:** Make the provider picker the single way to connect: `/connect` (bare) opens the picker and ignores any args, and the provider-specific `connect <id>` slash entries are removed from the user-facing surface (typed commands and the `/` command palette) while staying intact as internal plumbing for the picker, silent stored-key reconnect, and `--rekey`. Per `savvagent/otto#82`.

**Architecture:** Keep the plugin-driven connect flow intact. The five provider plugins keep their `connect <id>` slash registrations unchanged — those dispatch strings are only ever produced internally by the picker screen's `Enter`/`Alt+Enter` actions, by `build_connect_candidates` routability, and by silent stored-key/`--rekey` flows. Only `ConnectPlugin::handle_slash` (which now routes a typed arg to `RunSlash { name: "connect {id}" }`) and `build_palette_commands` (which lists every slash in the index) change, plus the user-facing strings/docs that instruct `/connect <id> [--rekey]`.

**Tech Stack:** Existing `crates/otto` plugin/runtime modules and locale files only (`connect/mod.rs`, `plugin/effects.rs`, `providers.rs`, five `provider_*/mod.rs` doc/comment updates, `command_palette/screen.rs` comment, four locale `.toml`s, `README.md`). No new dependencies, no `otto-plugin`/WIT/ABI change.

**Spec:** `docs/superpowers/specs/2026-09-09-issue-82-connect-picker-only-design.md` — read it first. This plan implements it exactly.

**Release line:** v0.27.0 (breaking user-facing slash-command removal → MINOR per CHANGELOG pre-1.0 policy; Non-Negotiable Rule 6).

**Branch:** `otto/issue-82-single-connect`

## File Map

**New files**
- None.

**Modified files**
- `crates/otto/src/plugin/builtin/connect/mod.rs` — `/connect` ignores args and always opens the picker (dropping the arg-routing + validation path and `args_hint`); add `connect::is_internal_connect_namespace`; refresh module docs; rewrite the outdated `handle_slash` tests.
- `crates/otto/src/plugin/effects.rs` — filter `connect <id>` slashes out of `build_palette_commands` via the connect-plugin predicate; targeted palette regression test.
- `crates/otto/src/providers.rs` — `turn_auth_hint` doc comment and its `/connect gemini --rekey` test assertion refresh.
- `crates/otto/src/plugin/builtin/provider_{anthropic,gemini,openai,deepseek,local}/mod.rs` — module docs/comments/test doc-strings that frame `/connect <id>` as a user command; comments only, no behavior change, registrations and tests kept.
- `crates/otto/src/plugin/builtin/command_palette/screen.rs` — stale `/connect anthropic`/`/connect gemini` collision comment refresh.
- `crates/otto/locales/{en,es,pt,hi}.toml` — `connect-rejected-keyed`, `connect-already`, `turn-auth-failed-hint`, `startup-build-failed`, `startup-timeout`, `use-not-connected` keys point at `/connect` + the picker (Alt+Enter to re-key).
- `README.md` — `/connect` command row documents the picker as the only connect path.
- `docs/superpowers/specs/2026-09-09-issue-82-connect-picker-only-design.md` — already committed design source of truth for this change.
- `docs/superpowers/plans/2026-09-09-issue-82-connect-picker-only.md` — this implementation plan.

## Task 1: `/connect` ignores args and always opens the provider picker

**Files:**
- Modify: `crates/otto/src/plugin/builtin/connect/mod.rs`

- [ ] Read the current `ConnectPlugin::handle_slash` (`crates/otto/src/plugin/builtin/connect/mod.rs`) and confirm that a typed `/connect <provider>` routes arg → `RunSlash { name: "connect {provider}" }` while the picker is opened only on the no-arg path; confirm `args_hint: Some("[provider]")` in `manifest()`.
- [ ] Add failing tests in `crates/otto/src/plugin/builtin/connect/mod.rs`: `handle_slash_always_opens_picker` asserting `handle_slash("connect", vec![])` and `handle_slash("connect", vec!["anthropic"])` (and, for parity, a junk arg like `vec!["bad provider"]`) all yield `Effect::OpenScreen { id: "connect.picker", .. }`. Replace the current `handle_slash_rejects_provider_with_spaces`, `handle_slash_rejects_provider_with_uppercase`, and `handle_slash_accepts_valid_provider_ids` tests (their asserted behavior is being removed). Run `cargo test -p otto connect::tests -- --nocapture` and expect the new/open assertions to fail before implementation.
- [ ] Implement: `handle_slash` ignores `args` entirely and always returns `Ok(vec![Effect::OpenScreen { id: "connect.picker".into(), args: ScreenArgs::ConnectPicker }])`; remove the provider-id validation path and the `connect {id}` `RunSlash` routing; set `args_hint: None` on the manifest's `SlashSpec` (the field is `Option<String>` in `otto-plugin`'s public API — set it to `None`, never remove the struct field); update the crate/module doc comments (which still describe the arg-routing behavior).
- [ ] Add `pub(crate) fn is_internal_connect_namespace(name: &str) -> bool` to `crates/otto/src/plugin/builtin/connect/mod.rs` returning `name.starts_with("connect ")`, with a comment explaining it is the reserved private namespace shared by the picker-dispatch slashes and the palette filter (Task 2). Keep it unit-tested.
- [ ] Record the public-interface check in code review notes: this removes user-facing `/connect <provider>` commands (breaking, Non-Negotiable Rule 6) but changes no SPP wire format, tool schema, plugin ABI surface, env var, or on-disk format; the internal slash names are unchanged.
- [ ] This task touches only `crates/otto/src/plugin/builtin/connect/mod.rs` — not `app.rs`, `tui.rs`, any streaming provider path, or the host transport — so the host-swap `RwLock` rule and the `rmcp` `ProgressDispatcher` forwarder-abort invariant are unaffected; note that explicitly in review. Note that `crates/otto/src/plugin/builtin/connect/screen.rs` is **intentionally untouched**: its picker `Enter`/`Alt+Enter` dispatches of `RunSlash { name: "connect {pid}" }` are exactly the internal plumbing this design preserves.
- [ ] Run `cargo test -p otto connect:: -- --nocapture` again and expect the targeted connect-plugin suite to pass.
- [ ] Run `cargo fmt --all` and commit the task with `git commit -m "otto: /connect always opens the provider picker"`.

## Task 2: Hide the internal `connect <id>` slashes from the command palette

**Files:**
- Modify: `crates/otto/src/plugin/effects.rs`
- Modify: `crates/otto/src/plugin/builtin/connect/mod.rs`

- [ ] Read `build_palette_commands` (`crates/otto/src/plugin/effects.rs`, line ~798) and the existing palette test harness (`palette_commands_use_requires_arg_not_args_hint`, line ~2276, which uses the `register_builtins(...)` pattern) to confirm where the filter belongs and how an index with provider slashes is constructed for tests.
- [ ] Add a failing regression test in `crates/otto/src/plugin/effects.rs`: with the built-in provider plugins registered, `build_palette_commands` must list `connect` but not contain `PaletteCommand { name: "connect anthropic" }` (and explicitly none of the five `connect <id>` names). Confirm `connect_picker_lists_all_provider_plugins` (line ~2338) still passes since it asserts the picker candidates, not the palette. Run `cargo test -p otto plugin::effects -- --nocapture` and expect the new assertion to fail before implementation.
- [ ] Implement: filter `(name, pid)` entries in `build_palette_commands` through `crate::plugin::builtin::connect::is_internal_connect_namespace(&name)` before building `PaletteCommand`s (skip hidden entries), mirroring the existing enable/disable skip behavior.
- [ ] Refresh the `command_palette/screen.rs` comment at lines ~121-122: it explains *why* the palette computes a dynamic column width ("the old fixed `{:<12}` width — `/connect anthropic`, `/connect gemini`, … — collide with their descriptions"). Those entries no longer reach the palette after this task, so update the comment to reflect that the internal-connect namespace is filtered at build time and the dynamic width now only needs to accommodate the shorter remaining command names.
- [ ] Record the public-interface check in code review notes: internal slash names are unchanged and remain dispatchable by `Effect::RunSlash`; the palette is a presentation surface, so filtering it is not itself a public-interface change.
- [ ] This task touches no host/transport/app/tui surface: the host-swap `RwLock` and `ProgressDispatcher` invariants are unaffected; note that explicitly in review.
- [ ] Run `cargo test -p otto plugin::effects -- --nocapture` and expect the targeted effect/palette tests to pass.
- [ ] Run `cargo clippy --workspace --all-targets` to confirm the modified effects path stays clean.
- [ ] Run `cargo fmt --all` and commit the task with `git commit -m "otto: hide internal connect <id> slashes from the command palette"`.

## Task 3: Point connect-related guidance at the picker instead of `/connect <id>`

**Files:**
- Modify: `crates/otto/locales/{en,es,pt,hi}.toml`
- Modify: `crates/otto/src/providers.rs`
- Modify: `crates/otto/src/plugin/builtin/provider_{anthropic,gemini,openai,deepseek,local}/mod.rs` (comments/doc-strings only)
- Modify: `README.md`

- [ ] Read the six affected keys in each of `crates/otto/locales/{en,es,pt,hi}.toml` (`connect-rejected-keyed`, `connect-already`, `turn-auth-failed-hint`, `startup-build-failed`, `startup-timeout`, `use-not-connected`) and note the named params each is rendered with (`%{id}`, `%{name}`, `%{err}`, `%{ms}`).
- [ ] Add a failing assertion in `crates/otto/src/providers.rs`'s `turn_auth_hint_only_for_known_keyed_providers` test: the `gemini` hint must NOT contain `--rekey` or `/connect gemini` and must reference the picker path (e.g. `/connect` and `Alt+Enter`). Run `cargo test -p otto providers::tests::turn_auth_hint -- --nocapture` and expect it to fail before the locale update.
- [ ] Rewrite the six keys in each locale so no string instructs typing a provider-specific `/connect <id>` or a `--rekey` flag: recovery guidance points at `/connect` + the picker, with `Alt+Enter` as the re-key gesture; keep each key's existing named params (dropping an unused `%{id}`/`%{name}` param from a template is fine — `rust_i18n` ignores extras — but the rendered-keys assertions below only reference strings, so verify the parity test in `tests/locales.rs` still passes). Preserve the file's per-key alignment style.
- [ ] Refresh `turn_auth_hint`'s doc comment in `crates/otto/src/providers.rs` (currently says "Render the `--rekey` hint …") to describe the picker/Alt+Enter recovery text it renders, and update the test's assertion to the new `en` string content (must contain `/connect` and the picker/Alt+Enter guidance, and must not contain `--rekey` or `/connect gemini`).
- [ ] Update comments/doc-strings in the five provider plugin modules that frame `connect <id>` as a user-typed command (`On /connect anthropic …`, `user can run /connect anthropic later`, `run /connect local later`, etc.) to say the slash is an internal picker-dispatch name; do not change behavior or the registered slash names, and do not touch their existing tests.
- [ ] Update the `/connect` row in `README.md` (line ~145) so the command column is `/connect` (no `[<provider>]`, no `[--rekey]`) and the description documents the picker as the one way to add/select a provider, keeping the silent-stored-key note and the `Alt+Enter` re-key gesture.
- [ ] Record the public-interface check in code review notes: this task changes no executable interface — the six locale keys' names and params stay stable, so the keyring/transcript/ABI/wire/env surfaces are untouched; the slash-command surface change was already recorded in Task 1.
- [ ] This task touches no app/tui/host/transport code: the host-swap `RwLock` and `ProgressDispatcher` invariants are unaffected; note that explicitly in review.
- [ ] Run `cargo test -p otto providers::tests::turn_auth_hint -- --nocapture`, `cargo test -p otto -- tests::locales` (locale parity), and `cargo test -p otto connect -- --nocapture`; expect the targeted suites to pass.
- [ ] Run `cargo fmt --all` and commit the task with `git commit -m "otto: guide connect recovery through the provider picker"`.
- [ ] Release note for Phase 4: after this feature PR merges, cut a dedicated release PR per `RELEASING.md` to ship the change as v0.27.0; that dedicated release PR, not this feature branch, will bump `workspace.package.version`, every internal `workspace.dependencies` version in `Cargo.toml`, and add the `CHANGELOG.md` entry (calling out the breaking slash-command removal per Non-Negotiable Rule 6).
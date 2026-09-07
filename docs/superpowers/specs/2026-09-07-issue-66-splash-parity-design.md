# Splash screen parity for `/splash` — design

Date: 2026-09-07
Status: pending review
Related: `savvagent/otto#66`

## Problem

`/splash` and the startup splash currently diverge. The startup path renders
the full splash overlay in `crates/otto/src/splash.rs:98-131`, including the
ASCII-art logo, tagline, sandbox status line, and the versioned “press any
key” hint. The `/splash` command instead opens `internal:splash`, whose
`SplashScreen::render` implementation in
`crates/otto/src/plugin/builtin/splash/screen.rs:38-65` returns a different
plain-text screen centered around connect status and an Esc-only dismiss hint.

Issue #66 requires `/splash` to show the same visual splash content as startup,
not a separate simplified screen.

This is not a fast-path change under `otto-development`: it changes runtime
behavior on a visible code path, touches more than two logical source files,
and needs tests to prove the shared rendering path stays aligned.

## Approach

Route both experiences through a single splash-content source instead of
maintaining two independent layouts.

1. Keep `crates/otto/src/splash.rs` as the single source of truth for splash
   content by extracting a helper that produces the shared logo/tagline/sandbox
   lines and versioned hint from the current `SandboxSplashState`. The startup
   `render(frame, area, sandbox)` path should consume that helper rather than
   keeping separate inline content assembly.
2. Update `crates/otto/src/plugin/builtin/splash/screen.rs` so
   `SplashScreen::render` returns that same shared splash content for screen
   stack consumers (including egui), and `SplashScreen::tips()` returns no
   extra footer hint chrome that would diverge from startup.
3. Update `crates/otto/src/ui.rs` so the fullscreen ratatui plugin-screen paint
   path recognizes the `"splash"` screen id and calls `crate::splash::render(...)`
   with the same `SandboxSplashState`, preserving the centered fullscreen
   startup layout for `/splash` in the terminal UI instead of left-aligning the
   shared text lines.
4. Feed the current `app.splash_sandbox` into `/splash` screen creation from
   the screen-open path so the plugin screen and startup overlay both render
   from the same cached, truthfulness-preserving sandbox state.
5. Remove now-unused connect-status-specific splash plugin state from
   `crates/otto/src/plugin/builtin/splash/mod.rs` if the shared renderer no
   longer consumes it.
6. Add targeted tests around shared content generation, the fullscreen
   splash-screen paint special-case, empty tips, and the any-key dismissal
   behavior so future edits cannot silently reintroduce the divergent `/splash`
   view.

## Scope

**In:**
- Shared splash rendering between startup and `/splash`
- `/splash` dismiss behavior needed to match the shared hint text
- Cleanup of splash-plugin-only state that existed solely for the old
  connect-status screen
- Minimal screen-open plumbing needed to inject `app.splash_sandbox` into the
  `/splash` screen instance
- Egui compatibility via the shared `SplashScreen::render` content path
- Targeted Rust tests for the new shared path

**Out:**
- Any change to the startup splash’s literal logo text content
- Any change to host/provider/tool architecture
- Any change to plugin ABI, slash-command names, env vars, or on-disk formats
- Broader screen-stack rendering refactors beyond the splash special-case

## Public-interface changes

None.

- **SPP wire format:** unchanged
- **Tool MCP schemas:** unchanged
- **Plugin ABI:** unchanged
- **Slash commands / env vars / on-disk formats:** unchanged

The `/splash` command keeps the same name and screen id; only its visual output
and dismiss behavior are aligned with the existing startup splash.

## Premise corrections

- The current `/splash` screen is not just missing styling — it is a separate
  connect-status-oriented UI path. Fixing the issue cleanly means reusing one
  shared content source across startup, ratatui screen-stack rendering, and any
  other frontend that consumes `SplashScreen::render`.
- The authoritative sandbox line should come from `App::splash_sandbox`, which
  is already loaded once and refreshed from the active host. Re-reading config
  or synthesizing a second sandbox source for `/splash` would regress the
  truthfulness guarantees documented in `crates/otto/src/splash.rs`.

## Assumptions

- **`/splash` should dismiss on any key, not just Esc/Enter.** Rationale: the
  task explicitly calls out sharing the startup hint, and the startup hint says
  “press any key to continue.”
- **The old `/splash` connect-status line can be removed entirely.** Rationale:
  the issue’s expected behavior is parity with the startup splash, whose visual
  content does not include provider connection state.
- **A small `ui.rs` special-case for the splash screen is acceptable.**
  Rationale: `paint_screen` already owns fullscreen-screen rendering, and
  routing one screen id through the existing renderer avoids inventing a second
  ratatui splash layout.
- **This ships as a PATCH release (`v0.25.3`).** Rationale: it is a bug fix
  with no public-interface change.

## Goal & Success Criteria

Make `/splash` render the same startup splash visuals by reusing the existing
shared splash content and the existing startup renderer, while preserving the
cached sandbox-state truthfulness and keeping the change internal to the TUI
crate.

- [ ] Opening `/splash` paints the same logo, tagline, sandbox line, and
      versioned hint that startup paints.
- [ ] Both startup splash and `/splash` derive their visible content from the
      same shared content helper in `crates/otto/src/splash.rs`, and ratatui
      `/splash` uses the existing centered startup renderer.
- [ ] `/splash` key handling matches the shared hint by closing on any key.
- [ ] `/splash` adds no extra screen tips/footer hint outside the shared splash
      content.
- [ ] `cargo test -p otto splash` and any targeted `ui`/plugin splash tests
      added for this change pass.
- [ ] `cargo build --workspace --all-targets`, `cargo test --workspace
      --no-fail-fast`, `cargo clippy --workspace --all-targets`, and
      `cargo fmt --all --check` pass before the PR is considered ready.

## Error Handling & Edge Cases

- If the splash screen is open from `/splash`, it must still use the current
  `app.splash_sandbox` value rather than falling back to a default or rereading
  disk.
- The splash fullscreen special-case must not affect non-splash screens; other
  screen ids should continue rendering through `Screen::render`.
- Frontends that use `Screen::render` directly must still receive the shared
  splash content even if they do not use the ratatui fullscreen special-case.
- Key handling for `/splash` should remain non-panicking and close cleanly for
  printable and non-printable keys alike.
- `/splash` must not add a second dismiss hint via `tips()` once the shared
  hint is rendered inline.
- Removing the connect-status-specific splash state must not leave dead hook
  subscriptions or stale tests behind.

## Risks & Open Questions

- `paint_screen` currently operates on `&dyn Screen`; adding splash parity via a
  screen-id branch is intentionally narrow, but tests need to guard that future
  refactors do not bypass the shared renderer.
- `/splash` will no longer expose the prior connected/connecting status text.
  That is intentional per the issue, but the change should be explicit in the
  PR summary so reviewers know it is a feature removal in service of parity, not
  an accidental omission.

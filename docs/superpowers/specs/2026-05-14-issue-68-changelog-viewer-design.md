# Changelog Viewer — design

Date: 2026-05-14
Status: pending review
Issue: #68 ("As a user, I want to view the CHANGELOG.md release notes")
Related: #67 (auto-install all release binaries) — ships in the same release.

## Problem

Users who upgrade otto currently have no in-band way to see what
changed. The CHANGELOG.md lives at the repo root on GitHub; reading it
requires switching to a browser. With `/update` going automatic in this
release (issue #67), discoverability of "what just landed" matters more
— the upgrade is silent on success, so the user only sees a "Updated to
v0.X" banner and a restart hint.

## Approach

A single new built-in plugin, `internal:changelog`, owns the feature. The
plugin:

1. Registers one slash command, `/changelog`, that emits an
   `Effect::OpenScreen` for a screen the same plugin contributes.
2. On screen open, spawns a tokio task to fetch
   `https://raw.githubusercontent.com/robhicks/otto-rs/master/CHANGELOG.md`.
   The screen renders a "Fetching changelog…" placeholder while the
   request is in flight.
3. When the fetch resolves, the screen swaps to a `Loaded` state holding
   a `Vec<StyledLine>` produced by parsing the markdown with
   [`tui-markdown`](https://crates.io/crates/tui-markdown) and
   translating its `ratatui::text::Text` output line-by-line into the
   plugin's `StyledLine` / `StyledSpan` vocabulary (a small adapter in
   `screen.rs`; ratatui `Style` → `StyledSpan { fg, bg, modifiers }` is
   field-for-field). On HTTP/IO failure the screen swaps to a
   `Failed { error }` state and renders an inline error line plus a
   retry hint.
4. The screen handles its own scroll/key input — `j/k`, `↑/↓`,
   `PageUp/PageDn`, `g/G`, `Esc/q`, and `r` (retry on the failed state).
   The transcript is unaffected; the screen overlays the home view and
   `Esc` returns the user to where they were.

A per-session in-memory cache keeps the rendered markdown so re-opening
the screen within the session doesn't re-hit the network. The cache
clears on process exit.

## Why a plugin (not direct ui.rs surgery)

Per the project memory's "new TUI features must be plugins" rule, every
new modal/screen goes through the otto-plugin trait surface. The
existing `internal:view-file` plugin is the closest analog: a slash
command + a screen marker. The CHANGELOG plugin differs in two ways:

- **Self-rendering.** `view-file` is a marker that delegates rendering
  to the editor widget driven by `ui.rs`. The changelog has no need
  for an editor; it's a pure read-only document with markdown styling.
  The plugin's `Screen::render` returns a `Vec<StyledLine>` (the
  translated markdown, scroll-windowed to the visible region) so
  `ui.rs` doesn't gain a new code path.
- **Stateful screen.** `view-file` is stateless (`#[derive(Default)]`).
  The changelog screen owns its `Loading | Loaded | Failed` state plus
  the scroll offset.

## Module layout

```
crates/otto/src/plugin/builtin/changelog/
├── mod.rs       # ChangelogPlugin: manifest, handle_slash, create_screen
├── fetch.rs     # ChangelogFetcher trait + reqwest-backed impl
└── screen.rs    # ChangelogScreen: state machine, render, on_key
```

Mirrors `view_file/{mod.rs, screen.rs}` plus a `fetch.rs` for the
testable network seam — same shape as `self_update/{mod, check, apply,
cache}.rs`.

## Data flow

1. User types `/changelog` and presses Enter.
2. `ChangelogPlugin::handle_slash` returns
   `Effect::OpenScreen { id: "changelog", args: ScreenArgs::None }`.
3. The runtime calls `ChangelogPlugin::create_screen`, which constructs
   a `ChangelogScreen` initialized in the `Loading` state and spawns a
   tokio task with an `Arc<Mutex<ChangelogState>>` handle. The task
   awaits `fetcher.fetch().await`, then writes either `Loaded { text }`
   or `Failed { error }` into the shared state.
4. `Screen::render` reads the shared state via `try_lock`. The render
   path is non-blocking: if the spawned task is mid-write the screen
   draws the previous frame. (Same pattern as `SelfUpdatePlugin`.)
5. On `r` from the `Failed` state, the screen resets state to `Loading`
   and re-spawns the same fetch task.
6. On `Esc/q`, the screen emits `Effect::CloseScreen`.

## Wire surface (otto-plugin)

A new `ScreenArgs::Changelog` variant is added; opening the screen
takes no parameters today. (The variant exists rather than reusing
`ScreenArgs::None` so the screen-id resolution table in
`ScreenArgs::screen_id()` stays exhaustive and future-proof — e.g., a
later "scroll to v0.13.0" feature can land args without breaking the
public surface.)

## Locales

New `[changelog]` section in `crates/otto/locales/{en,es,hi,pt}.toml`:

| key | English |
|---|---|
| `changelog.slash-summary` | "Show release notes from CHANGELOG.md" |
| `changelog.loading` | "Fetching changelog…" |
| `changelog.fetch-failed` | "Couldn't fetch changelog: %{err}" |
| `changelog.retry-hint` | "Press r to retry, Esc to close" |
| `changelog.tips` | "j/k scroll · g/G top/bottom · r retry · Esc close" |
| `plugin.changelog-description` | "View the project's release notes" |

## Dependencies added

- `tui-markdown = "0.3"` (workspace dep) — markdown → `ratatui::text::Text`.
  Pulls only `ratatui-core`; we already pin ratatui at the workspace level.

No new runtime requirements (`reqwest` is already in the workspace).

## Error handling

- **DNS / TCP / TLS error.** Surface the underlying error string in the
  screen body; offer `r` to retry. The screen does NOT push a transcript
  note on failure — errors stay in the screen's own surface so the
  transcript is preserved as-is.
- **HTTP non-2xx (e.g., GitHub returns 404 if the URL ever changes).**
  Same path as IO failure; the error message includes the status code.
- **Markdown parse panic from `tui-markdown`.** Caught via
  `std::panic::catch_unwind` around the parse call; surfaces as a
  `Failed { error: "rendering error" }` state rather than tearing down
  the TUI.

## Testing strategy

- **Unit tests in `mod.rs`.** Manifest registration (slash + screen
  contributions), `handle_slash` returns `OpenScreen`, opt-out of
  unrelated commands.
- **Unit tests in `screen.rs`.** State transitions
  (`Loading → Loaded`, `Loading → Failed`, `Failed + r → Loading`),
  scroll-offset clamping, key dispatch, `Esc/q → CloseScreen`. A
  trait-injected `ChangelogFetcher` keeps the network out of the suite,
  same pattern as `ReleasesFetcher` in `self_update`.
- **Unit tests in `fetch.rs`.** Confirm the production fetcher targets
  the documented URL and sets the otto User-Agent. The
  reqwest-backed call itself is not exercised in unit tests; an
  integration test with `wiremock` could be added later if flakes show
  up, but the URL/UA assertions cover the regression surface.

No integration tests live in this PR — the existing `tests/` suite has
no markdown-rendering harness and adding one is scope creep.

## What's explicitly out of scope

- Auto-opening the viewer after a self-update (decided in
  brainstorming — keep the surface minimal).
- "Last seen version" persistence / "what's new since you last looked"
  filtering. Same reason.
- Offline fallback (`include_str!` of CHANGELOG.md). Pure-network for
  now; revisit if telemetry shows users hit the failure path often.
- Tag pinning. The viewer always shows `master`'s CHANGELOG so users
  see entries for releases they haven't installed yet.
- Search / find-in-page. Esc → close → ask the agent is a viable
  workaround for v1.

## Release coupling

This PR ships in the same release as #67. Both PRs land on master, then
a third PR bumps `[workspace.package].version` (per memory's SemVer
policy this is a feature → MINOR bump, e.g. 0.13.0), updates CHANGELOG
with two entries (auto-install + changelog viewer), and refreshes
README's slash-command table.

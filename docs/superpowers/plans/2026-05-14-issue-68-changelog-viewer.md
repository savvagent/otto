# Changelog Viewer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the `internal:changelog` plugin: a new `/changelog` slash command opens a dedicated screen that streams `https://raw.githubusercontent.com/robhicks/otto-rs/master/CHANGELOG.md`, renders it via `tui-markdown` translated into the plugin crate's `StyledLine` vocabulary, and supports scroll/retry/close keybindings. Per-session in-memory cache; no auto-open; closes #68.

**Architecture:** A single new built-in plugin under `crates/otto/src/plugin/builtin/changelog/` with three files (`mod.rs` for the plugin shell, `fetch.rs` for the network seam, `screen.rs` for the state machine + render). One new variant on `ScreenArgs` in `otto-plugin`. Spawned tokio task on screen open writes into `Arc<Mutex<ChangelogState>>`; render reads via `try_lock` (same non-blocking pattern as `SelfUpdatePlugin`). Markdown → `Vec<StyledLine>` is a self-contained adapter inside `screen.rs`.

**Tech Stack:** `tui-markdown 0.3` (workspace dep, `default-features = false` to skip the `syntect` + `ansi-to-tui` highlight-code stack), `reqwest` (already in workspace), `tokio` (already in workspace), the existing `otto-plugin` `Screen` trait. Project's stable toolchain (`rustup run stable`) is the test/lint reference per project memory.

**Spec:** `docs/superpowers/specs/2026-05-14-issue-68-changelog-viewer-design.md`

**Branch:** `feat/issue-68-changelog-viewer` (already created off master).

---

## File Map

**New files**
- `crates/otto/src/plugin/builtin/changelog/mod.rs` — plugin shell: manifest, `handle_slash`, `create_screen`, plugin-level tests.
- `crates/otto/src/plugin/builtin/changelog/fetch.rs` — `ChangelogFetcher` trait + `GithubChangelogFetcher` reqwest impl + URL/UA tests.
- `crates/otto/src/plugin/builtin/changelog/screen.rs` — `ChangelogState` enum, `ChangelogScreen` struct, `Screen` impl (state transitions, scroll math, key dispatch, markdown→`StyledLine` adapter), screen-level tests.

**Modified files**
- `Cargo.toml` (workspace root) — add `tui-markdown` to `[workspace.dependencies]`.
- `crates/otto/Cargo.toml` — pull `tui-markdown.workspace = true`.
- `crates/otto-plugin/src/types.rs` — add `ScreenArgs::Changelog` variant + matching arm in `screen_id()` + extend the existing exhaustive test.
- `crates/otto/src/plugin/builtin/mod.rs` — `pub mod changelog;` declaration.
- `crates/otto/src/plugin/mod.rs` — register `ChangelogPlugin::new()` in the `plugins` vec inside `builtin_set()`.
- `crates/otto/locales/{en,es,hi,pt}.toml` — new `[changelog]` section + `plugin.changelog-description` key.

---

## Task 1: Add `tui-markdown` workspace dep

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/otto/Cargo.toml`

- [ ] **Step 1: Add to workspace `[dependencies]` table**

In `Cargo.toml`, locate the existing `tui-textarea = …` line in `[workspace.dependencies]` and add directly above it:

```toml
# Markdown → ratatui Text renderer used by `internal:changelog`. We turn
# off `highlight-code` (the default feature) to avoid pulling in syntect
# + ansi-to-tui — CHANGELOG.md doesn't need source-level syntax
# highlighting, and the highlighters add ~MB to the binary.
tui-markdown = { version = "0.3", default-features = false }
```

- [ ] **Step 2: Pull the dep into the otto crate**

In `crates/otto/Cargo.toml`, find the line `tui-textarea.workspace = true` and add directly above it:

```toml
tui-markdown.workspace = true
```

- [ ] **Step 3: Verify the dep resolves**

Run: `rustup run stable cargo check -p otto 2>&1 | tail -5`
Expected: `Finished `dev` profile …`. (No warnings about unresolved deps.)

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock crates/otto/Cargo.toml
git commit -m "deps: add tui-markdown 0.3 (workspace)

Pulls the markdown→ratatui-Text renderer used by the upcoming
internal:changelog plugin. default-features = false skips the
highlight-code stack (syntect + ansi-to-tui) — the changelog has too
few code blocks to justify the binary bloat."
```

---

## Task 2: Add `ScreenArgs::Changelog` variant

**Files:**
- Modify: `crates/otto-plugin/src/types.rs`

- [ ] **Step 1: Extend the exhaustive `screen_id` test (failing first)**

In `crates/otto-plugin/src/types.rs`, locate the test
`fn screen_args_screen_id_pairs_every_non_none_variant()` (around line 529) and add a new assertion at the bottom of the function, just before the closing brace:

```rust
        assert_eq!(
            ScreenArgs::Changelog.screen_id(),
            Some("changelog")
        );
```

- [ ] **Step 2: Run the test to confirm it fails**

Run: `rustup run stable cargo test -p otto-plugin screen_args_screen_id_pairs_every_non_none_variant 2>&1 | tail -10`
Expected: compile error — `no variant or associated item named 'Changelog' found for enum 'ScreenArgs'`.

- [ ] **Step 3: Add the variant**

In the same file, locate the `ScreenArgs` enum (starts around line 280) and add a new variant after `ModelPicker { … }` (last variant, before the closing `}` of the enum):

```rust
    /// Open the changelog viewer; takes no parameters today.
    ///
    /// The variant exists rather than reusing [`ScreenArgs::None`] so the
    /// `screen_id()` table stays exhaustive — a future "scroll to a
    /// specific version" feature can land an arg here without breaking
    /// the public surface.
    Changelog,
```

- [ ] **Step 4: Add the screen_id arm**

Still in the same file, in `impl ScreenArgs::screen_id`, add the matching arm before the closing brace of the `match`:

```rust
            ScreenArgs::Changelog => Some("changelog"),
```

- [ ] **Step 5: Run the test, confirm pass**

Run: `rustup run stable cargo test -p otto-plugin screen_args_screen_id_pairs_every_non_none_variant 2>&1 | tail -5`
Expected: `test result: ok. 1 passed; …`.

- [ ] **Step 6: Run the rest of the otto-plugin tests to confirm no fallout**

Run: `rustup run stable cargo test -p otto-plugin 2>&1 | tail -5`
Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add crates/otto-plugin/src/types.rs
git commit -m "plugin: add ScreenArgs::Changelog variant for #68

Pairs with the upcoming internal:changelog plugin's screen id
\"changelog\". Carries no parameters today; defining a dedicated variant
(rather than reusing ScreenArgs::None) keeps screen_id() exhaustive
and leaves room for future args (e.g. scroll-to-version)."
```

---

## Task 3: `changelog/fetch.rs` — `ChangelogFetcher` trait + GitHub impl

**Files:**
- Create: `crates/otto/src/plugin/builtin/changelog/fetch.rs`

- [ ] **Step 1: Create the module file with the trait + production impl + tests**

Create `crates/otto/src/plugin/builtin/changelog/fetch.rs` with:

```rust
//! Network seam for `internal:changelog`.
//!
//! The screen invokes [`ChangelogFetcher::fetch`] to pull the latest
//! `CHANGELOG.md` from GitHub. The trait keeps the reqwest call out of
//! unit tests; production uses [`GithubChangelogFetcher`] and tests
//! substitute a stub.

use async_trait::async_trait;

/// URL of the canonical CHANGELOG.md. Streams from `master` so the
/// viewer always reflects the most recent release — including entries
/// for versions the user hasn't installed yet.
pub const CHANGELOG_URL: &str =
    "https://raw.githubusercontent.com/robhicks/otto-rs/master/CHANGELOG.md";

/// User-Agent value sent with the request. Includes the running binary
/// version so request logs identify the caller cohort, mirroring the
/// pattern used in [`crate::plugin::builtin::self_update::check`].
const USER_AGENT: &str = concat!("otto-rs/", env!("CARGO_PKG_VERSION"), " (changelog)");

#[async_trait]
pub trait ChangelogFetcher: Send + Sync {
    /// Return the raw markdown content of CHANGELOG.md, or an
    /// [`anyhow::Error`] on any failure (network, non-2xx, parse).
    async fn fetch(&self) -> anyhow::Result<String>;
}

/// Production [`ChangelogFetcher`] backed by `reqwest`.
#[derive(Debug, Default)]
pub struct GithubChangelogFetcher;

#[async_trait]
impl ChangelogFetcher for GithubChangelogFetcher {
    async fn fetch(&self) -> anyhow::Result<String> {
        let resp = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()?
            .get(CHANGELOG_URL)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.text().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_targets_raw_master_branch() {
        // Regression guard: pinning to a tag would freeze the viewer at
        // the build's commit and stop users from seeing entries for
        // versions they haven't installed yet.
        assert!(
            CHANGELOG_URL.starts_with("https://raw.githubusercontent.com/robhicks/otto-rs/"),
            "URL must hit raw.githubusercontent.com: {CHANGELOG_URL}"
        );
        assert!(
            CHANGELOG_URL.ends_with("/master/CHANGELOG.md"),
            "URL must reference master/CHANGELOG.md: {CHANGELOG_URL}"
        );
    }

    #[test]
    fn user_agent_identifies_otto() {
        assert!(USER_AGENT.contains("otto"));
        assert!(USER_AGENT.contains("changelog"));
    }
}
```

- [ ] **Step 2: Run the unit tests (file isn't wired into a `mod` yet so this needs the parent `mod.rs` first — defer the run to Task 4)**

Skip running tests for this task in isolation; they'll run as part of Task 5 once `mod.rs` declares the submodule.

- [ ] **Step 3: Commit**

```bash
git add crates/otto/src/plugin/builtin/changelog/fetch.rs
git commit -m "feat(changelog): add fetcher trait + reqwest-backed impl (#68)

Pure network seam: ChangelogFetcher::fetch() returns the raw
CHANGELOG.md markdown. Production hits raw.githubusercontent.com on
master so users see entries for versions they haven't installed.
Tests assert the URL pins to master and that the User-Agent
identifies the caller cohort."
```

---

## Task 4: `changelog/screen.rs` — state enum + struct + state-transition tests

**Files:**
- Create: `crates/otto/src/plugin/builtin/changelog/screen.rs`

- [ ] **Step 1: Create the file with state types, struct, constructor, and the in-test stub fetcher**

Create `crates/otto/src/plugin/builtin/changelog/screen.rs` with:

```rust
//! `internal:changelog` screen: state machine + render + key handling.
//!
//! The screen is opened by [`super::ChangelogPlugin::create_screen`] in
//! the [`ChangelogState::Loading`] state. `create_screen` spawns a tokio
//! task that calls [`super::fetch::ChangelogFetcher::fetch`] and writes
//! the result into the shared state ([`ChangelogState::Loaded`] on
//! success, [`ChangelogState::Failed`] on error). The user can press
//! `r` from `Failed` to re-spawn the fetch; this module exposes
//! [`ChangelogScreen::reset_to_loading`] so the plugin can re-spawn
//! externally without `screen.rs` taking a fetcher dependency.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use otto_plugin::{
    Effect, KeyCodePortable, KeyEventPortable, PluginError, Region, Screen, StyledLine,
    StyledSpan, TextMods,
};

/// What the screen is currently showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangelogState {
    /// Fetch is in flight; render shows a placeholder.
    Loading,
    /// Fetch succeeded; render shows the markdown-translated lines.
    Loaded {
        /// Translated markdown ready to paint.
        lines: Vec<StyledLine>,
    },
    /// Fetch failed; render shows the error and the retry hint.
    Failed {
        /// Underlying error string surfaced verbatim to the user.
        error: String,
    },
}

/// The screen instance pushed onto the screen stack.
pub struct ChangelogScreen {
    /// Shared with the spawned fetch task. Read by `render` via
    /// `try_lock` so the render hot path never blocks; written by the
    /// task once the fetch resolves and by `on_key` on retry.
    pub(crate) state: Arc<Mutex<ChangelogState>>,
    /// Top-line index inside the currently-rendered line buffer.
    /// Bumped by scroll keys, clamped at render time so a `Loading →
    /// Loaded` transition with fewer lines than the previous offset
    /// can't render an empty screen.
    scroll_offset: usize,
}

impl ChangelogScreen {
    /// Construct a new screen sharing the supplied state cell. The
    /// caller (the plugin) is expected to also hand the same `Arc` to
    /// the spawned fetch task so it can publish the result.
    pub fn new(state: Arc<Mutex<ChangelogState>>) -> Self {
        Self {
            state,
            scroll_offset: 0,
        }
    }

    /// Reset the shared state cell to `Loading` and zero the scroll
    /// offset. Called from `on_key` when the user presses `r` while
    /// in `Failed`; the plugin's retry path also calls this from the
    /// outside before re-spawning the fetch task.
    pub(crate) fn reset_to_loading(&mut self) {
        *self.state.lock().unwrap() = ChangelogState::Loading;
        self.scroll_offset = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded_state(n_lines: usize) -> ChangelogState {
        ChangelogState::Loaded {
            lines: (0..n_lines)
                .map(|i| StyledLine {
                    spans: vec![StyledSpan {
                        text: format!("line {i}"),
                        fg: None,
                        bg: None,
                        modifiers: TextMods::default(),
                    }],
                })
                .collect(),
        }
    }

    #[test]
    fn new_screen_starts_in_loading() {
        let state = Arc::new(Mutex::new(ChangelogState::Loading));
        let screen = ChangelogScreen::new(Arc::clone(&state));
        assert_eq!(*screen.state.lock().unwrap(), ChangelogState::Loading);
        assert_eq!(screen.scroll_offset, 0);
    }

    #[test]
    fn reset_to_loading_from_failed_clears_state_and_scroll() {
        let state = Arc::new(Mutex::new(ChangelogState::Failed {
            error: "DNS error".into(),
        }));
        let mut screen = ChangelogScreen::new(Arc::clone(&state));
        screen.scroll_offset = 42;

        screen.reset_to_loading();

        assert_eq!(*screen.state.lock().unwrap(), ChangelogState::Loading);
        assert_eq!(screen.scroll_offset, 0);
    }

    #[test]
    fn reset_to_loading_from_loaded_also_clears_state_and_scroll() {
        let state = Arc::new(Mutex::new(loaded_state(10)));
        let mut screen = ChangelogScreen::new(Arc::clone(&state));
        screen.scroll_offset = 3;

        screen.reset_to_loading();

        assert_eq!(*screen.state.lock().unwrap(), ChangelogState::Loading);
        assert_eq!(screen.scroll_offset, 0);
    }
}
```

- [ ] **Step 2: Defer running the tests until `mod.rs` exists in Task 5; this file is not yet a recognised submodule.**

(Tasks 4–7 all extend `screen.rs`; tests run together at the end of Task 7.)

- [ ] **Step 3: Commit**

```bash
git add crates/otto/src/plugin/builtin/changelog/screen.rs
git commit -m "feat(changelog): screen state enum + transitions (#68)

ChangelogState (Loading | Loaded | Failed), ChangelogScreen owning a
shared Arc<Mutex<state>>, and reset_to_loading() for the retry path.
Tests cover initial state and reset transitions from both Failed and
Loaded."
```

---

## Task 5: `changelog/screen.rs` — `Screen::on_key` keybindings

**Files:**
- Modify: `crates/otto/src/plugin/builtin/changelog/screen.rs`

- [ ] **Step 1: Add `on_key` tests**

Append the following tests inside the existing `mod tests` block in `screen.rs` (just before the closing `}`):

```rust
    fn key(c: KeyCodePortable) -> KeyEventPortable {
        KeyEventPortable {
            code: c,
            modifiers: otto_plugin::KeyMods::default(),
        }
    }

    async fn screen_with(state: ChangelogState) -> ChangelogScreen {
        ChangelogScreen::new(Arc::new(Mutex::new(state)))
    }

    #[tokio::test]
    async fn esc_emits_close_screen() {
        let mut s = screen_with(ChangelogState::Loading).await;
        let effs = s.on_key(key(KeyCodePortable::Esc)).await.unwrap();
        assert!(matches!(effs[0], Effect::CloseScreen));
    }

    #[tokio::test]
    async fn q_emits_close_screen() {
        let mut s = screen_with(ChangelogState::Loading).await;
        let effs = s.on_key(key(KeyCodePortable::Char('q'))).await.unwrap();
        assert!(matches!(effs[0], Effect::CloseScreen));
    }

    #[tokio::test]
    async fn down_and_j_increment_scroll_offset() {
        let mut s = screen_with(loaded_state(50)).await;
        s.on_key(key(KeyCodePortable::Down)).await.unwrap();
        assert_eq!(s.scroll_offset, 1);
        s.on_key(key(KeyCodePortable::Char('j'))).await.unwrap();
        assert_eq!(s.scroll_offset, 2);
    }

    #[tokio::test]
    async fn up_and_k_decrement_scroll_offset_with_clamp_at_zero() {
        let mut s = screen_with(loaded_state(50)).await;
        s.scroll_offset = 2;
        s.on_key(key(KeyCodePortable::Up)).await.unwrap();
        s.on_key(key(KeyCodePortable::Char('k'))).await.unwrap();
        assert_eq!(s.scroll_offset, 0);
        // Pressing again must NOT underflow.
        s.on_key(key(KeyCodePortable::Up)).await.unwrap();
        assert_eq!(s.scroll_offset, 0);
    }

    #[tokio::test]
    async fn page_down_advances_by_page_size() {
        let mut s = screen_with(loaded_state(200)).await;
        s.on_key(key(KeyCodePortable::PageDown)).await.unwrap();
        assert_eq!(s.scroll_offset, super::PAGE_SIZE);
    }

    #[tokio::test]
    async fn page_up_retreats_by_page_size_with_clamp() {
        let mut s = screen_with(loaded_state(200)).await;
        s.scroll_offset = 5;
        s.on_key(key(KeyCodePortable::PageUp)).await.unwrap();
        assert_eq!(s.scroll_offset, 0);
    }

    #[tokio::test]
    async fn lower_g_jumps_to_top_and_upper_g_jumps_to_bottom() {
        let mut s = screen_with(loaded_state(50)).await;
        s.on_key(key(KeyCodePortable::Char('G'))).await.unwrap();
        // 'G' sets the offset to last-line-index; render-time clamp
        // narrows further once region.height is known.
        assert_eq!(s.scroll_offset, 49);
        s.on_key(key(KeyCodePortable::Char('g'))).await.unwrap();
        assert_eq!(s.scroll_offset, 0);
    }

    #[tokio::test]
    async fn r_in_failed_state_resets_to_loading() {
        let mut s = screen_with(ChangelogState::Failed {
            error: "boom".into(),
        })
        .await;
        s.scroll_offset = 7;

        let effs = s.on_key(key(KeyCodePortable::Char('r'))).await.unwrap();

        // Plugin observes the reset-to-loading by polling state; the
        // screen itself does not emit a re-spawn effect (no such effect
        // exists). It just emits an empty effects vec; the spawned
        // fetch task is restarted by the plugin's own retry hook.
        assert!(effs.is_empty());
        assert_eq!(*s.state.lock().unwrap(), ChangelogState::Loading);
        assert_eq!(s.scroll_offset, 0);
    }

    #[tokio::test]
    async fn r_outside_failed_state_is_ignored() {
        let mut s = screen_with(loaded_state(10)).await;
        s.scroll_offset = 3;
        let effs = s.on_key(key(KeyCodePortable::Char('r'))).await.unwrap();
        assert!(effs.is_empty());
        // No state change.
        assert!(matches!(*s.state.lock().unwrap(), ChangelogState::Loaded { .. }));
        assert_eq!(s.scroll_offset, 3);
    }
```

- [ ] **Step 2: Add `PAGE_SIZE` constant + `on_key` impl**

Above the `impl ChangelogScreen { … }` block in `screen.rs`, insert:

```rust
/// Number of lines a `PageUp` / `PageDown` press advances the scroll
/// offset by. Chosen to feel snappy on the typical 80×24 terminal
/// without skipping an entire CHANGELOG section.
pub(crate) const PAGE_SIZE: usize = 10;
```

Then below the existing `impl ChangelogScreen` block, append a `Screen` trait impl with the on_key handler (other trait methods are added in later tasks; we keep them stubbed for now):

```rust
#[async_trait]
impl Screen for ChangelogScreen {
    fn id(&self) -> String {
        "changelog".to_string()
    }

    fn render(&self, _region: Region) -> Vec<StyledLine> {
        // Filled in by Task 6.
        vec![]
    }

    async fn on_key(&mut self, key: KeyEventPortable) -> Result<Vec<Effect>, PluginError> {
        match key.code {
            KeyCodePortable::Esc | KeyCodePortable::Char('q') => Ok(vec![Effect::CloseScreen]),

            KeyCodePortable::Down | KeyCodePortable::Char('j') => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
                Ok(vec![])
            }
            KeyCodePortable::Up | KeyCodePortable::Char('k') => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                Ok(vec![])
            }
            KeyCodePortable::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_add(PAGE_SIZE);
                Ok(vec![])
            }
            KeyCodePortable::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(PAGE_SIZE);
                Ok(vec![])
            }
            KeyCodePortable::Char('g') => {
                self.scroll_offset = 0;
                Ok(vec![])
            }
            KeyCodePortable::Char('G') => {
                let len = match &*self.state.lock().unwrap() {
                    ChangelogState::Loaded { lines } => lines.len(),
                    _ => 0,
                };
                self.scroll_offset = len.saturating_sub(1);
                Ok(vec![])
            }
            KeyCodePortable::Char('r') => {
                if matches!(&*self.state.lock().unwrap(), ChangelogState::Failed { .. }) {
                    self.reset_to_loading();
                }
                Ok(vec![])
            }
            _ => Ok(vec![]),
        }
    }

    fn tips(&self) -> Vec<StyledLine> {
        vec![StyledLine::plain(
            rust_i18n::t!("changelog.tips").to_string(),
        )]
    }
}
```

- [ ] **Step 3: Defer test run to Task 7 once `mod.rs` exists.**

(The submodule isn't visible to the compiler until Task 8.)

- [ ] **Step 4: Commit**

```bash
git add crates/otto/src/plugin/builtin/changelog/screen.rs
git commit -m "feat(changelog): screen on_key + PAGE_SIZE + tips (#68)

Adds the keybinding surface: j/↓ + k/↑ line scroll (saturating),
PageUp/PageDown by PAGE_SIZE=10 lines, g/G top/bottom, Esc/q close,
r retry (only meaningful in Failed). render() is still stubbed —
filled in by the next commit."
```

---

## Task 6: `changelog/screen.rs` — `Screen::render` with scroll windowing

**Files:**
- Modify: `crates/otto/src/plugin/builtin/changelog/screen.rs`

- [ ] **Step 1: Add render tests inside the existing `mod tests`**

Append at the bottom of the existing test module (before the closing `}`):

```rust
    fn region(width: u16, height: u16) -> Region {
        Region {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    fn rust_i18n_init() {
        // Each render branch consults rust-i18n; pin the locale so
        // string assertions are deterministic.
        rust_i18n::set_locale("en");
    }

    #[test]
    fn render_loading_returns_localized_placeholder() {
        rust_i18n_init();
        let s = ChangelogScreen::new(Arc::new(Mutex::new(ChangelogState::Loading)));
        let lines = s.render(region(80, 24));
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].spans[0].text.to_lowercase().contains("fetch")
                || lines[0].spans[0].text.to_lowercase().contains("load"),
            "loading placeholder must mention fetching/loading: {:?}",
            lines[0].spans[0].text
        );
    }

    #[test]
    fn render_failed_includes_error_and_retry_hint() {
        rust_i18n_init();
        let s = ChangelogScreen::new(Arc::new(Mutex::new(ChangelogState::Failed {
            error: "boom".into(),
        })));
        let lines = s.render(region(80, 24));
        let combined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.text.clone()))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(combined.contains("boom"), "got: {combined}");
        assert!(
            combined.to_lowercase().contains("retry") || combined.contains("r "),
            "expected retry hint: {combined}"
        );
    }

    #[test]
    fn render_loaded_returns_visible_window_starting_at_scroll_offset() {
        let s = ChangelogScreen::new(Arc::new(Mutex::new(loaded_state(50))));
        // Default offset = 0, region height = 5.
        let lines = s.render(region(80, 5));
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0].spans[0].text, "line 0");
        assert_eq!(lines[4].spans[0].text, "line 4");
    }

    #[test]
    fn render_loaded_respects_scroll_offset() {
        let mut s = ChangelogScreen::new(Arc::new(Mutex::new(loaded_state(50))));
        s.scroll_offset = 10;
        let lines = s.render(region(80, 3));
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].spans[0].text, "line 10");
        assert_eq!(lines[2].spans[0].text, "line 12");
    }

    #[test]
    fn render_loaded_clamps_scroll_offset_when_past_end() {
        // scroll_offset bigger than the line count must not blow up;
        // render returns an empty window rather than panicking.
        let s = {
            let mut sc = ChangelogScreen::new(Arc::new(Mutex::new(loaded_state(5))));
            sc.scroll_offset = 99;
            sc
        };
        let lines = s.render(region(80, 10));
        assert!(
            lines.is_empty(),
            "render must clamp / return empty when scrolled past end, got {} lines",
            lines.len()
        );
    }
```

- [ ] **Step 2: Replace the stub `render` with a real implementation**

In the same file, replace the previous `fn render(&self, _region: Region) -> Vec<StyledLine> { vec![] }` with:

```rust
    fn render(&self, region: Region) -> Vec<StyledLine> {
        // try_lock keeps the render hot path non-blocking — if the
        // spawned fetch task is mid-write, draw an empty frame and the
        // banner shows on the next.
        let Ok(guard) = self.state.try_lock() else {
            return vec![];
        };
        match &*guard {
            ChangelogState::Loading => vec![StyledLine::plain(
                rust_i18n::t!("changelog.loading").to_string(),
            )],
            ChangelogState::Failed { error } => vec![
                StyledLine::plain(
                    rust_i18n::t!("changelog.fetch-failed", err = error.clone()).to_string(),
                ),
                StyledLine::plain(rust_i18n::t!("changelog.retry-hint").to_string()),
            ],
            ChangelogState::Loaded { lines } => {
                let visible = region.height as usize;
                if visible == 0 || self.scroll_offset >= lines.len() {
                    return vec![];
                }
                let end = (self.scroll_offset + visible).min(lines.len());
                lines[self.scroll_offset..end].to_vec()
            }
        }
    }
```

- [ ] **Step 3: Defer test run to Task 7 once `mod.rs` exists.**

- [ ] **Step 4: Commit**

```bash
git add crates/otto/src/plugin/builtin/changelog/screen.rs
git commit -m "feat(changelog): screen render with scroll windowing (#68)

Loading → localized placeholder; Failed → error line + retry hint;
Loaded → visible window slice [scroll_offset .. min(end, len)]. Render
clamps a past-the-end scroll offset to an empty Vec rather than
panicking, so a Loading→Loaded transition with fewer lines than the
prior offset is safe."
```

---

## Task 7: `changelog/screen.rs` — markdown → `Vec<StyledLine>` adapter

**Files:**
- Modify: `crates/otto/src/plugin/builtin/changelog/screen.rs`

- [ ] **Step 1: Add adapter tests**

Append inside the existing `mod tests` block:

```rust
    #[test]
    fn markdown_to_styled_lines_renders_bold_heading_with_modifier() {
        let lines = super::markdown_to_styled_lines("# Hello\n\nbody");
        let combined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.text.clone()))
            .collect::<Vec<_>>()
            .join("");
        assert!(combined.contains("Hello"), "got: {combined}");
        assert!(combined.contains("body"), "got: {combined}");

        // The heading line must carry a bold span somewhere.
        let heading_line = lines
            .iter()
            .find(|l| l.spans.iter().any(|s| s.text.contains("Hello")))
            .expect("must find a line containing the heading text");
        assert!(
            heading_line.spans.iter().any(|s| s.modifiers.bold),
            "heading line must contain at least one bold span: {heading_line:?}"
        );
    }

    #[test]
    fn markdown_to_styled_lines_handles_empty_input() {
        let lines = super::markdown_to_styled_lines("");
        // Empty input is allowed; the adapter must not panic. A zero-or-
        // one-line result is acceptable depending on tui-markdown's
        // emit-an-empty-line behavior.
        assert!(lines.len() <= 1, "got {} lines", lines.len());
    }

    #[test]
    fn markdown_to_styled_lines_translates_unicode_text_intact() {
        let lines = super::markdown_to_styled_lines("café 漢字 🚀");
        let combined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.text.clone()))
            .collect::<Vec<_>>()
            .join("");
        assert!(combined.contains("café"), "got: {combined}");
        assert!(combined.contains("漢字"), "got: {combined}");
        assert!(combined.contains("🚀"), "got: {combined}");
    }
```

- [ ] **Step 2: Implement the adapter**

In `screen.rs`, append at the bottom of the file (after the `Screen` impl, before the `#[cfg(test)] mod tests` block):

```rust
/// Translate a markdown string into the plugin crate's
/// [`StyledLine`] vocabulary. Uses [`tui_markdown::from_str`] to do
/// the markdown parsing and styled-text emission, then converts each
/// ratatui `Line` / `Span` into the equivalent `StyledLine` /
/// `StyledSpan`. Pure function; defensive against panics inside the
/// markdown crate (a corrupt body should fail the screen, not the
/// process).
pub(crate) fn markdown_to_styled_lines(input: &str) -> Vec<StyledLine> {
    use ratatui_core::style::{Color, Modifier};

    let text = std::panic::catch_unwind(|| tui_markdown::from_str(input))
        .map_err(|_| "rendering error")
        .unwrap_or_default();

    text.lines
        .into_iter()
        .map(|line| {
            let spans = line
                .spans
                .into_iter()
                .map(|span| {
                    let modifier = span.style.add_modifier;
                    StyledSpan {
                        text: span.content.into_owned(),
                        fg: span.style.fg.and_then(map_color),
                        bg: span.style.bg.and_then(map_color),
                        modifiers: TextMods {
                            bold: modifier.contains(Modifier::BOLD),
                            italic: modifier.contains(Modifier::ITALIC),
                            underline: modifier.contains(Modifier::UNDERLINED),
                            reverse: modifier.contains(Modifier::REVERSED),
                            dim: modifier.contains(Modifier::DIM),
                        },
                    }
                })
                .collect();
            StyledLine { spans }
        })
        .collect()
}

/// Map a `ratatui_core::style::Color` to the plugin crate's
/// [`otto_plugin::ThemeColor`]. Reset → `None` so the runtime
/// inherits from the active theme.
fn map_color(c: ratatui_core::style::Color) -> Option<otto_plugin::ThemeColor> {
    use ratatui_core::style::Color;
    use otto_plugin::ThemeColor;
    Some(match c {
        Color::Reset => return None,
        Color::Black => ThemeColor::Black,
        Color::Red => ThemeColor::Red,
        Color::Green => ThemeColor::Green,
        Color::Yellow => ThemeColor::Yellow,
        Color::Blue => ThemeColor::Blue,
        Color::Magenta => ThemeColor::Magenta,
        Color::Cyan => ThemeColor::Cyan,
        Color::Gray => ThemeColor::Gray,
        Color::DarkGray => ThemeColor::DarkGray,
        Color::LightRed => ThemeColor::LightRed,
        Color::LightGreen => ThemeColor::LightGreen,
        Color::LightYellow => ThemeColor::LightYellow,
        Color::LightBlue => ThemeColor::LightBlue,
        Color::LightMagenta => ThemeColor::LightMagenta,
        Color::LightCyan => ThemeColor::LightCyan,
        Color::White => ThemeColor::White,
        Color::Indexed(i) => ThemeColor::Indexed(i),
        Color::Rgb(r, g, b) => ThemeColor::Rgb { r, g, b },
    })
}
```

No additional `use` lines at the top of the file: `markdown_to_styled_lines` brings the `ratatui_core::style` items into scope inside the function body, and `map_color` does the same for its narrow ratatui surface. This keeps the file-wide import set focused on the plugin types.

- [ ] **Step 3: Skip running tests this task — Task 8 wires the submodule into mod.rs and Task 9 runs the suite.**

- [ ] **Step 4: Commit**

```bash
git add crates/otto/src/plugin/builtin/changelog/screen.rs
git commit -m "feat(changelog): markdown → StyledLine adapter (#68)

Pure function markdown_to_styled_lines() drives tui-markdown and maps
each ratatui Span (style fg/bg/modifier) to StyledSpan. catch_unwind
guards against panics inside the markdown crate so a corrupt body
fails the screen rather than the process. map_color covers every
ratatui_core::style::Color variant (Reset → None for theme
inheritance)."
```

---

## Task 8: `changelog/mod.rs` — `ChangelogPlugin` shell

**Files:**
- Create: `crates/otto/src/plugin/builtin/changelog/mod.rs`

- [ ] **Step 1: Create the module file with the plugin shell + tests**

Create `crates/otto/src/plugin/builtin/changelog/mod.rs` with:

```rust
//! `internal:changelog` plugin: streams CHANGELOG.md from
//! raw.githubusercontent.com and renders it via tui-markdown in a
//! dedicated screen. Closes #68.
//!
//! On `/changelog`, [`ChangelogPlugin::handle_slash`] returns
//! `Effect::OpenScreen { id: "changelog", args: ScreenArgs::Changelog }`.
//! The runtime then calls [`ChangelogPlugin::create_screen`], which
//! constructs a [`screen::ChangelogScreen`] in [`screen::ChangelogState::Loading`]
//! and spawns a tokio task that calls
//! [`fetch::ChangelogFetcher::fetch`] and writes the result into the
//! screen's shared state.
//!
//! The fetcher is trait-injected so unit tests can substitute a stub.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use otto_plugin::{
    Contributions, Effect, Manifest, Plugin, PluginError, PluginId, PluginKind, Screen,
    ScreenArgs, ScreenSpec, SlashSpec,
};

pub mod fetch;
pub mod screen;

pub use fetch::{ChangelogFetcher, GithubChangelogFetcher};
pub use screen::{ChangelogScreen, ChangelogState};

const SCREEN_ID: &str = "changelog";

/// The plugin instance held by the runtime.
pub struct ChangelogPlugin {
    /// Fetcher used by every newly-opened screen instance. Defaults to
    /// [`GithubChangelogFetcher`]; tests substitute a stub.
    fetcher: Arc<dyn ChangelogFetcher>,
}

impl ChangelogPlugin {
    pub fn new() -> Self {
        Self::with_fetcher(Arc::new(GithubChangelogFetcher))
    }

    pub fn with_fetcher(fetcher: Arc<dyn ChangelogFetcher>) -> Self {
        Self { fetcher }
    }
}

impl Default for ChangelogPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for ChangelogPlugin {
    fn manifest(&self) -> Manifest {
        let mut contributions = Contributions::default();
        contributions.slash_commands = vec![SlashSpec {
            name: "changelog".into(),
            summary: rust_i18n::t!("changelog.slash-summary").to_string(),
            args_hint: None,
            requires_arg: false,
        }];
        contributions.screens = vec![ScreenSpec {
            id: SCREEN_ID.into(),
        }];

        Manifest {
            id: PluginId::new("internal:changelog").expect("valid built-in id"),
            name: "Changelog".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: rust_i18n::t!("plugin.changelog-description").to_string(),
            kind: PluginKind::Optional,
            contributions,
        }
    }

    async fn handle_slash(
        &mut self,
        name: &str,
        _args: Vec<String>,
    ) -> Result<Vec<Effect>, PluginError> {
        if name != "changelog" {
            return Ok(vec![]);
        }
        Ok(vec![Effect::OpenScreen {
            id: SCREEN_ID.into(),
            args: ScreenArgs::Changelog,
        }])
    }

    fn create_screen(&self, id: &str, args: ScreenArgs) -> Result<Box<dyn Screen>, PluginError> {
        match (id, args) {
            (SCREEN_ID, ScreenArgs::Changelog) => {
                let state = Arc::new(Mutex::new(ChangelogState::Loading));
                let screen = ChangelogScreen::new(Arc::clone(&state));
                spawn_fetch_task(Arc::clone(&self.fetcher), state);
                Ok(Box::new(screen))
            }
            (SCREEN_ID, other) => Err(PluginError::InvalidArgs(format!(
                "/changelog takes no args; got {other:?}"
            ))),
            (other, _) => Err(PluginError::ScreenNotFound(other.to_string())),
        }
    }
}

/// Spawn a task that calls `fetcher.fetch()` and publishes the result
/// into the supplied shared state cell. Free function so it's directly
/// testable and so the retry path (later) can call it without holding
/// `&self`.
fn spawn_fetch_task(
    fetcher: Arc<dyn ChangelogFetcher>,
    state: Arc<Mutex<ChangelogState>>,
) {
    tokio::spawn(async move {
        let new_state = match fetcher.fetch().await {
            Ok(markdown) => ChangelogState::Loaded {
                lines: screen::markdown_to_styled_lines(&markdown),
            },
            Err(e) => ChangelogState::Failed {
                error: e.to_string(),
            },
        };
        if let Ok(mut guard) = state.lock() {
            *guard = new_state;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::HOME_LOCK;

    /// In-test fetcher: returns canned markdown or a canned error.
    struct StubFetcher {
        result: Mutex<Result<String, String>>,
    }

    impl StubFetcher {
        fn ok(md: &str) -> Self {
            Self {
                result: Mutex::new(Ok(md.into())),
            }
        }
        fn err(msg: &str) -> Self {
            Self {
                result: Mutex::new(Err(msg.into())),
            }
        }
    }

    #[async_trait]
    impl ChangelogFetcher for StubFetcher {
        async fn fetch(&self) -> anyhow::Result<String> {
            match &*self.result.lock().unwrap() {
                Ok(s) => Ok(s.clone()),
                Err(m) => Err(anyhow::anyhow!(m.clone())),
            }
        }
    }

    #[test]
    fn manifest_contributes_slash_and_screen() {
        let _lock = HOME_LOCK.lock().unwrap();
        rust_i18n::set_locale("en");

        let p = ChangelogPlugin::new();
        let m = p.manifest();
        assert_eq!(m.id.as_str(), "internal:changelog");
        assert_eq!(m.contributions.slash_commands.len(), 1);
        assert_eq!(m.contributions.slash_commands[0].name, "changelog");
        assert_eq!(m.contributions.screens.len(), 1);
        assert_eq!(m.contributions.screens[0].id, SCREEN_ID);
        assert_eq!(m.kind, PluginKind::Optional);
    }

    #[tokio::test]
    async fn slash_changelog_emits_open_screen() {
        let mut p = ChangelogPlugin::with_fetcher(Arc::new(StubFetcher::ok("# x")));
        let effects = p.handle_slash("changelog", vec![]).await.unwrap();
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            Effect::OpenScreen { id, args } => {
                assert_eq!(id, SCREEN_ID);
                assert!(matches!(args, ScreenArgs::Changelog));
            }
            other => panic!("expected OpenScreen, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn slash_ignores_other_commands() {
        let mut p = ChangelogPlugin::with_fetcher(Arc::new(StubFetcher::ok("# x")));
        let effects = p.handle_slash("not-changelog", vec![]).await.unwrap();
        assert!(effects.is_empty());
    }

    #[tokio::test]
    async fn create_screen_with_correct_id_and_args_returns_screen() {
        let p = ChangelogPlugin::with_fetcher(Arc::new(StubFetcher::ok("# x")));
        let screen = p.create_screen(SCREEN_ID, ScreenArgs::Changelog).unwrap();
        assert_eq!(screen.id(), SCREEN_ID);
    }

    #[tokio::test]
    async fn create_screen_with_unknown_id_returns_screen_not_found() {
        let p = ChangelogPlugin::with_fetcher(Arc::new(StubFetcher::ok("# x")));
        let err = p
            .create_screen("not-changelog", ScreenArgs::Changelog)
            .unwrap_err();
        assert!(matches!(err, PluginError::ScreenNotFound(_)));
    }

    #[tokio::test]
    async fn create_screen_with_wrong_args_returns_invalid_args() {
        let p = ChangelogPlugin::with_fetcher(Arc::new(StubFetcher::ok("# x")));
        let err = p.create_screen(SCREEN_ID, ScreenArgs::None).unwrap_err();
        assert!(matches!(err, PluginError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn spawn_fetch_task_writes_loaded_on_success() {
        let state = Arc::new(Mutex::new(ChangelogState::Loading));
        spawn_fetch_task(
            Arc::new(StubFetcher::ok("# Heading\n\nbody")),
            Arc::clone(&state),
        );
        for _ in 0..200 {
            tokio::task::yield_now().await;
            if matches!(*state.lock().unwrap(), ChangelogState::Loaded { .. }) {
                return;
            }
        }
        panic!("state never transitioned to Loaded");
    }

    #[tokio::test]
    async fn spawn_fetch_task_writes_failed_on_error() {
        let state = Arc::new(Mutex::new(ChangelogState::Loading));
        spawn_fetch_task(
            Arc::new(StubFetcher::err("DNS error")),
            Arc::clone(&state),
        );
        for _ in 0..200 {
            tokio::task::yield_now().await;
            if let ChangelogState::Failed { error } = &*state.lock().unwrap() {
                assert!(error.contains("DNS error"), "got: {error}");
                return;
            }
        }
        panic!("state never transitioned to Failed");
    }
}
```

- [ ] **Step 2: Wire the new submodule into the plugin tree**

Edit `crates/otto/src/plugin/builtin/mod.rs`. Find the existing `pub mod self_update;` line (around line 61) and add directly above it:

```rust
/// `internal:changelog` plugin: streams CHANGELOG.md and renders it
/// via tui-markdown in a dedicated `/changelog` screen. Closes #68.
pub mod changelog;
```

- [ ] **Step 3: Verify the new module compiles**

Run: `rustup run stable cargo check -p otto 2>&1 | tail -10`
Expected: `Finished `dev` profile …` (no errors). If you see `error[E0432]: unresolved import …` for `ScreenSpec`, look it up in `crates/otto-plugin/src/manifest.rs` to confirm the type name and fix the `use` line accordingly.

- [ ] **Step 4: Commit**

```bash
git add crates/otto/src/plugin/builtin/changelog/mod.rs crates/otto/src/plugin/builtin/mod.rs
git commit -m "feat(changelog): ChangelogPlugin shell + module wiring (#68)

Manifest contributes /changelog slash + the \"changelog\" screen.
handle_slash emits Effect::OpenScreen; create_screen constructs the
screen and spawns the fetch task. PluginKind::Optional so users can
disable via /plugins. Tests cover the manifest, slash dispatch,
create_screen happy path + both error paths, and the spawn-fetch task
on both Ok and Err."
```

---

## Task 9: Locale strings — add `[changelog]` section to all four locales

**Files:**
- Modify: `crates/otto/locales/en.toml`
- Modify: `crates/otto/locales/es.toml`
- Modify: `crates/otto/locales/hi.toml`
- Modify: `crates/otto/locales/pt.toml`

- [ ] **Step 1: Add the `[changelog]` block to `en.toml`**

Locate the `[self-update]` section in `crates/otto/locales/en.toml` (around line 121). Immediately after the closing of that section (the blank line before `[picker.themes]`), insert:

```toml
[changelog]
slash-summary  = "Show release notes from CHANGELOG.md"
loading        = "Fetching changelog…"
fetch-failed   = "Couldn't fetch changelog: %{err}"
retry-hint     = "Press r to retry, Esc to close."
tips           = "j/k scroll · g/G top/bottom · r retry · Esc close"
```

Then locate the `[plugin]` section (search for `self-update-description`) and add directly below it:

```toml
changelog-description     = "View the project's CHANGELOG.md release notes"
```

- [ ] **Step 2: Mirror in `es.toml`**

Same surgery in `crates/otto/locales/es.toml` with these strings (insertion points: after `[self-update]` block, and below `self-update-description` in `[plugin]`):

```toml
[changelog]
slash-summary  = "Mostrar las notas de versión de CHANGELOG.md"
loading        = "Obteniendo el changelog…"
fetch-failed   = "No se pudo obtener el changelog: %{err}"
retry-hint     = "Pulsa r para reintentar, Esc para cerrar."
tips           = "j/k desplazar · g/G inicio/fin · r reintentar · Esc cerrar"
```

```toml
changelog-description     = "Ver las notas de versión de CHANGELOG.md"
```

- [ ] **Step 3: Mirror in `hi.toml`**

```toml
[changelog]
slash-summary  = "CHANGELOG.md से रिलीज़ नोट्स दिखाएँ"
loading        = "changelog ला रहा है…"
fetch-failed   = "changelog नहीं ला सका: %{err}"
retry-hint     = "पुनः प्रयास के लिए r दबाएँ, Esc से बंद करें।"
tips           = "j/k स्क्रॉल · g/G ऊपर/नीचे · r पुनः प्रयास · Esc बंद"
```

```toml
changelog-description     = "प्रोजेक्ट के CHANGELOG.md रिलीज़ नोट्स देखें"
```

- [ ] **Step 4: Mirror in `pt.toml`**

```toml
[changelog]
slash-summary  = "Mostrar as notas de versão do CHANGELOG.md"
loading        = "Obtendo o changelog…"
fetch-failed   = "Não foi possível obter o changelog: %{err}"
retry-hint     = "Pressione r para tentar novamente, Esc para fechar."
tips           = "j/k rolar · g/G topo/fim · r tentar novamente · Esc fechar"
```

```toml
changelog-description     = "Ver as notas de versão do CHANGELOG.md do projeto"
```

- [ ] **Step 5: Run the locales test (catches missing-key drift across files)**

Run: `rustup run stable cargo test -p otto --test locales 2>&1 | tail -10`
Expected: all green. (If a non-English locale is missing a key the test fails with the offending key name; add it.)

- [ ] **Step 6: Commit**

```bash
git add crates/otto/locales/en.toml crates/otto/locales/es.toml crates/otto/locales/hi.toml crates/otto/locales/pt.toml
git commit -m "i18n: add [changelog] section + plugin description (#68)

New keys: slash-summary, loading, fetch-failed, retry-hint, tips, plus
plugin.changelog-description. en is canonical; es/hi/pt translated."
```

---

## Task 10: Register `ChangelogPlugin` in the runtime

**Files:**
- Modify: `crates/otto/src/plugin/mod.rs`

- [ ] **Step 1: Add the registration**

In `crates/otto/src/plugin/mod.rs`, locate the `plugins` vec inside `builtin_set()` (around line 80–98). Insert in alphabetical order (between `builtin::clear::ClearPlugin` and `builtin::command_palette::…`):

```rust
        Box::new(builtin::changelog::ChangelogPlugin::new()),
```

- [ ] **Step 2: Run the full self_update + changelog test surface**

Run: `rustup run stable cargo test -p otto changelog 2>&1 | tail -20`
Expected: all changelog tests pass (counted in the `test result: ok.` line).

- [ ] **Step 3: Run the full crate test suite**

Run: `rustup run stable cargo test --workspace 2>&1 | grep -E '^test result:|FAILED' | tail -40`
Expected: every line says `ok.`. No `FAILED` lines.

- [ ] **Step 4: Commit**

```bash
git add crates/otto/src/plugin/mod.rs
git commit -m "feat(changelog): register ChangelogPlugin in builtin set (#68)

Wires the new plugin into the runtime so /changelog actually fires.
Plugin is Optional, so users can disable via /plugins like any other
non-core feature."
```

---

## Task 11: Final verification + push + open PR

**Files:** none modified directly; this task only runs verification commands and pushes.

- [ ] **Step 1: Run formatter under stable**

Run: `rustup run stable cargo fmt --all 2>&1 | tail -5`
Expected: no output, exit 0. If it modifies anything, run `git status` to see the changes and commit:

```bash
git add -u
git commit -m "fmt: cargo fmt --all (#68)"
```

- [ ] **Step 2: Verify fmt clean**

Run: `rustup run stable cargo fmt --all -- --check 2>&1 | tail -3`
Expected: no output, exit 0.

- [ ] **Step 3: Run clippy with workspace + all-targets + warnings as errors**

Run: `rustup run stable cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -20`
Expected: `Finished `dev` profile …` with no error lines. If clippy flags an issue, fix it inline (typical hits: `redundant_clone` on `Arc::clone`, `needless_borrow`).

- [ ] **Step 4: Re-run the full workspace test suite**

Run: `rustup run stable cargo test --workspace 2>&1 | grep -E '^test result:|FAILED' | tail -40`
Expected: every line says `ok.`. No `FAILED`.

- [ ] **Step 5: Inspect the diff once more before push**

Run: `git status && git log --oneline master..HEAD`
Expected: ~10 commits ahead of master, 8 modified files plus new `crates/otto/src/plugin/builtin/changelog/{mod,fetch,screen}.rs` and `docs/superpowers/specs/2026-05-14-issue-68-changelog-viewer-design.md` and `docs/superpowers/plans/2026-05-14-issue-68-changelog-viewer.md`.

- [ ] **Step 6: Push the branch**

Run:
```bash
git push -u origin feat/issue-68-changelog-viewer
```

- [ ] **Step 7: Open the PR**

Run (replace `<commit-summary>` with a 1–2 sentence rundown):

```bash
gh pr create --title "feat(changelog): /changelog slash + viewer screen (#68)" --body "$(cat <<'EOF'
## Summary

- New built-in plugin `internal:changelog` exposes a `/changelog` slash command that opens a dedicated screen.
- Screen streams `https://raw.githubusercontent.com/robhicks/otto-rs/master/CHANGELOG.md` and renders it via `tui-markdown`, translating the output into the plugin crate's `StyledLine` vocabulary.
- Scroll/retry/close keybindings: `j/k`/`↑/↓` line, `PageUp/PageDown`, `g/G`, `r` (in `Failed`), `Esc/q`.
- New `ScreenArgs::Changelog` variant; locale strings added in en/es/hi/pt.
- No auto-open after self-update — pure on-demand. Per-session in-memory cache.

Closes #68. Ships in the same release as #67 (auto-install all release binaries).

## Test plan

- [x] `rustup run stable cargo fmt --all -- --check`
- [x] `rustup run stable cargo clippy --workspace --all-targets -- -D warnings`
- [x] `rustup run stable cargo test --workspace`
- [ ] Manual: `cargo run -p otto`, type `/changelog`, confirm fetch + render + scroll + Esc close. (Run after merge.)
- [ ] Manual: simulate offline (`OTTO_NO_UPDATE_CHECK=1` and disconnect), `/changelog`, confirm `Failed` state shows error and `r` retries. (Run after merge.)
EOF
)"
```

- [ ] **Step 8: Verify CI for the pushed SHA**

Run:
```bash
gh pr checks $(gh pr view --json number -q .number) 2>&1 | tail -10
```

If runs are queued or in-flight: report that to the user with the run IDs and stop. Do NOT poll. Per project memory, never claim "push is good" without `gh run` confirming green for the pushed SHA.

If runs have completed and pass: report the PR URL + green checks.
If a run failed: read the failure with `gh run view <run-id> --log-failed | tail -50` and either fix inline (then push again) or report to the user.

---

## Out of scope (do NOT add to this PR)

- Workspace version bump + CHANGELOG entry — that's a follow-up PR per the user's direction (handled by the release-coupling PR alongside #67).
- Auto-open the changelog viewer after `/update` lands.
- "What's new since you last looked" persisted marker / filtering.
- Bundled offline copy of CHANGELOG.md (`include_str!`).
- Tag pinning of the source URL.
- Find-in-page / search.

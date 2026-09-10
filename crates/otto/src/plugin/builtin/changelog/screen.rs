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
    Effect, KeyCodePortable, KeyEventPortable, PluginError, Region, Screen, StyledLine, StyledSpan,
    TextMods,
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

/// Number of lines a `PageUp` / `PageDown` press advances the scroll
/// offset by. Chosen to feel snappy on the typical 80×24 terminal
/// without skipping an entire CHANGELOG section.
pub(crate) const PAGE_SIZE: usize = 10;

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

#[async_trait]
impl Screen for ChangelogScreen {
    fn id(&self) -> String {
        "changelog".to_string()
    }

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
                if visible == 0 || lines.is_empty() {
                    return vec![];
                }
                // Clamp the offset so a "scroll past the end" press (G,
                // PageDown overshoot) leaves the last `visible` lines
                // filling the viewport, matching vim/less convention.
                let max_offset = lines.len().saturating_sub(visible);
                let effective_offset = self.scroll_offset.min(max_offset);
                let end = (effective_offset + visible).min(lines.len());
                lines[effective_offset..end].to_vec()
            }
        }
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

/// Translate a markdown string into the plugin crate's
/// [`StyledLine`] vocabulary. Uses [`tui_markdown::from_str`] to do
/// the markdown parsing and styled-text emission, then converts each
/// ratatui `Line` / `Span` into the equivalent `StyledLine` /
/// `StyledSpan`. Pure function; defensive against panics inside the
/// markdown crate (a corrupt body should fail the screen, not the
/// process).
///
/// Note: `tui_markdown` links against ratatui 0.29 while this crate
/// links against ratatui 0.30 (via ratatui_core). The Color types
/// from the two versions are not the same Rust type, so fg/bg colors
/// are extracted via their `Display` representation and re-mapped
/// without naming either crate's Color enum directly. Modifiers are
/// mapped via their underlying `u16` bit pattern (BOLD=0x0001,
/// DIM=0x0002, ITALIC=0x0004, UNDERLINED=0x0008, REVERSED=0x0040),
/// which is identical across both versions.
pub(crate) fn markdown_to_styled_lines(input: &str) -> Vec<StyledLine> {
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
                    // Modifier is a u16 bitflags type with identical bit
                    // layout in ratatui 0.29 and ratatui_core (0.30).
                    let modifier_bits = span.style.add_modifier.bits();
                    StyledSpan {
                        text: span.content.into_owned(),
                        // Colors are extracted via Display to avoid a
                        // cross-version type mismatch (ratatui 0.29 vs
                        // ratatui_core). map_color_str is pure and safe.
                        fg: span.style.fg.and_then(|c| map_color_str(&c.to_string())),
                        bg: span.style.bg.and_then(|c| map_color_str(&c.to_string())),
                        modifiers: TextMods {
                            bold: modifier_bits & 0x0001 != 0,
                            dim: modifier_bits & 0x0002 != 0,
                            italic: modifier_bits & 0x0004 != 0,
                            underline: modifier_bits & 0x0008 != 0,
                            reverse: modifier_bits & 0x0040 != 0,
                        },
                    }
                })
                .collect();
            StyledLine { spans }
        })
        .collect()
}

/// Map a ratatui `Color` Display string to the plugin crate's
/// [`otto_plugin::ThemeColor`]. `Reset` maps to `None` so the
/// runtime inherits from the active theme. This avoids naming the
/// ratatui Color enum type (which differs between the ratatui 0.29
/// used by tui-markdown and the ratatui_core used by this crate).
fn map_color_str(s: &str) -> Option<otto_plugin::ThemeColor> {
    use otto_plugin::ThemeColor;
    match s {
        "Reset" => None,
        "Black" => Some(ThemeColor::Black),
        "Red" => Some(ThemeColor::Red),
        "Green" => Some(ThemeColor::Green),
        "Yellow" => Some(ThemeColor::Yellow),
        "Blue" => Some(ThemeColor::Blue),
        "Magenta" => Some(ThemeColor::Magenta),
        "Cyan" => Some(ThemeColor::Cyan),
        "Gray" => Some(ThemeColor::Gray),
        "DarkGray" => Some(ThemeColor::DarkGray),
        "LightRed" => Some(ThemeColor::LightRed),
        "LightGreen" => Some(ThemeColor::LightGreen),
        "LightYellow" => Some(ThemeColor::LightYellow),
        "LightBlue" => Some(ThemeColor::LightBlue),
        "LightMagenta" => Some(ThemeColor::LightMagenta),
        "LightCyan" => Some(ThemeColor::LightCyan),
        "White" => Some(ThemeColor::White),
        s => {
            // Try Indexed: ratatui 0.29 Display for Indexed(n) is just "n".
            if let Ok(idx) = s.parse::<u8>() {
                return Some(ThemeColor::Indexed(idx));
            }
            // Try RGB: ratatui 0.29 Display for Rgb(r,g,b) is "#RRGGBB".
            if let Some(hex) = s.strip_prefix('#') {
                if hex.len() == 6 {
                    if let (Ok(r), Ok(g), Ok(b)) = (
                        u8::from_str_radix(&hex[0..2], 16),
                        u8::from_str_radix(&hex[2..4], 16),
                        u8::from_str_radix(&hex[4..6], 16),
                    ) {
                        return Some(ThemeColor::Rgb { r, g, b });
                    }
                }
            }
            // Unknown variant — inherit from theme.
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded_state(n_lines: usize) -> ChangelogState {
        ChangelogState::Loaded {
            lines: (0..n_lines)
                .map(|i| StyledLine::plain(format!("line {i}")))
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

    fn key(c: KeyCodePortable) -> KeyEventPortable {
        KeyEventPortable {
            code: c,
            modifiers: otto_plugin::KeyMods::default(),
        }
    }

    fn screen_with(state: ChangelogState) -> ChangelogScreen {
        ChangelogScreen::new(Arc::new(Mutex::new(state)))
    }

    #[tokio::test]
    async fn esc_emits_close_screen() {
        let mut s = screen_with(ChangelogState::Loading);
        let effs = s.on_key(key(KeyCodePortable::Esc)).await.unwrap();
        assert!(matches!(effs[0], Effect::CloseScreen));
    }

    #[tokio::test]
    async fn q_emits_close_screen() {
        let mut s = screen_with(ChangelogState::Loading);
        let effs = s.on_key(key(KeyCodePortable::Char('q'))).await.unwrap();
        assert!(matches!(effs[0], Effect::CloseScreen));
    }

    #[tokio::test]
    async fn down_and_j_increment_scroll_offset() {
        let mut s = screen_with(loaded_state(50));
        s.on_key(key(KeyCodePortable::Down)).await.unwrap();
        assert_eq!(s.scroll_offset, 1);
        s.on_key(key(KeyCodePortable::Char('j'))).await.unwrap();
        assert_eq!(s.scroll_offset, 2);
    }

    #[tokio::test]
    async fn up_and_k_decrement_scroll_offset_with_clamp_at_zero() {
        let mut s = screen_with(loaded_state(50));
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
        let mut s = screen_with(loaded_state(200));
        s.on_key(key(KeyCodePortable::PageDown)).await.unwrap();
        assert_eq!(s.scroll_offset, super::PAGE_SIZE);
    }

    #[tokio::test]
    async fn page_up_retreats_by_page_size_with_clamp() {
        let mut s = screen_with(loaded_state(200));
        s.scroll_offset = 5;
        s.on_key(key(KeyCodePortable::PageUp)).await.unwrap();
        assert_eq!(s.scroll_offset, 0);
    }

    #[tokio::test]
    async fn lower_g_jumps_to_top_and_upper_g_jumps_to_bottom() {
        let mut s = screen_with(loaded_state(50));
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
        });
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
        let mut s = screen_with(loaded_state(10));
        s.scroll_offset = 3;
        let effs = s.on_key(key(KeyCodePortable::Char('r'))).await.unwrap();
        assert!(effs.is_empty());
        // No state change.
        assert!(matches!(
            *s.state.lock().unwrap(),
            ChangelogState::Loaded { .. }
        ));
        assert_eq!(s.scroll_offset, 3);
    }

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
    fn render_loaded_clamps_scroll_offset_to_show_last_page_when_past_end() {
        // scroll_offset bigger than (len - visible) must leave the last
        // visible lines filling the viewport — vim/less convention. The
        // assertion guards against the previous behavior (empty Vec).
        let s = {
            let mut sc = ChangelogScreen::new(Arc::new(Mutex::new(loaded_state(20))));
            sc.scroll_offset = 99;
            sc
        };
        let lines = s.render(region(80, 5));
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0].spans[0].text, "line 15");
        assert_eq!(lines[4].spans[0].text, "line 19");
    }

    #[tokio::test]
    async fn upper_g_then_render_shows_last_visible_page() {
        let mut s = screen_with(loaded_state(20));
        s.on_key(key(KeyCodePortable::Char('G'))).await.unwrap();
        let lines = s.render(region(80, 5));
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0].spans[0].text, "line 15");
        assert_eq!(lines[4].spans[0].text, "line 19");
    }

    #[test]
    fn markdown_to_styled_lines_renders_heading_and_body() {
        let lines = super::markdown_to_styled_lines("# Hello\n\nbody");
        let combined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.text.clone()))
            .collect::<Vec<_>>()
            .join("");
        assert!(combined.contains("Hello"), "got: {combined}");
        assert!(combined.contains("body"), "got: {combined}");

        // The heading and body must appear on separate lines.
        let heading_line = lines
            .iter()
            .find(|l| l.spans.iter().any(|s| s.text.contains("Hello")))
            .expect("must find a line containing the heading text");
        let body_line = lines
            .iter()
            .find(|l| l.spans.iter().any(|s| s.text.contains("body")))
            .expect("must find a line containing the body text");
        assert!(
            !std::ptr::eq(heading_line as *const _, body_line as *const _),
            "heading and body must be on separate lines"
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
}

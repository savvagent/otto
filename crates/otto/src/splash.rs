//! Startup splash. Painted as a full-frame overlay until any key dismisses it,
//! or [`SPLASH_DURATION`] elapses — whichever comes first.
//!
//! # Truthfulness (v0.8)
//!
//! The splash banner reports four mutually exclusive sandbox states:
//!
//! | [`SandboxSplashState`]   | Color  | Nag line shown |
//! |--------------------------|--------|----------------|
//! | `OnDefault`              | green  | yes — "use `/sandbox off` to disable" |
//! | `OnExplicit`             | green  | no             |
//! | `OffExplicit`            | yellow | no             |
//! | `ParseError`             | red    | error detail   |
//!
//! The state is computed once at `App::new` from
//! [`SandboxConfig::load_with_status`] and refreshed when a host materializes
//! (via `App::refresh_splash_sandbox_from_host`) so the banner reflects the
//! config the host will actually apply, not whatever the on-disk file said
//! when we first read it. The splash never re-reads disk per frame.

use std::time::Duration;

use otto_host::{SandboxConfig, SandboxLoadStatus, SandboxMode};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

/// How long the splash lingers before auto-dismissing.
pub const SPLASH_DURATION: Duration = Duration::from_secs(3);

const LOGO: &[&str] = &[
    " ██████╗   ████████╗  ████████╗   ██████╗  ",
    "██╔═══██╗  ╚══██╔══╝  ╚══██╔══╝  ██╔═══██╗ ",
    "██║   ██║     ██║        ██║     ██║   ██║ ",
    "██║   ██║     ██║        ██║     ██║   ██║ ",
    "╚██████╔╝     ██║        ██║     ╚██████╔╝ ",
    " ╚═════╝      ╚═╝        ╚═╝      ╚═════╝  ",
];

const TAGLINE: &str = "the savvy MCP-native terminal coding agent";
const HINT: &str = "press any key to continue";

const LOGO_WIDTH: u16 = 43;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SplashLineKind {
    Blank,
    Logo,
    Tagline,
    SandboxOn,
    SandboxOff,
    SandboxError,
    Hint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SplashContentLine {
    pub text: String,
    pub kind: SplashLineKind,
    pub centered: bool,
}

/// What the splash shows for the sandbox indicator. Derived once at startup
/// (and refreshed on `/connect`) so the splash never re-reads disk per frame.
#[derive(Debug, Clone)]
pub enum SandboxSplashState {
    /// Sandbox enabled, but the user has not declared an explicit preference.
    /// Shows the green "on" line *with* the nag pointing at `/sandbox off`.
    OnDefault,
    /// Sandbox explicitly enabled. Shows the green "on" line, no nag.
    OnExplicit,
    /// Sandbox explicitly disabled. Shows the yellow "off" line, no nag.
    OffExplicit,
    /// On-disk config failed to load. Shows a red line with the failure
    /// reason so the user knows their preferred state is *not* applied
    /// and they should fix `~/.otto/sandbox.toml`.
    ParseError {
        /// Short human-readable detail (parser error or version mismatch).
        detail: String,
    },
}

impl SandboxSplashState {
    /// Build from a config + status pair as returned by
    /// [`SandboxConfig::load_with_status`].
    pub fn from_load(cfg: &SandboxConfig, status: &SandboxLoadStatus) -> Self {
        match status {
            SandboxLoadStatus::ParseError { message } => Self::ParseError {
                detail: format!("parse error: {message}"),
            },
            SandboxLoadStatus::UnsupportedVersion { found, max } => Self::ParseError {
                detail: format!(
                    "sandbox.toml declares schema version {found}, this build supports up to {max}"
                ),
            },
            SandboxLoadStatus::NoFile | SandboxLoadStatus::Loaded => match cfg.mode {
                SandboxMode::Default => Self::OnDefault,
                SandboxMode::On => Self::OnExplicit,
                SandboxMode::Off => Self::OffExplicit,
            },
        }
    }

    /// Build from an already-active host's [`SandboxConfig`]. A host that
    /// successfully constructed by definition has a parsed config in hand,
    /// so status is implicitly `Loaded`.
    pub fn from_host_config(cfg: &SandboxConfig) -> Self {
        Self::from_load(cfg, &SandboxLoadStatus::Loaded)
    }
}

pub fn render(frame: &mut Frame, area: Rect, sandbox: &SandboxSplashState) {
    frame.render_widget(Clear, area);
    let lines: Vec<Line<'static>> = shared_content(sandbox)
        .into_iter()
        .map(|content| {
            let line = Line::from(Span::styled(content.text, ratatui_style(content.kind)));
            if content.centered {
                line.centered()
            } else {
                line
            }
        })
        .collect();

    let total_h = lines.len() as u16;
    let rect = center_rect(LOGO_WIDTH, total_h, area);
    frame.render_widget(Paragraph::new(lines), rect);
}

/// Shared splash content for startup rendering and slash-screen consumers.
pub(crate) fn shared_content(state: &SandboxSplashState) -> Vec<SplashContentLine> {
    let (sandbox_text, sandbox_kind) = sandbox_line(state);
    let mut lines: Vec<SplashContentLine> = LOGO
        .iter()
        .map(|row| SplashContentLine {
            text: (*row).to_string(),
            kind: SplashLineKind::Logo,
            centered: true,
        })
        .collect();
    lines.push(SplashContentLine {
        text: String::new(),
        kind: SplashLineKind::Blank,
        centered: false,
    });
    lines.push(SplashContentLine {
        text: TAGLINE.to_string(),
        kind: SplashLineKind::Tagline,
        centered: true,
    });
    lines.push(SplashContentLine {
        text: String::new(),
        kind: SplashLineKind::Blank,
        centered: false,
    });
    lines.push(SplashContentLine {
        text: sandbox_text,
        kind: sandbox_kind,
        centered: true,
    });
    lines.push(SplashContentLine {
        text: String::new(),
        kind: SplashLineKind::Blank,
        centered: false,
    });
    lines.push(SplashContentLine {
        text: format!("{HINT} · v{}", env!("CARGO_PKG_VERSION")),
        kind: SplashLineKind::Hint,
        centered: true,
    });
    lines
}

/// Map a [`SandboxSplashState`] to the shared sandbox line's text + semantic
/// style so both renderers can stay in sync without duplicating content logic.
fn sandbox_line(state: &SandboxSplashState) -> (String, SplashLineKind) {
    match state {
        SandboxSplashState::OnDefault => (
            "sandbox: on (use /sandbox off to disable)".to_string(),
            SplashLineKind::SandboxOn,
        ),
        SandboxSplashState::OnExplicit => ("sandbox: on".to_string(), SplashLineKind::SandboxOn),
        SandboxSplashState::OffExplicit => ("sandbox: off".to_string(), SplashLineKind::SandboxOff),
        SandboxSplashState::ParseError { detail } => (
            format!("sandbox: defaults — {detail}"),
            SplashLineKind::SandboxError,
        ),
    }
}

fn ratatui_style(kind: SplashLineKind) -> Style {
    match kind {
        SplashLineKind::Blank => Style::default(),
        SplashLineKind::Logo => Style::default()
            .fg(Color::LightBlue)
            .add_modifier(Modifier::BOLD),
        SplashLineKind::Tagline => Style::default().fg(Color::LightBlue),
        SplashLineKind::SandboxOn => Style::default().fg(Color::Green),
        SplashLineKind::SandboxOff => Style::default().fg(Color::Yellow),
        SplashLineKind::SandboxError => {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        }
        SplashLineKind::Hint => Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    }
}

fn center_rect(w: u16, h: u16, area: Rect) -> Rect {
    let width = w.min(area.width);
    let height = h.min(area.height);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with_mode(mode: SandboxMode) -> SandboxConfig {
        SandboxConfig {
            mode,
            ..SandboxConfig::default()
        }
    }

    #[test]
    fn from_load_classifies_loaded_default_as_on_with_nag() {
        let cfg = cfg_with_mode(SandboxMode::Default);
        let state = SandboxSplashState::from_load(&cfg, &SandboxLoadStatus::Loaded);
        assert!(matches!(state, SandboxSplashState::OnDefault));
        let (text, _) = sandbox_line(&state);
        assert!(
            text.contains("on") && text.contains("/sandbox off"),
            "Default mode must show the v0.7 nag pointing to `/sandbox off`: {text}"
        );
    }

    #[test]
    fn from_load_classifies_no_file_as_default_on() {
        let cfg = SandboxConfig::default();
        let state = SandboxSplashState::from_load(&cfg, &SandboxLoadStatus::NoFile);
        assert!(
            matches!(state, SandboxSplashState::OnDefault),
            "absent file is morally equivalent to the implicit Default — the user \
             hasn't said anything yet, so the nag still applies"
        );
    }

    #[test]
    fn from_load_classifies_explicit_on_as_on_no_nag() {
        let cfg = cfg_with_mode(SandboxMode::On);
        let state = SandboxSplashState::from_load(&cfg, &SandboxLoadStatus::Loaded);
        assert!(matches!(state, SandboxSplashState::OnExplicit));
        let (text, _) = sandbox_line(&state);
        assert!(
            !text.contains("/sandbox"),
            "explicit On must not show the v0.7 nag (no-nag promise): {text}"
        );
    }

    #[test]
    fn from_load_classifies_explicit_off_as_off_no_nag() {
        let cfg = cfg_with_mode(SandboxMode::Off);
        let state = SandboxSplashState::from_load(&cfg, &SandboxLoadStatus::Loaded);
        assert!(matches!(state, SandboxSplashState::OffExplicit));
        let (text, _) = sandbox_line(&state);
        assert!(
            !text.contains("/sandbox"),
            "explicit Off must not show the v0.7 nag (no-nag promise): {text}"
        );
    }

    #[test]
    fn from_load_classifies_parse_error_as_red_with_detail() {
        let cfg = cfg_with_mode(SandboxMode::Off); // fail-safe in load_from_path
        let state = SandboxSplashState::from_load(
            &cfg,
            &SandboxLoadStatus::ParseError {
                message: "TOML parse error at line 1".to_string(),
            },
        );
        match &state {
            SandboxSplashState::ParseError { detail } => {
                assert!(
                    detail.contains("TOML parse error"),
                    "ParseError detail must carry the parser message: {detail}"
                );
            }
            other => panic!("expected ParseError, got {other:?}"),
        }
        let (_text, kind) = sandbox_line(&state);
        assert_eq!(
            ratatui_style(kind).fg,
            Some(Color::Red),
            "parse-error state must be rendered red"
        );
    }

    #[test]
    fn from_load_classifies_unsupported_version_as_red_with_detail() {
        let cfg = cfg_with_mode(SandboxMode::Off);
        let state = SandboxSplashState::from_load(
            &cfg,
            &SandboxLoadStatus::UnsupportedVersion { found: 99, max: 1 },
        );
        match &state {
            SandboxSplashState::ParseError { detail } => {
                assert!(
                    detail.contains("99") && detail.contains("1"),
                    "version-mismatch detail must mention both versions: {detail}"
                );
            }
            other => panic!("expected ParseError, got {other:?}"),
        }
    }

    #[test]
    fn parse_error_overrides_otherwise_friendly_mode() {
        // Even if the loader managed to recover a mode, ParseError status
        // means the on-disk preference is *not* faithfully applied — the
        // splash must say so, not pretend everything is fine.
        let cfg = cfg_with_mode(SandboxMode::On);
        let state = SandboxSplashState::from_load(
            &cfg,
            &SandboxLoadStatus::ParseError {
                message: "x".to_string(),
            },
        );
        assert!(matches!(state, SandboxSplashState::ParseError { .. }));
    }

    #[test]
    fn from_host_config_treats_status_as_loaded() {
        // A live host always has a parsed config, so the host-based
        // classifier never produces ParseError.
        let cfg = cfg_with_mode(SandboxMode::Off);
        let state = SandboxSplashState::from_host_config(&cfg);
        assert!(matches!(state, SandboxSplashState::OffExplicit));
    }

    #[test]
    fn logo_rows_match_logo_width() {
        for row in LOGO {
            assert_eq!(row.chars().count(), usize::from(LOGO_WIDTH));
        }
    }

    #[test]
    fn shared_content_includes_inline_hint_and_sandbox_line() {
        let lines = shared_content(&SandboxSplashState::OnDefault);

        assert!(
            lines.iter().any(
                |line| line.text == "sandbox: on (use /sandbox off to disable)"
                    && line.kind == SplashLineKind::SandboxOn
                    && line.centered
            ),
            "shared splash content must include the centered sandbox line"
        );
        assert!(
            lines.iter().any(|line| line.text
                == format!("press any key to continue · v{}", env!("CARGO_PKG_VERSION"))
                && line.kind == SplashLineKind::Hint
                && line.centered),
            "shared splash content must include the centered versioned hint"
        );
    }

    #[test]
    fn shared_content_marks_logo_rows_centered_for_startup_and_splash_parity() {
        let lines = shared_content(&SandboxSplashState::OnDefault);
        let logo_rows = lines
            .iter()
            .filter(|line| line.kind == SplashLineKind::Logo)
            .collect::<Vec<_>>();

        assert!(
            !logo_rows.is_empty(),
            "shared splash content should include logo rows"
        );
        assert!(
            logo_rows.iter().all(|line| line.centered),
            "shared splash logo rows must stay centered for startup and /splash parity"
        );
    }
}

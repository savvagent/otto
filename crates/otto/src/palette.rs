//! Render-path color palette derived from the active [`Theme`].
//!
//! The TUI does not restructure widgets when the theme changes — it only
//! swaps the [`Color`] values used by [`Style`]s on existing widgets.
//! [`Palette::for_theme`] returns the slot-to-color map for one of the
//! three built-in themes; render-path code reads slots (`fg`, `accent`,
//! `error`, …) instead of hard-coded colors.
//!
//! Slots are deliberately minimal: only the ones the UI actually paints
//! today. Add new slots as new widgets need them; default to the closest
//! existing slot when extending.

use crate::plugin::builtin::themes::catalog::Theme;
use ratatui::style::{Color, Style};
use ratatui_themes::ThemePalette;

/// Render-path color palette derived from an active [`Theme`].
///
/// Each field is one semantic "slot" the UI paints with. Values are
/// concrete [`Color`]s so widgets can build [`Style`]s with no extra
/// indirection.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Default text color.
    pub fg: Color,
    /// Default background color.
    pub bg: Color,
    /// Border/chrome color for blocks and dividers.
    pub border: Color,
    /// Accent color for highlights, active rows, header chrome.
    pub accent: Color,
    /// Muted/dim color for notes, metadata, hints.
    pub muted: Color,
    /// Error / denial color.
    pub error: Color,
    /// Success / "on" color.
    pub success: Color,
    /// Warning / "pending" color.
    pub warning: Color,
    /// Secondary accent (e.g. user vs. assistant differentiation).
    pub secondary: Color,
}

impl Palette {
    /// Resolve the palette for a [`Theme`].
    ///
    /// Built-in themes (Dark / Light / HighContrast) are hand-tuned
    /// to keep the v0.7 color story intact. Upstream themes are mapped
    /// from `ratatui_themes::ThemePalette` via [`Palette::from_upstream`].
    #[must_use]
    pub fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Dark => Self {
                fg: Color::White,
                bg: Color::Reset,
                border: Color::DarkGray,
                accent: Color::Blue,
                muted: Color::DarkGray,
                error: Color::Red,
                success: Color::Green,
                warning: Color::Yellow,
                secondary: Color::Cyan,
            },
            Theme::Light => Self {
                fg: Color::Black,
                bg: Color::White,
                border: Color::Gray,
                accent: Color::Blue,
                muted: Color::DarkGray,
                error: Color::Red,
                success: Color::Green,
                warning: Color::Yellow,
                secondary: Color::Magenta,
            },
            Theme::HighContrast => Self {
                fg: Color::White,
                bg: Color::Black,
                border: Color::White,
                accent: Color::Yellow,
                muted: Color::Gray,
                error: Color::LightRed,
                success: Color::LightGreen,
                warning: Color::LightYellow,
                secondary: Color::LightCyan,
            },
            Theme::Upstream(name) => Self::from_upstream(name.palette()),
        }
    }

    /// Map a `ratatui_themes::ThemePalette` into our slot layout.
    ///
    /// Slot correspondences:
    ///
    /// | Our slot   | Upstream field | Notes                                  |
    /// |------------|----------------|----------------------------------------|
    /// | `fg`       | `fg`           | direct                                 |
    /// | `bg`       | `bg`           | direct                                 |
    /// | `accent`   | `accent`       | direct                                 |
    /// | `muted`    | `muted`        | blended toward `fg` for legibility     |
    /// | `error`    | `error`        | direct                                 |
    /// | `warning`  | `warning`      | direct                                 |
    /// | `success`  | `success`      | direct                                 |
    /// | `secondary`| `secondary`    | direct                                 |
    /// | `border`   | `selection`    | upstream's selection bg reads as chrome|
    ///
    /// Upstream's `info` slot has no direct counterpart in our layout
    /// (we don't render "info"-flavored chrome). It is intentionally
    /// dropped; new slots can map to it if a need appears.
    ///
    /// `muted` is intentionally not the raw upstream value: many themes
    /// (Solarized Light, Catppuccin Latte, Tokyo Night Day, …) set
    /// `muted` to a "comment" color that is barely legible against the
    /// theme background. The UI uses `muted` for command descriptions,
    /// tips, footer chrome, and conversation notes — content the user
    /// needs to read — so we blend `muted` halfway toward `fg` to raise
    /// contrast while keeping a clear visual distinction from primary
    /// text.
    #[must_use]
    pub fn from_upstream(p: ThemePalette) -> Self {
        Self {
            fg: p.fg,
            bg: p.bg,
            border: p.selection,
            accent: p.accent,
            muted: blend(p.muted, p.fg, 0.5),
            error: p.error,
            success: p.success,
            warning: p.warning,
            secondary: p.secondary,
        }
    }

    /// Base `fg`-on-`bg` style for the frame's default background.
    #[must_use]
    pub fn base_style(self) -> Style {
        Style::default().fg(self.fg).bg(self.bg)
    }
}

/// Linear interpolation between two `Color`s. `t == 0.0` returns `a`,
/// `t == 1.0` returns `b`. Only RGB inputs blend; anything else (named
/// colors, indexed palette, `Color::Reset`) falls through to `a` because
/// we don't have a defined channel space for them.
///
/// Used to lift `muted` toward `fg` so upstream themes whose `muted` is
/// a low-contrast comment color stay readable; see
/// [`Palette::from_upstream`].
fn blend(a: Color, b: Color, t: f32) -> Color {
    let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) = (a, b) else {
        return a;
    };
    let mix = |x: u8, y: u8| {
        let v = (1.0 - t) * f32::from(x) + t * f32::from(y);
        v.round().clamp(0.0, 255.0) as u8
    };
    Color::Rgb(mix(ar, br), mix(ag, bg), mix(ab, bb))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_themes::ThemeName;

    #[test]
    fn for_theme_returns_distinct_palettes() {
        let dark = Palette::for_theme(Theme::Dark);
        let light = Palette::for_theme(Theme::Light);
        let hc = Palette::for_theme(Theme::HighContrast);
        // Backgrounds differ across all three themes.
        assert_ne!(dark.bg, light.bg);
        assert_ne!(dark.bg, hc.bg);
        assert_ne!(light.bg, hc.bg);
    }

    #[test]
    fn base_style_uses_palette_fg_and_bg() {
        let p = Palette::for_theme(Theme::Light);
        let s = p.base_style();
        assert_eq!(s.fg, Some(Color::Black));
        assert_eq!(s.bg, Some(Color::White));
    }

    #[test]
    fn for_theme_handles_every_upstream_theme() {
        for upstream in ThemeName::all() {
            let p = Palette::for_theme(Theme::Upstream(*upstream));
            // The minimum invariant: fg != bg. If a future upstream
            // theme regresses to fg == bg the whole palette is unusable.
            assert_ne!(
                p.fg,
                p.bg,
                "{}'s palette must have a legible fg/bg pair",
                upstream.slug()
            );
        }
    }

    #[test]
    fn upstream_themes_differ_from_each_other_at_the_bg() {
        // Two different upstream themes must produce different
        // backgrounds — otherwise from_upstream is collapsing them.
        let mut seen = std::collections::HashSet::new();
        for upstream in ThemeName::all() {
            let bg = Palette::for_theme(Theme::Upstream(*upstream)).bg;
            seen.insert(format!("{bg:?}"));
        }
        assert!(
            seen.len() >= 10,
            "expected the 15-theme catalog to yield at least 10 distinct \
             backgrounds, got {}: {seen:?}",
            seen.len()
        );
    }

    #[test]
    fn from_upstream_threads_every_slot() {
        // Smoke-test the field plumbing using Dracula's known values.
        let p = Palette::from_upstream(ThemeName::Dracula.palette());
        // Dracula's bg is RGB(40, 42, 54).
        assert_eq!(p.bg, Color::Rgb(40, 42, 54));
        // Dracula's accent is purple, RGB(189, 147, 249).
        assert_eq!(p.accent, Color::Rgb(189, 147, 249));
        // border comes from upstream's `selection`, which Dracula sets
        // to RGB(68, 71, 90).
        assert_eq!(p.border, Color::Rgb(68, 71, 90));
    }
}

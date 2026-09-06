//! Theme selection and persistence for the TUI.
//!
//! Themes are runtime-selectable palettes applied at the render-path
//! boundary; switching themes does not restructure the widget tree.
//! Selection is loaded from `~/.savvagent/config.toml`'s `[theme]`
//! section at startup and persisted on every successful `/theme <name>`
//! invocation. The render path itself stays in `app.rs` / `ui.rs`; this
//! module owns only the type, serialization, and disk I/O.
//!
//! # Catalog
//!
//! Three hand-rolled built-ins ([`Theme::Dark`], [`Theme::Light`],
//! [`Theme::HighContrast`]) plus the [`ratatui_themes`] upstream catalog
//! (Dracula, Nord, Gruvbox, Solarized, Tokyo Night, Catppuccin, …) are
//! exposed as a single [`Theme`] enum. The upstream catalog grows
//! whenever the dependency is updated — call sites that iterate via
//! [`Theme::all`] or [`Theme::catalog`] pick up new themes automatically.
//!
//! Contract: unknown theme names never crash. [`Theme::from_name`]
//! returns `None`; callers fall back to the active theme and emit a
//! warning line.

use std::path::Path;
use std::str::FromStr;

use crate::config_file::{ConfigFile, ThemeSection};
use ratatui_themes::ThemeName;
use savvagent_plugin::{ThemeColor, ThemeEntry, ThemePalette};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// One of the TUI's selectable themes.
///
/// The three built-ins ([`Theme::Dark`], [`Theme::Light`],
/// [`Theme::HighContrast`]) are hand-rolled and exist before
/// `ratatui-themes` integration. [`Theme::Upstream`] wraps the entire
/// upstream catalog (Dracula, Nord, …); the wire format uses each
/// theme's slug (e.g. `tokyo-night`, `catppuccin-mocha`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Default dark theme. Hand-rolled palette.
    #[default]
    Dark,
    /// Hand-rolled light palette.
    Light,
    /// Hand-rolled high-contrast palette intended for accessibility.
    HighContrast,
    /// Any theme from the `ratatui-themes` upstream catalog (Dracula,
    /// Nord, Gruvbox, Tokyo Night, etc.). The palette mapping lives in
    /// [`crate::palette::Palette::for_theme`].
    Upstream(ThemeName),
}

impl Theme {
    /// Stable wire name used in `[theme].name` and `/theme <name>`. For
    /// upstream themes this is the slug (`dracula`, `tokyo-night`, …)
    /// from `ratatui_themes::ThemeName::slug`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Light => "light",
            Theme::HighContrast => "high-contrast",
            Theme::Upstream(t) => t.slug(),
        }
    }

    /// `true` for the three hand-rolled built-ins; `false` for any
    /// theme from the upstream catalog. Used by the `/theme list`
    /// renderer to group output.
    #[must_use]
    pub fn is_builtin(self) -> bool {
        !matches!(self, Theme::Upstream(_))
    }

    /// Parse a slug from `config.toml` or `/theme <slug>`. Case-sensitive
    /// to match the wire format. Returns `None` for unknown inputs —
    /// the caller decides how to surface the failure.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "dark" => Some(Theme::Dark),
            "light" => Some(Theme::Light),
            "high-contrast" => Some(Theme::HighContrast),
            // Match upstream slugs case-sensitively. `ThemeName::from_str`
            // is forgiving (lowercase + accepts display names), but the
            // wire format we persist is strictly the kebab-case slug, so
            // we round-trip through it explicitly.
            other => ThemeName::all()
                .iter()
                .find(|t| t.slug() == other)
                .copied()
                .map(Theme::Upstream),
        }
    }

    /// Every selectable theme in display order: built-ins first, then
    /// the upstream catalog. Length grows whenever `ratatui-themes`
    /// adds a theme.
    #[must_use]
    pub fn all() -> Vec<Theme> {
        let mut out = vec![Theme::Dark, Theme::Light, Theme::HighContrast];
        out.extend(ThemeName::all().iter().copied().map(Theme::Upstream));
        out
    }

    /// Only the upstream catalog (no built-ins). Useful for renderers
    /// that want to group "Built-in" vs "Catalog" sections.
    // Currently unused in the binary; the test below pins the
    // catalog/builtin partition invariant. Retained as part of the
    // public Theme API for future iteration helpers (e.g., settings UI).
    #[allow(dead_code)]
    pub fn catalog() -> impl Iterator<Item = Theme> {
        ThemeName::all().iter().copied().map(Theme::Upstream)
    }

    /// Human-friendly label shown in the picker UI. Built-ins use the
    /// canonical capitalised forms; upstream themes delegate to
    /// [`ThemeName::display_name`] (e.g. `"Tokyo Night"`,
    /// `"Catppuccin Mocha"`).
    #[must_use]
    pub fn display_name(self) -> &'static str {
        match self {
            Theme::Dark => "Dark",
            Theme::Light => "Light",
            Theme::HighContrast => "High Contrast",
            Theme::Upstream(t) => t.display_name(),
        }
    }

    /// Adapt this theme into the WIT-portable [`ThemeEntry`] returned by
    /// `Plugin::themes` / `Contributions::themes`. The palette is a
    /// coarse approximation (the rich render-path palette lives in
    /// [`crate::palette`]) — enough for picker UIs that only need
    /// representative colors without dragging `ratatui::style::Color`
    /// across the plugin boundary.
    #[must_use]
    pub fn to_theme_entry(self) -> ThemeEntry {
        let dark = !matches!(self, Theme::Light);
        let palette = match self {
            Theme::Dark => ThemePalette::new(
                ThemeColor::Black,
                ThemeColor::White,
                ThemeColor::Blue,
                ThemeColor::Gray,
            ),
            Theme::Light => ThemePalette::new(
                ThemeColor::White,
                ThemeColor::Black,
                ThemeColor::Blue,
                ThemeColor::Gray,
            ),
            Theme::HighContrast => ThemePalette::new(
                ThemeColor::Black,
                ThemeColor::White,
                ThemeColor::Yellow,
                ThemeColor::Gray,
            ),
            Theme::Upstream(_) => ThemePalette::new(
                ThemeColor::Black,
                ThemeColor::White,
                ThemeColor::Cyan,
                ThemeColor::Gray,
            ),
        };
        ThemeEntry {
            slug: self.name().to_string(),
            label: self.display_name().to_string(),
            dark,
            palette,
        }
    }
}

// --- Serde adapter ---------------------------------------------------
//
// The built-in [`derive`] form doesn't compose cleanly with the
// `Upstream(ThemeName)` variant: we'd need an untagged enum that tried
// each variant in turn, which is brittle for forward-compat. Instead we
// serialize each theme as its slug — a flat string — and deserialize
// via [`Theme::from_name`]. Unknown slugs become a loud
// `serde::de::Error` rather than a silent default.

impl Serialize for Theme {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for Theme {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        Theme::from_name(&s).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "unknown theme `{s}` — run `/theme list` to see available themes"
            ))
        })
    }
}

impl FromStr for Theme {
    type Err = UnknownTheme;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Theme::from_name(s).ok_or_else(|| UnknownTheme(s.to_string()))
    }
}

/// Returned by [`Theme::from_str`] when the slug isn't recognised. The
/// caller decides how to surface it (the TUI's `/theme` handler renders
/// a "not found — keeping current" note instead of bubbling the error).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownTheme(pub String);

impl std::fmt::Display for UnknownTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown theme `{}`", self.0)
    }
}

impl std::error::Error for UnknownTheme {}

/// Load the user's theme selection. Returns [`Theme::default()`] if the
/// file is absent or unparseable.
///
/// Parse errors are logged at `warn!` level. Missing-file is silent
/// (expected on first run).
pub fn load() -> Theme {
    load_from_path(&ConfigFile::default_path())
}

/// Load the theme selection from an explicit path. Pure inner used by
/// [`load`] and tests.
pub(crate) fn load_from_path(path: &Path) -> Theme {
    Theme::from_name(&ConfigFile::load_or_default(path).config.theme.name).unwrap_or_default()
}

/// Persist the selected theme.
pub fn save(theme: Theme) -> std::io::Result<()> {
    save_to_path(&ConfigFile::default_path(), theme)
}

pub(crate) fn save_to_path(path: &Path, theme: Theme) -> std::io::Result<()> {
    ConfigFile::save_theme_section(
        path,
        ThemeSection {
            name: theme.name().to_string(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // --- Built-ins ---

    #[test]
    fn from_name_recognises_built_ins() {
        assert_eq!(Theme::from_name("dark"), Some(Theme::Dark));
        assert_eq!(Theme::from_name("light"), Some(Theme::Light));
        assert_eq!(Theme::from_name("high-contrast"), Some(Theme::HighContrast));
    }

    #[test]
    fn from_name_rejects_unknown() {
        assert_eq!(Theme::from_name(""), None);
        assert_eq!(Theme::from_name("DARK"), None);
        assert_eq!(Theme::from_name("HIGH-CONTRAST"), None);
        assert_eq!(Theme::from_name("not-a-real-theme"), None);
    }

    #[test]
    fn all_returns_built_ins_plus_catalog() {
        let themes = Theme::all();
        // 3 built-ins + 15 upstream = 18 (will grow as ratatui-themes does).
        assert_eq!(themes.len(), 3 + ThemeName::all().len());
        // Built-ins come first.
        assert_eq!(themes[0], Theme::Dark);
        assert_eq!(themes[1], Theme::Light);
        assert_eq!(themes[2], Theme::HighContrast);
        // Catalog follows in upstream order.
        for (i, upstream) in ThemeName::all().iter().enumerate() {
            assert_eq!(themes[3 + i], Theme::Upstream(*upstream));
        }
    }

    #[test]
    fn name_round_trips_through_from_name_for_every_theme() {
        for t in Theme::all() {
            assert_eq!(
                Theme::from_name(t.name()),
                Some(t),
                "slug round-trip failed for {t:?}"
            );
        }
    }

    #[test]
    fn is_builtin_partitions_correctly() {
        assert!(Theme::Dark.is_builtin());
        assert!(Theme::Light.is_builtin());
        assert!(Theme::HighContrast.is_builtin());
        for upstream in ThemeName::all() {
            assert!(
                !Theme::Upstream(*upstream).is_builtin(),
                "{} should not be builtin",
                upstream.slug()
            );
        }
    }

    // --- Upstream catalog ---

    #[test]
    fn from_name_recognises_every_upstream_slug() {
        for upstream in ThemeName::all() {
            let theme = Theme::from_name(upstream.slug());
            assert_eq!(
                theme,
                Some(Theme::Upstream(*upstream)),
                "upstream slug `{}` did not round-trip",
                upstream.slug()
            );
        }
    }

    #[test]
    fn catalog_iterator_excludes_built_ins() {
        for t in Theme::catalog() {
            assert!(
                !t.is_builtin(),
                "Theme::catalog() must not yield a built-in: {t:?}"
            );
        }
        assert_eq!(Theme::catalog().count(), ThemeName::all().len());
    }

    #[test]
    fn from_str_returns_unknown_theme_error() {
        let err = "bogus".parse::<Theme>().unwrap_err();
        assert_eq!(err.0, "bogus");
        assert!(err.to_string().contains("bogus"));
    }

    // --- Serde adapter ---

    #[test]
    fn theme_section_round_trips_built_ins() {
        for t in [Theme::Dark, Theme::Light, Theme::HighContrast] {
            let section = ThemeSection {
                name: t.name().to_string(),
            };
            let text = toml::to_string(&section).unwrap();
            let parsed: ThemeSection = toml::from_str(&text).unwrap();
            assert_eq!(parsed.name, t.name());
        }
    }

    #[test]
    fn theme_section_round_trips_every_upstream_theme() {
        for upstream in ThemeName::all() {
            let theme = Theme::Upstream(*upstream);
            let section = ThemeSection {
                name: theme.name().to_string(),
            };
            let text = toml::to_string(&section).unwrap();
            assert!(
                text.contains(upstream.slug()),
                "serialized form must contain the slug `{}`: {text}",
                upstream.slug()
            );
            let parsed: ThemeSection = toml::from_str(&text).unwrap();
            assert_eq!(parsed.name, theme.name());
        }
    }

    #[test]
    fn theme_section_missing_field_defaults_to_dark() {
        let parsed: ThemeSection = toml::from_str("").unwrap();
        assert_eq!(parsed.name, Theme::Dark.name());
    }

    #[test]
    fn theme_section_with_unknown_name_is_a_loud_parse_error() {
        // Unknown slug → serde::de::Error::custom (NOT a silent default).
        let r: Result<ThemeSection, _> = toml::from_str(r#"name = "totally-bogus""#);
        let err = r.unwrap_err().to_string();
        assert!(
            err.contains("totally-bogus"),
            "parse error must surface the offending slug: {err}"
        );
    }

    // --- load_from_path + save_to_path ---

    #[test]
    fn load_from_path_returns_default_when_file_absent() {
        let td = TempDir::new().unwrap();
        let missing = td.path().join("config.toml");
        assert_eq!(load_from_path(&missing), Theme::Dark);
    }

    #[test]
    fn load_from_path_returns_default_on_parse_error() {
        let td = TempDir::new().unwrap();
        let path = td.path().join("config.toml");
        std::fs::write(&path, "[theme]\nname = \"totally-bogus\"\n").unwrap();
        assert_eq!(
            load_from_path(&path),
            Theme::Dark,
            "parse failure must fall back to default, not crash"
        );
    }

    #[test]
    fn save_then_load_round_trips_built_in() {
        let td = TempDir::new().unwrap();
        let path = td.path().join("nested").join("config.toml");
        save_to_path(&path, Theme::HighContrast).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[theme]"), "saved config: {text}");
        assert!(
            text.contains(r#"name = "high-contrast""#),
            "saved config: {text}"
        );
        assert!(
            !path.with_file_name("theme.toml").exists(),
            "save should stop creating standalone theme.toml"
        );
        assert_eq!(load_from_path(&path), Theme::HighContrast);
    }

    #[test]
    fn save_then_load_round_trips_upstream_theme() {
        let td = TempDir::new().unwrap();
        let path = td.path().join("config.toml");
        let theme = Theme::Upstream(ThemeName::TokyoNight);
        save_to_path(&path, theme).unwrap();
        assert_eq!(load_from_path(&path), theme);
    }

    #[test]
    fn save_overwrites_previous_value() {
        let td = TempDir::new().unwrap();
        let path = td.path().join("config.toml");
        save_to_path(&path, Theme::Light).unwrap();
        save_to_path(&path, Theme::Upstream(ThemeName::Nord)).unwrap();
        assert_eq!(load_from_path(&path), Theme::Upstream(ThemeName::Nord));
    }
}

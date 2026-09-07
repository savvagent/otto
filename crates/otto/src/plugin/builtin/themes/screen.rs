//! Wraps the lifted [`ThemePicker`] state machine as a `Screen` plugin.
//!
//! The picker itself takes a `crossterm::event::KeyEvent`; this Screen
//! adapter converts the WIT-portable `KeyEventPortable` into a crossterm
//! event before delegating, then maps the resulting `PickerOutcome` to
//! the closed effect vocabulary. The pre-open snapshot lives on the
//! [`ThemePicker`] itself so Cancel/Esc reverts deterministically.

use async_trait::async_trait;
use otto_plugin::{
    Effect, KeyCodePortable, KeyEventPortable, PluginError, Region, Screen, StyledLine, StyledSpan,
    TextMods, ThemeColor,
};

use super::picker::{PickerOutcome, ThemePicker};

/// Per-open instance of the theme picker modal.
pub struct ThemePickerScreen {
    inner: ThemePicker,
}

impl ThemePickerScreen {
    /// Construct from a pre-built [`ThemePicker`]. The picker captures
    /// the pre-open theme in its own state, so Esc/Cancel can revert
    /// without the runtime tracking that snapshot separately.
    pub fn new(inner: ThemePicker) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Screen for ThemePickerScreen {
    fn id(&self) -> String {
        "themes.picker".to_string()
    }

    fn render(&self, _region: Region) -> Vec<StyledLine> {
        let mut out: Vec<StyledLine> = Vec::new();
        out.push(StyledLine::plain(
            rust_i18n::t!(
                "picker.themes.filter-label",
                filter = self.inner.filter.clone()
            )
            .to_string(),
        ));
        out.push(StyledLine::plain(""));

        let filtered = self.inner.filtered_themes();
        if filtered.is_empty() {
            out.push(StyledLine {
                spans: vec![StyledSpan {
                    text: rust_i18n::t!(
                        "picker.themes.no-match",
                        filter = self.inner.filter.clone()
                    )
                    .to_string(),
                    fg: Some(ThemeColor::Warning),
                    bg: None,
                    modifiers: TextMods::default(),
                }],
            });
            return out;
        }

        // Group built-ins first, then upstream catalog. Picker cursor is
        // an index into the flat filtered list, so we keep that mapping
        // by walking with the original index.
        let builtins: Vec<(usize, _)> = filtered
            .iter()
            .enumerate()
            .filter(|(_, t)| t.is_builtin())
            .map(|(i, t)| (i, *t))
            .collect();
        let catalog: Vec<(usize, _)> = filtered
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.is_builtin())
            .map(|(i, t)| (i, *t))
            .collect();

        if !builtins.is_empty() {
            out.push(StyledLine {
                spans: vec![StyledSpan {
                    text: rust_i18n::t!("picker.themes.section-builtin").to_string(),
                    fg: Some(ThemeColor::Muted),
                    bg: None,
                    modifiers: TextMods::default(),
                }],
            });
            for (i, t) in &builtins {
                out.push(self.row(*i, *t));
            }
        }
        if !catalog.is_empty() {
            out.push(StyledLine {
                spans: vec![StyledSpan {
                    text: rust_i18n::t!("picker.themes.section-catalog").to_string(),
                    fg: Some(ThemeColor::Muted),
                    bg: None,
                    modifiers: TextMods::default(),
                }],
            });
            for (i, t) in &catalog {
                out.push(self.row(*i, *t));
            }
        }
        out
    }

    async fn on_key(&mut self, key: KeyEventPortable) -> Result<Vec<Effect>, PluginError> {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        // Translate portable -> crossterm. The picker rejects modifier-bearing
        // chars on its own (Ctrl/Alt + char becomes a no-op), so we collapse
        // unknown codes to a no-op return here rather than synthesising one.
        let ct_code = match key.code {
            KeyCodePortable::Char(c) => KeyCode::Char(c),
            KeyCodePortable::Enter => KeyCode::Enter,
            KeyCodePortable::Esc => KeyCode::Esc,
            KeyCodePortable::Up => KeyCode::Up,
            KeyCodePortable::Down => KeyCode::Down,
            KeyCodePortable::Backspace => KeyCode::Backspace,
            _ => return Ok(vec![]),
        };
        let mut mods = KeyModifiers::empty();
        if key.modifiers.ctrl {
            mods |= KeyModifiers::CONTROL;
        }
        if key.modifiers.alt {
            mods |= KeyModifiers::ALT;
        }
        if key.modifiers.shift {
            mods |= KeyModifiers::SHIFT;
        }
        let ct_event = KeyEvent::new(ct_code, mods);
        let outcome = self.inner.on_key(ct_event);

        match outcome {
            PickerOutcome::Stay => Ok(vec![]),
            PickerOutcome::PreviewTheme(t) => Ok(vec![Effect::SetActiveTheme {
                slug: t.name().to_string(),
                persist: false,
            }]),
            PickerOutcome::Apply(t) => Ok(vec![Effect::Stack(vec![
                Effect::SetActiveTheme {
                    slug: t.name().to_string(),
                    persist: true,
                },
                Effect::CloseScreen,
            ])]),
            PickerOutcome::Cancel => Ok(vec![Effect::Stack(vec![
                Effect::SetActiveTheme {
                    slug: self.inner.pre_open_theme.name().to_string(),
                    persist: false,
                },
                Effect::CloseScreen,
            ])]),
        }
    }

    fn tips(&self) -> Vec<StyledLine> {
        vec![StyledLine::plain(
            rust_i18n::t!("picker.themes.tips").to_string(),
        )]
    }
}

impl ThemePickerScreen {
    fn row(&self, filtered_idx: usize, theme: super::catalog::Theme) -> StyledLine {
        let is_cursor = filtered_idx == self.inner.cursor;
        let is_active = theme == self.inner.pre_open_theme;
        let prefix = if is_cursor { "    > " } else { "      " };
        let active_marker = if is_active { "  (active)" } else { "" };
        StyledLine {
            spans: vec![
                StyledSpan {
                    text: format!("{prefix}{:<24}", theme.name()),
                    fg: Some(if is_cursor {
                        ThemeColor::Accent
                    } else {
                        ThemeColor::Fg
                    }),
                    bg: None,
                    modifiers: TextMods {
                        bold: is_cursor,
                        ..Default::default()
                    },
                },
                StyledSpan {
                    text: format!("{}{active_marker}", theme.display_name()),
                    fg: Some(ThemeColor::Muted),
                    bg: None,
                    modifiers: TextMods::default(),
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::builtin::themes::catalog::Theme;
    use otto_plugin::KeyMods;

    fn key(c: KeyCodePortable) -> KeyEventPortable {
        KeyEventPortable {
            code: c,
            modifiers: KeyMods::default(),
        }
    }

    #[tokio::test]
    async fn apply_emits_stack_settheme_persist_then_close() {
        let mut s = ThemePickerScreen::new(ThemePicker::new(Theme::default()));
        let effs = s.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        match &effs[0] {
            Effect::Stack(children) => {
                assert!(matches!(
                    children[0],
                    Effect::SetActiveTheme { persist: true, .. }
                ));
                assert!(matches!(children[1], Effect::CloseScreen));
            }
            other => panic!("expected Stack, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn cancel_reverts_to_pre_open() {
        let pre = Theme::default();
        let mut s = ThemePickerScreen::new(ThemePicker::new(pre));
        // Move cursor first so the preview drifts away from the pre-open
        // theme; then Esc should still restore `pre`.
        let _ = s.on_key(key(KeyCodePortable::Down)).await.unwrap();
        let effs = s.on_key(key(KeyCodePortable::Esc)).await.unwrap();
        match &effs[0] {
            Effect::Stack(children) => match &children[0] {
                Effect::SetActiveTheme {
                    slug,
                    persist: false,
                } => assert_eq!(slug, pre.name()),
                other => panic!("unexpected first child: {other:?}"),
            },
            other => panic!("expected Stack, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn down_emits_preview_set_theme_persist_false() {
        let mut s = ThemePickerScreen::new(ThemePicker::new(Theme::default()));
        let effs = s.on_key(key(KeyCodePortable::Down)).await.unwrap();
        assert!(matches!(
            effs[0],
            Effect::SetActiveTheme { persist: false, .. }
        ));
    }

    #[tokio::test]
    async fn typed_char_filters() {
        let mut s = ThemePickerScreen::new(ThemePicker::new(Theme::default()));
        let _ = s.on_key(key(KeyCodePortable::Char('d'))).await.unwrap();
        let _ = s.on_key(key(KeyCodePortable::Char('r'))).await.unwrap();
        let _ = s.on_key(key(KeyCodePortable::Char('a'))).await.unwrap();
        // After typing "dra", the filter should restrict to themes
        // containing that substring (Dracula).
        let filtered = s.inner.filtered_themes();
        assert!(
            filtered.iter().all(|t| t.name().contains("dra")),
            "all filtered themes must contain `dra`: {filtered:?}"
        );
    }

    #[test]
    fn id_is_themes_picker() {
        let s = ThemePickerScreen::new(ThemePicker::new(Theme::default()));
        assert_eq!(s.id(), "themes.picker");
    }
}

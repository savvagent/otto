//! Wraps `LanguagePicker` as a `Screen`. Mirrors
//! `internal:themes::screen::ThemePickerScreen`.

use async_trait::async_trait;
use otto_plugin::{
    Effect, KeyCodePortable, KeyEventPortable, PluginError, Region, Screen, StyledLine, StyledSpan,
    TextMods, ThemeColor,
};

use super::catalog::lookup;
use super::picker::{LanguagePicker, PickerOutcome};

pub struct LanguagePickerScreen {
    inner: LanguagePicker,
}

impl LanguagePickerScreen {
    pub fn new(inner: LanguagePicker) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Screen for LanguagePickerScreen {
    fn id(&self) -> String {
        "language.picker".to_string()
    }

    fn render(&self, _region: Region) -> Vec<StyledLine> {
        let mut out: Vec<StyledLine> = Vec::new();
        out.push(StyledLine::plain(
            rust_i18n::t!(
                "picker.language.filter-label",
                filter = self.inner.filter.clone()
            )
            .to_string(),
        ));
        out.push(StyledLine::plain(""));

        let filtered = self.inner.filtered();
        if filtered.is_empty() {
            out.push(StyledLine {
                spans: vec![StyledSpan {
                    text: rust_i18n::t!(
                        "picker.language.no-match",
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

        for (idx, l) in filtered.iter().enumerate() {
            out.push(self.row(idx, l));
        }
        out
    }

    async fn on_key(&mut self, key: KeyEventPortable) -> Result<Vec<Effect>, PluginError> {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
            PickerOutcome::PreviewLocale(c) => Ok(vec![Effect::SetActiveLocale {
                code: c.to_string(),
                persist: false,
            }]),
            PickerOutcome::Apply(c) => Ok(vec![Effect::Stack(vec![
                Effect::SetActiveLocale {
                    code: c.to_string(),
                    persist: true,
                },
                Effect::CloseScreen,
            ])]),
            PickerOutcome::Cancel => Ok(vec![Effect::Stack(vec![
                Effect::SetActiveLocale {
                    code: self.inner.pre_open_code.to_string(),
                    persist: false,
                },
                Effect::CloseScreen,
            ])]),
        }
    }

    fn tips(&self) -> Vec<StyledLine> {
        vec![StyledLine::plain(
            rust_i18n::t!("picker.language.tips").to_string(),
        )]
    }
}

impl LanguagePickerScreen {
    fn row(&self, filtered_idx: usize, lang: &super::catalog::Language) -> StyledLine {
        let is_cursor = filtered_idx == self.inner.cursor;
        let is_active = lang.code == self.inner.pre_open_code;
        let prefix = if is_cursor { "    > " } else { "      " };
        let active_marker = if is_active { "  (active)" } else { "" };
        let native = lookup(lang.code)
            .map(|l| l.native_name)
            .unwrap_or(lang.code);
        StyledLine {
            spans: vec![
                StyledSpan {
                    text: format!("{prefix}{:<6}", lang.code),
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
                    text: format!("{native}{active_marker}"),
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
    use otto_plugin::KeyMods;

    fn key(c: KeyCodePortable) -> KeyEventPortable {
        KeyEventPortable {
            code: c,
            modifiers: KeyMods::default(),
        }
    }

    #[tokio::test]
    async fn enter_emits_stack_setlocale_persist_true_then_close() {
        let mut s = LanguagePickerScreen::new(LanguagePicker::new("en"));
        let effs = s.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        match &effs[0] {
            Effect::Stack(children) => {
                assert!(matches!(
                    children[0],
                    Effect::SetActiveLocale { persist: true, .. }
                ));
                assert!(matches!(children[1], Effect::CloseScreen));
            }
            other => panic!("expected Stack, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn esc_reverts_to_pre_open_locale() {
        let mut s = LanguagePickerScreen::new(LanguagePicker::new("en"));
        // Drift the preview first.
        let _ = s.on_key(key(KeyCodePortable::Down)).await.unwrap();
        let effs = s.on_key(key(KeyCodePortable::Esc)).await.unwrap();
        match &effs[0] {
            Effect::Stack(children) => match &children[0] {
                Effect::SetActiveLocale {
                    code,
                    persist: false,
                } => assert_eq!(code, "en"),
                other => panic!("unexpected first child: {other:?}"),
            },
            other => panic!("expected Stack, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn down_emits_preview_set_locale_persist_false() {
        let mut s = LanguagePickerScreen::new(LanguagePicker::new("en"));
        let effs = s.on_key(key(KeyCodePortable::Down)).await.unwrap();
        assert!(matches!(
            effs[0],
            Effect::SetActiveLocale { persist: false, .. }
        ));
    }

    #[test]
    fn id_is_language_picker() {
        let s = LanguagePickerScreen::new(LanguagePicker::new("en"));
        assert_eq!(s.id(), "language.picker");
    }

    #[test]
    fn render_includes_every_supported_language() {
        let s = LanguagePickerScreen::new(LanguagePicker::new("en"));
        let lines = s.render(Region {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        });
        let body: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|sp| sp.text.clone()))
            .collect::<Vec<_>>()
            .join(" ");
        for l in super::super::catalog::supported() {
            assert!(
                body.contains(l.code),
                "missing code {} in render: {body}",
                l.code
            );
            assert!(
                body.contains(l.native_name),
                "missing native {} in render: {body}",
                l.native_name
            );
        }
    }
}

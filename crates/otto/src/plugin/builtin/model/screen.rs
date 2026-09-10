//! Model picker screen — choose from the active provider's advertised
//! models. Mirrors `internal:connect::screen::ConnectPickerScreen`.

use async_trait::async_trait;
use otto_plugin::{
    Effect, KeyCodePortable, KeyEventPortable, ModelEntry, PluginError, Region, Screen, StyledLine,
    StyledSpan, TextMods, ThemeColor,
};

/// Model picker screen. Renders one row per advertised model; Enter
/// emits [`Effect::SetActiveModel`] for the highlighted row. Esc closes
/// the screen without making any change. When the candidate list is
/// empty (provider has no `list_models` or it failed), the screen
/// renders an explanatory note and Enter just closes.
#[derive(Debug)]
pub struct ModelPickerScreen {
    models: Vec<ModelEntry>,
    cursor: usize,
    current_id: String,
}

impl ModelPickerScreen {
    /// Construct a picker pre-positioned on the row matching
    /// `current_id` (when present). When `current_id` isn't in
    /// `models`, the cursor starts at the top of the list.
    pub fn new(current_id: String, models: Vec<ModelEntry>) -> Self {
        let cursor = models.iter().position(|m| m.id == current_id).unwrap_or(0);
        Self {
            models,
            cursor,
            current_id,
        }
    }
}

#[async_trait]
impl Screen for ModelPickerScreen {
    fn id(&self) -> String {
        "model.picker".to_string()
    }

    fn render(&self, _region: Region) -> Vec<StyledLine> {
        if self.models.is_empty() {
            return vec![StyledLine {
                spans: vec![StyledSpan {
                    text: rust_i18n::t!("picker.model.no-models").to_string(),
                    fg: Some(ThemeColor::Warning),
                    bg: None,
                    modifiers: TextMods::default(),
                }],
            }];
        }
        self.models
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let is_cursor = i == self.cursor;
                let is_active = m.id == self.current_id;
                let cursor_marker = if is_cursor { "> " } else { "  " };
                let active_marker = if is_active { " ▶" } else { "" };
                let display = if m.display_name.is_empty() {
                    m.id.clone()
                } else {
                    m.display_name.clone()
                };
                StyledLine {
                    spans: vec![
                        StyledSpan {
                            text: format!("{cursor_marker}{display}"),
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
                            text: format!("  ({}){active_marker}", m.id),
                            fg: Some(ThemeColor::Muted),
                            bg: None,
                            modifiers: TextMods::default(),
                        },
                    ],
                }
            })
            .collect()
    }

    async fn on_key(&mut self, key: KeyEventPortable) -> Result<Vec<Effect>, PluginError> {
        match key.code {
            KeyCodePortable::Esc => Ok(vec![Effect::CloseScreen]),
            KeyCodePortable::Up | KeyCodePortable::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                Ok(vec![])
            }
            KeyCodePortable::Down | KeyCodePortable::Char('j') => {
                let max = self.models.len().saturating_sub(1);
                if self.cursor < max {
                    self.cursor += 1;
                }
                Ok(vec![])
            }
            KeyCodePortable::Enter => {
                let Some(model) = self.models.get(self.cursor).cloned() else {
                    return Ok(vec![Effect::CloseScreen]);
                };
                Ok(vec![Effect::Stack(vec![
                    Effect::CloseScreen,
                    Effect::SetActiveModel {
                        id: model.id,
                        persist: true,
                    },
                ])])
            }
            _ => Ok(vec![]),
        }
    }

    fn tips(&self) -> Vec<StyledLine> {
        vec![StyledLine::plain(
            rust_i18n::t!("picker.model.tips").to_string(),
        )]
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

    fn flash() -> ModelEntry {
        ModelEntry {
            id: "gemini-2.5-flash".into(),
            display_name: "Gemini 2.5 Flash".into(),
        }
    }

    fn pro() -> ModelEntry {
        ModelEntry {
            id: "gemini-2.5-pro".into(),
            display_name: "Gemini 2.5 Pro".into(),
        }
    }

    #[tokio::test]
    async fn empty_models_renders_helper_text() {
        let s = ModelPickerScreen::new(String::new(), vec![]);
        let lines = s.render(Region {
            x: 0,
            y: 0,
            width: 60,
            height: 10,
        });
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|sp| sp.text.clone()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains(rust_i18n::t!("picker.model.no-models").as_ref()),
            "expected no-models text, got: {joined}"
        );
        // Pins span colours so the #117 constructor rewrite cannot change them silently.
        assert_eq!(lines[0].spans[0].fg, Some(ThemeColor::Warning));
    }

    // Pins span colours so the #117 constructor rewrite cannot change them silently.
    #[test]
    fn row_spans_pin_cursor_accent_and_muted_id_column() {
        let s = ModelPickerScreen::new("gemini-2.5-flash".into(), vec![flash(), pro()]);
        let lines = s.render(Region {
            x: 0,
            y: 0,
            width: 60,
            height: 10,
        });
        let cursor_row = &lines[0];
        assert_eq!(cursor_row.spans[0].fg, Some(ThemeColor::Accent));
        assert!(cursor_row.spans[0].modifiers.bold);
        assert_eq!(cursor_row.spans[1].fg, Some(ThemeColor::Muted));

        let other_row = &lines[1];
        assert_eq!(other_row.spans[0].fg, Some(ThemeColor::Fg));
        assert!(!other_row.spans[0].modifiers.bold);
        assert_eq!(other_row.spans[1].fg, Some(ThemeColor::Muted));
    }

    #[tokio::test]
    async fn cursor_clamps_to_bounds() {
        let mut s = ModelPickerScreen::new("gemini-2.5-flash".into(), vec![flash(), pro()]);
        // Cursor starts at 0 (flash); Up at index 0 must stay at 0.
        let _ = s.on_key(key(KeyCodePortable::Up)).await.unwrap();
        assert_eq!(s.cursor, 0);
        // Down advances; second Down at len-1 must clamp.
        let _ = s.on_key(key(KeyCodePortable::Down)).await.unwrap();
        assert_eq!(s.cursor, 1);
        let _ = s.on_key(key(KeyCodePortable::Down)).await.unwrap();
        assert_eq!(s.cursor, 1);
    }

    #[tokio::test]
    async fn enter_emits_set_active_model_with_selected_id() {
        let mut s = ModelPickerScreen::new("gemini-2.5-flash".into(), vec![flash(), pro()]);
        let _ = s.on_key(key(KeyCodePortable::Down)).await.unwrap();
        let effs = s.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        match &effs[0] {
            Effect::Stack(children) => {
                assert!(matches!(children[0], Effect::CloseScreen));
                match &children[1] {
                    Effect::SetActiveModel { id, persist } => {
                        assert_eq!(id, "gemini-2.5-pro");
                        assert!(*persist);
                    }
                    other => panic!("expected SetActiveModel, got {other:?}"),
                }
            }
            other => panic!("expected Stack, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn enter_with_empty_just_closes() {
        let mut s = ModelPickerScreen::new(String::new(), vec![]);
        let effs = s.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        assert!(matches!(effs[0], Effect::CloseScreen));
    }

    #[tokio::test]
    async fn esc_closes() {
        let mut s = ModelPickerScreen::new(String::new(), vec![flash()]);
        let effs = s.on_key(key(KeyCodePortable::Esc)).await.unwrap();
        assert!(matches!(effs[0], Effect::CloseScreen));
    }

    #[test]
    fn cursor_initialized_to_current_id_row_when_present() {
        let s = ModelPickerScreen::new("gemini-2.5-pro".into(), vec![flash(), pro()]);
        assert_eq!(s.cursor, 1, "cursor must start on the current_id row");
    }
}

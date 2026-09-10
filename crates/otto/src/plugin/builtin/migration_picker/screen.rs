//! First-launch migration picker screen. Multi-select list of detected
//! providers; user confirms with Enter, dismisses with Esc. Emits a
//! synthetic slash that the migration_picker plugin handles to write
//! config.toml and close.

use async_trait::async_trait;
use otto_plugin::{
    Effect, KeyCodePortable, KeyEventPortable, PluginError, ProviderId, Region, Screen, StyledLine,
    ThemeColor,
};

/// First-launch migration picker screen.
///
/// Starts with all detected providers selected; the user can toggle
/// individual rows with Space before pressing Enter to confirm.
#[derive(Debug)]
pub struct MigrationPickerScreen {
    /// `(provider_id, selected)` rows. Starts all-selected so the user
    /// can confirm "all of them" with one Enter press.
    rows: Vec<(ProviderId, bool)>,
    cursor: usize,
}

impl MigrationPickerScreen {
    /// Construct the picker with the given provider ids, all pre-selected.
    pub fn new(detected: Vec<ProviderId>) -> Self {
        let rows = detected.into_iter().map(|id| (id, true)).collect();
        Self { rows, cursor: 0 }
    }

    /// Returns the display string for each selected row. Converts to `String`
    /// only at this render/emit boundary.
    fn selected_ids(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|(_, sel)| *sel)
            .map(|(id, _)| id.as_str().to_string())
            .collect()
    }
}

#[async_trait]
impl Screen for MigrationPickerScreen {
    fn id(&self) -> String {
        "migration.picker".to_string()
    }

    fn render(&self, _region: Region) -> Vec<StyledLine> {
        let title = rust_i18n::t!("migration.picker.title").to_string();
        let hint = rust_i18n::t!("migration.picker.hint").to_string();

        let mut lines = vec![
            StyledLine::plain(title),
            StyledLine::plain(""),
            StyledLine::muted(hint),
            StyledLine::plain(""),
        ];

        for (i, (id, selected)) in self.rows.iter().enumerate() {
            let arrow = if i == self.cursor { "▶ " } else { "  " };
            let row_text = if *selected {
                rust_i18n::t!("migration.picker.row-selected", name = id.as_str()).to_string()
            } else {
                rust_i18n::t!("migration.picker.row-unselected", name = id.as_str()).to_string()
            };
            lines.push(StyledLine::plain(format!("{arrow}{row_text}")));
        }
        lines
    }

    async fn on_key(&mut self, key: KeyEventPortable) -> Result<Vec<Effect>, PluginError> {
        match key.code {
            KeyCodePortable::Esc => Ok(vec![Effect::Stack(vec![
                Effect::CloseScreen,
                Effect::RunSlash {
                    name: "_internal:migration-dismiss".into(),
                    args: vec![],
                },
            ])]),
            KeyCodePortable::Up => {
                self.cursor = self.cursor.saturating_sub(1);
                Ok(vec![])
            }
            KeyCodePortable::Down => {
                let max = self.rows.len().saturating_sub(1);
                if self.cursor < max {
                    self.cursor += 1;
                }
                Ok(vec![])
            }
            KeyCodePortable::Char(' ') => {
                if let Some(row) = self.rows.get_mut(self.cursor) {
                    row.1 = !row.1;
                }
                Ok(vec![])
            }
            KeyCodePortable::Enter => {
                let selected = self.selected_ids();
                Ok(vec![Effect::Stack(vec![
                    Effect::CloseScreen,
                    Effect::RunSlash {
                        name: "_internal:migration-confirm".into(),
                        args: selected,
                    },
                ])])
            }
            _ => Ok(vec![]),
        }
    }

    fn tips(&self) -> Vec<StyledLine> {
        vec![StyledLine::plain(
            "Space toggle · Enter confirm · Esc use default".to_string(),
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otto_plugin::KeyMods;

    fn pid(s: &str) -> ProviderId {
        ProviderId::new(s).unwrap()
    }

    fn key(c: KeyCodePortable) -> KeyEventPortable {
        KeyEventPortable {
            code: c,
            modifiers: KeyMods::default(),
        }
    }

    fn region() -> Region {
        Region {
            x: 0,
            y: 0,
            width: 60,
            height: 20,
        }
    }

    #[tokio::test]
    async fn enter_emits_confirm_slash_with_selected_ids() {
        let mut s = MigrationPickerScreen::new(vec![pid("anthropic"), pid("gemini")]);
        // Both selected by default.
        let effs = s.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        match &effs[0] {
            Effect::Stack(children) => {
                assert!(matches!(children[0], Effect::CloseScreen));
                match &children[1] {
                    Effect::RunSlash { name, args } => {
                        assert_eq!(name, "_internal:migration-confirm");
                        // selected_ids() converts ProviderId to String at boundary.
                        assert_eq!(args, &vec!["anthropic".to_string(), "gemini".to_string()]);
                    }
                    _ => panic!("expected RunSlash, got {:?}", children[1]),
                }
            }
            _ => panic!("expected Stack, got {:?}", effs[0]),
        }
    }

    #[tokio::test]
    async fn space_toggles_selection() {
        let mut s = MigrationPickerScreen::new(vec![pid("anthropic"), pid("gemini")]);
        // Toggle the first row off.
        let _ = s.on_key(key(KeyCodePortable::Char(' '))).await.unwrap();
        let effs = s.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        match &effs[0] {
            Effect::Stack(children) => match &children[1] {
                Effect::RunSlash { args, .. } => {
                    assert_eq!(args, &vec!["gemini".to_string()]);
                }
                _ => panic!("expected RunSlash, got {:?}", children[1]),
            },
            _ => panic!("expected Stack, got {:?}", effs[0]),
        }
    }

    #[tokio::test]
    async fn esc_emits_dismiss_slash() {
        let mut s = MigrationPickerScreen::new(vec![pid("anthropic"), pid("gemini")]);
        let effs = s.on_key(key(KeyCodePortable::Esc)).await.unwrap();
        match &effs[0] {
            Effect::Stack(children) => {
                assert!(matches!(children[0], Effect::CloseScreen));
                match &children[1] {
                    Effect::RunSlash { name, .. } => {
                        assert_eq!(name, "_internal:migration-dismiss");
                    }
                    _ => panic!("expected RunSlash, got {:?}", children[1]),
                }
            }
            _ => panic!("expected Stack, got {:?}", effs[0]),
        }
    }

    #[tokio::test]
    async fn down_then_space_toggles_second_row() {
        let mut s = MigrationPickerScreen::new(vec![pid("anthropic"), pid("gemini")]);
        let _ = s.on_key(key(KeyCodePortable::Down)).await.unwrap();
        let _ = s.on_key(key(KeyCodePortable::Char(' '))).await.unwrap();
        // First still selected, second deselected.
        assert!(s.rows[0].1);
        assert!(!s.rows[1].1);
    }

    #[test]
    fn render_shows_title_and_rows() {
        let s = MigrationPickerScreen::new(vec![pid("anthropic"), pid("gemini")]);
        let lines = s.render(region());
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|sp| sp.text.clone()))
            .collect::<Vec<_>>()
            .join("\n");
        // Title should appear.
        assert!(
            joined.contains("First launch"),
            "expected title in render, got: {joined}"
        );
        // Both provider ids should appear.
        assert!(joined.contains("anthropic"), "anthropic missing: {joined}");
        assert!(joined.contains("gemini"), "gemini missing: {joined}");
        // Pins span colours so the #117 constructor rewrite cannot change them silently.
        // Title and row lines are unstyled (fg: None); only the hint line is Muted.
        assert_eq!(lines[0].spans[0].fg, None);
        assert_eq!(lines[1].spans[0].fg, None);
        assert_eq!(lines[2].spans[0].fg, Some(ThemeColor::Muted));
        assert_eq!(lines[3].spans[0].fg, None);
        assert_eq!(lines[4].spans[0].fg, None);
    }
}

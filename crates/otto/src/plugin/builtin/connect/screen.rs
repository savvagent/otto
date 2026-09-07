//! Provider picker — choose from registered provider plugins.

use async_trait::async_trait;
use otto_plugin::{
    Effect, KeyCodePortable, KeyEventPortable, PluginError, ProviderId, Region, Screen, StyledLine,
    StyledSpan, TextMods, ThemeColor,
};

/// Provider picker screen.
///
/// v0.9 ships an empty fallback list. PR 6 wires registered-provider
/// discovery via `HostEvent::ProviderRegistered` + an in-memory cache
/// here. For PR 5 the screen renders "no providers registered yet".
#[derive(Debug)]
pub struct ConnectPickerScreen {
    candidates: Vec<(ProviderId, String)>,
    cursor: usize,
}

impl ConnectPickerScreen {
    /// Construct a picker with no pre-loaded candidates (PR 5 default).
    pub fn new() -> Self {
        Self {
            candidates: vec![],
            cursor: 0,
        }
    }

    /// Public constructor used by `ConnectPlugin::create_screen` (and by
    /// tests) to inject the registered-provider set into a freshly opened
    /// picker. `ConnectPlugin` accumulates candidates via
    /// [`Plugin::on_event`] on [`otto_plugin::HostEvent::ProviderRegistered`].
    pub fn with_candidates(candidates: Vec<(ProviderId, String)>) -> Self {
        Self {
            candidates,
            cursor: 0,
        }
    }
}

impl Default for ConnectPickerScreen {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Screen for ConnectPickerScreen {
    fn id(&self) -> String {
        "connect.picker".to_string()
    }

    fn render(&self, _region: Region) -> Vec<StyledLine> {
        if self.candidates.is_empty() {
            return vec![
                StyledLine::plain(rust_i18n::t!("picker.connect.no-providers").to_string()),
                StyledLine::plain(""),
                StyledLine {
                    spans: vec![StyledSpan {
                        text: rust_i18n::t!("picker.connect.open-plugins-hint").to_string(),
                        fg: Some(ThemeColor::Warning),
                        bg: None,
                        modifiers: TextMods::default(),
                    }],
                },
            ];
        }
        self.candidates
            .iter()
            .enumerate()
            .map(|(i, (id, display))| {
                let marker = if i == self.cursor { "▶ " } else { "  " };
                StyledLine::plain(format!("{marker}{display}  ({})", id.as_str()))
            })
            .collect()
    }

    async fn on_key(&mut self, key: KeyEventPortable) -> Result<Vec<Effect>, PluginError> {
        match key.code {
            KeyCodePortable::Esc => Ok(vec![Effect::CloseScreen]),
            KeyCodePortable::Up => {
                self.cursor = self.cursor.saturating_sub(1);
                Ok(vec![])
            }
            KeyCodePortable::Down => {
                let max = self.candidates.len().saturating_sub(1);
                if self.cursor < max {
                    self.cursor += 1;
                }
                Ok(vec![])
            }
            KeyCodePortable::Enter => {
                let Some((pid, _)) = self.candidates.get(self.cursor).cloned() else {
                    return Ok(vec![Effect::CloseScreen]);
                };
                let name = if key.modifiers.alt {
                    format!("connect {} --rekey", pid.as_str())
                } else {
                    format!("connect {}", pid.as_str())
                };
                Ok(vec![Effect::Stack(vec![
                    Effect::CloseScreen,
                    Effect::RunSlash { name, args: vec![] },
                ])])
            }
            _ => Ok(vec![]),
        }
    }

    fn tips(&self) -> Vec<StyledLine> {
        vec![StyledLine::plain(
            rust_i18n::t!("picker.connect.tips").to_string(),
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

    #[tokio::test]
    async fn empty_candidates_renders_helper_text() {
        let s = ConnectPickerScreen::new();
        let lines = s.render(Region {
            x: 0,
            y: 0,
            width: 60,
            height: 10,
        });
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.text.clone()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains(rust_i18n::t!("picker.connect.no-providers").as_ref()),
            "expected no-providers text, got: {joined}"
        );
        assert!(
            joined.contains(rust_i18n::t!("picker.connect.open-plugins-hint").as_ref()),
            "expected open-plugins-hint text, got: {joined}"
        );
    }

    #[tokio::test]
    async fn enter_with_candidate_routes_to_connect_provider_slash() {
        let mut s = ConnectPickerScreen::with_candidates(vec![(
            ProviderId::new("anthropic").unwrap(),
            "Anthropic".into(),
        )]);
        let effs = s.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        match &effs[0] {
            Effect::Stack(children) => {
                assert!(matches!(children[0], Effect::CloseScreen));
                match &children[1] {
                    Effect::RunSlash { name, .. } => assert_eq!(name, "connect anthropic"),
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
    }

    #[tokio::test]
    async fn alt_enter_emits_rekey_slash() {
        let mut s = ConnectPickerScreen::with_candidates(vec![(
            ProviderId::new("anthropic").unwrap(),
            "Anthropic".into(),
        )]);
        let mut k = key(KeyCodePortable::Enter);
        k.modifiers.alt = true;
        let effs = s.on_key(k).await.unwrap();
        match &effs[0] {
            Effect::Stack(children) => {
                assert!(matches!(children[0], Effect::CloseScreen));
                match &children[1] {
                    Effect::RunSlash { name, .. } => {
                        assert_eq!(name, "connect anthropic --rekey");
                    }
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
    }
}

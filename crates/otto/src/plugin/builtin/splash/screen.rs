//! Splash screen — fullscreen HUD with connect status.

use async_trait::async_trait;
use otto_plugin::{
    Effect, KeyCodePortable, KeyEventPortable, PluginError, ProviderId, Region, Screen, StyledLine,
    StyledSpan, TextMods, ThemeColor,
};

/// Cached connect state forwarded from [`super::SplashPlugin`] to each new
/// screen instance so the displayed status survives open/close cycles.
#[derive(Clone)]
pub struct CachedHud {
    /// Whether a successful connect event has been received.
    pub connected: bool,
    /// The provider that connected, if known.
    pub last_provider: Option<ProviderId>,
}

/// Fullscreen splash screen displaying the startup HUD and connect status.
pub struct SplashScreen {
    /// Cached HUD state passed in at construction time.
    pub hud: Option<CachedHud>,
}

impl SplashScreen {
    /// Create a new `SplashScreen` with the given cached HUD state.
    pub fn new(cached: Option<CachedHud>) -> Self {
        Self { hud: cached }
    }
}

#[async_trait]
impl Screen for SplashScreen {
    fn id(&self) -> String {
        "splash".to_string()
    }

    fn render(&self, _region: Region) -> Vec<StyledLine> {
        let mut lines = vec![
            StyledLine {
                spans: vec![StyledSpan {
                    text: rust_i18n::t!("splash.app-name").to_string(),
                    fg: Some(ThemeColor::Accent),
                    bg: None,
                    modifiers: TextMods {
                        bold: true,
                        ..Default::default()
                    },
                }],
            },
            StyledLine::plain(""),
        ];
        let status = match &self.hud {
            Some(h) if h.connected => match &h.last_provider {
                Some(p) => rust_i18n::t!("splash.connected-to", provider = p.as_str()).to_string(),
                None => rust_i18n::t!("splash.connected").to_string(),
            },
            _ => rust_i18n::t!("splash.connecting").to_string(),
        };
        lines.push(StyledLine::plain(status));
        lines.push(StyledLine::plain(""));
        lines.push(StyledLine::plain(
            rust_i18n::t!("splash.press-esc-dismiss").to_string(),
        ));
        lines
    }

    async fn on_key(&mut self, key: KeyEventPortable) -> Result<Vec<Effect>, PluginError> {
        match key.code {
            KeyCodePortable::Esc | KeyCodePortable::Enter => Ok(vec![Effect::CloseScreen]),
            _ => Ok(vec![]),
        }
    }

    fn tips(&self) -> Vec<StyledLine> {
        vec![StyledLine::plain(rust_i18n::t!("splash.tips").to_string())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otto_plugin::KeyMods;

    #[tokio::test]
    async fn esc_emits_close_screen() {
        let mut s = SplashScreen::new(None);
        let effs = s
            .on_key(KeyEventPortable {
                code: KeyCodePortable::Esc,
                modifiers: KeyMods::default(),
            })
            .await
            .unwrap();
        assert!(matches!(effs.first(), Some(Effect::CloseScreen)));
    }

    #[test]
    fn render_includes_dismiss_hint() {
        let s = SplashScreen::new(None);
        let lines = s.render(Region {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        });
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.text.clone()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains(rust_i18n::t!("splash.press-esc-dismiss").as_ref()));
    }
}

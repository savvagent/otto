//! Splash screen — shared startup splash content exposed as a Screen.

use async_trait::async_trait;
use otto_plugin::{
    Effect, KeyEventPortable, PluginError, Region, Screen, StyledLine, StyledSpan, TextMods,
    ThemeColor,
};

use crate::splash::{SandboxSplashState, SplashLineKind, shared_content};

/// Fullscreen splash screen displaying the shared startup splash content.
pub struct SplashScreen {
    /// Cached sandbox state shown inline with the shared splash content.
    pub sandbox: SandboxSplashState,
}

impl SplashScreen {
    /// Create a new `SplashScreen` using the default splash sandbox state.
    pub fn new() -> Self {
        Self::with_sandbox(SandboxSplashState::OnDefault)
    }

    /// Create a splash screen with the given cached sandbox state.
    pub fn with_sandbox(sandbox: SandboxSplashState) -> Self {
        Self { sandbox }
    }
}

#[async_trait]
impl Screen for SplashScreen {
    fn id(&self) -> String {
        "splash".to_string()
    }

    fn render(&self, _region: Region) -> Vec<StyledLine> {
        shared_content(&self.sandbox)
            .into_iter()
            .map(|line| {
                let (fg, modifiers) = match line.kind {
                    SplashLineKind::Blank => (None, TextMods::default()),
                    SplashLineKind::Logo => (
                        Some(ThemeColor::Accent),
                        TextMods {
                            bold: true,
                            ..Default::default()
                        },
                    ),
                    SplashLineKind::Tagline => (Some(ThemeColor::Accent), TextMods::default()),
                    SplashLineKind::SandboxOn => (Some(ThemeColor::Green), TextMods::default()),
                    SplashLineKind::SandboxOff => (Some(ThemeColor::Yellow), TextMods::default()),
                    SplashLineKind::SandboxError => (
                        Some(ThemeColor::Red),
                        TextMods {
                            bold: true,
                            ..Default::default()
                        },
                    ),
                    SplashLineKind::Hint => (
                        Some(ThemeColor::Muted),
                        TextMods {
                            italic: true,
                            ..Default::default()
                        },
                    ),
                };
                StyledLine {
                    spans: vec![StyledSpan {
                        text: line.text,
                        fg,
                        bg: None,
                        modifiers,
                    }],
                }
            })
            .collect()
    }

    async fn on_key(&mut self, key: KeyEventPortable) -> Result<Vec<Effect>, PluginError> {
        let _ = key;
        Ok(vec![Effect::CloseScreen])
    }

    fn tips(&self) -> Vec<StyledLine> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otto_plugin::KeyMods;

    #[tokio::test]
    async fn esc_emits_close_screen() {
        let mut s = SplashScreen::new();
        let effs = s
            .on_key(KeyEventPortable {
                code: otto_plugin::KeyCodePortable::Esc,
                modifiers: KeyMods::default(),
            })
            .await
            .unwrap();
        assert!(matches!(effs.first(), Some(Effect::CloseScreen)));
    }

    #[test]
    fn render_includes_shared_splash_hint_and_sandbox_line() {
        let s = SplashScreen::with_sandbox(SandboxSplashState::OnDefault);
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
        assert!(joined.contains("sandbox: on (use /sandbox off to disable)"));
        assert!(joined.contains(&format!(
            "press any key to continue · v{}",
            env!("CARGO_PKG_VERSION")
        )));
    }

    #[tokio::test]
    async fn printable_key_also_closes_screen() {
        let mut s = SplashScreen::new();
        let effs = s
            .on_key(KeyEventPortable {
                code: otto_plugin::KeyCodePortable::Char('x'),
                modifiers: KeyMods::default(),
            })
            .await
            .unwrap();
        assert!(matches!(effs.first(), Some(Effect::CloseScreen)));
    }

    #[test]
    fn tips_are_empty_when_hint_is_inline() {
        let s = SplashScreen::new();
        assert!(s.tips().is_empty());
    }
}

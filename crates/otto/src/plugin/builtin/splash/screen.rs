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

#[derive(Debug, Clone)]
pub(crate) struct RenderedSplashLine {
    pub line: StyledLine,
    pub centered: bool,
}

pub(crate) fn shared_splash_styled_lines(sandbox: &SandboxSplashState) -> Vec<RenderedSplashLine> {
    shared_content(sandbox)
        .into_iter()
        .map(|line| {
            let (fg, modifiers) = match line.kind {
                SplashLineKind::Blank => (None, TextMods::default()),
                SplashLineKind::Logo => (
                    Some(ThemeColor::LightBlue),
                    TextMods {
                        bold: true,
                        ..Default::default()
                    },
                ),
                SplashLineKind::Tagline => (Some(ThemeColor::LightBlue), TextMods::default()),
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
                    Some(ThemeColor::DarkGray),
                    TextMods {
                        italic: true,
                        ..Default::default()
                    },
                ),
            };
            RenderedSplashLine {
                line: StyledLine {
                    spans: vec![StyledSpan {
                        text: line.text,
                        fg,
                        bg: None,
                        modifiers,
                    }],
                },
                centered: line.centered,
            }
        })
        .collect()
}

fn center_text(text: &str, width: u16) -> String {
    let text_width = text.chars().count();
    let left_pad = usize::from(width).saturating_sub(text_width) / 2;
    format!("{}{}", " ".repeat(left_pad), text)
}

#[async_trait]
impl Screen for SplashScreen {
    fn id(&self) -> String {
        "splash".to_string()
    }

    fn render(&self, region: Region) -> Vec<StyledLine> {
        shared_splash_styled_lines(&self.sandbox)
            .into_iter()
            .map(|mut line| {
                if line.centered {
                    line.line.spans[0].text = center_text(&line.line.spans[0].text, region.width);
                }
                line.line
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

    #[test]
    fn render_uses_startup_splash_colors_for_logo_and_hint() {
        let s = SplashScreen::with_sandbox(SandboxSplashState::OnDefault);
        let lines = s.render(Region {
            x: 0,
            y: 0,
            width: 100,
            height: 24,
        });

        let logo = lines
            .iter()
            .find(|line| {
                line.spans
                    .first()
                    .is_some_and(|span| span.text.trim_start().starts_with("███████╗"))
            })
            .expect("logo row present");
        assert_eq!(logo.spans[0].fg, Some(ThemeColor::LightBlue));

        let hint = lines
            .iter()
            .find(|line| {
                line.spans.first().is_some_and(|span| {
                    span.text.trim()
                        == format!("press any key to continue · v{}", env!("CARGO_PKG_VERSION"))
                })
            })
            .expect("hint row present");
        assert_eq!(hint.spans[0].fg, Some(ThemeColor::DarkGray));
    }

    #[test]
    fn render_centers_shared_splash_content_for_direct_renderers() {
        let s = SplashScreen::with_sandbox(SandboxSplashState::OnDefault);
        let lines = s.render(Region {
            x: 0,
            y: 0,
            width: 100,
            height: 24,
        });
        let rendered = lines
            .iter()
            .map(|line| {
                line.spans
                    .first()
                    .expect("single-span splash line")
                    .text
                    .as_str()
            })
            .collect::<Vec<_>>();

        let logo = rendered
            .iter()
            .find(|text| text.trim_start().starts_with("███████╗"))
            .expect("logo row present");
        assert!(
            logo.starts_with("           "),
            "logo row should be padded to center within direct renderers: {logo:?}"
        );

        let tagline = rendered
            .iter()
            .find(|text| text.trim() == "the savvy MCP-native terminal coding agent")
            .expect("tagline row present");
        assert_ne!(
            tagline.trim_start().len(),
            tagline.len(),
            "tagline should be padded to center within direct renderers: {tagline:?}"
        );

        let sandbox = rendered
            .iter()
            .find(|text| text.trim() == "sandbox: on (use /sandbox off to disable)")
            .expect("sandbox row present");
        assert_ne!(
            sandbox.trim_start().len(),
            sandbox.len(),
            "sandbox line should be padded to center within direct renderers: {sandbox:?}"
        );

        let hint = rendered
            .iter()
            .find(|text| {
                text.trim() == format!("press any key to continue · v{}", env!("CARGO_PKG_VERSION"))
            })
            .expect("hint row present");
        assert_ne!(
            hint.trim_start().len(),
            hint.len(),
            "hint row should be padded to center within direct renderers: {hint:?}"
        );
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

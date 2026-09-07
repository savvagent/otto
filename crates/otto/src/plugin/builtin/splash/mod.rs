//! `internal:splash` — startup HUD + parse-error rendering, exposed as a
//! Screen. The poll-loop wiring stays in main.rs in PR 3; PR 7 replaces
//! the poll with on_event(Connect) dispatch from the host.

pub mod screen;

use async_trait::async_trait;
use otto_plugin::{
    Contributions, Effect, Manifest, Plugin, PluginError, PluginId, PluginKind, Screen, ScreenArgs,
    ScreenLayout, ScreenSpec, SlashSpec,
};

use screen::SplashScreen;

/// Plugin wrapper exposing the splash screen and `/splash` slash command.
pub struct SplashPlugin;

impl SplashPlugin {
    /// Create a new `SplashPlugin`.
    pub fn new() -> Self {
        Self
    }
}

impl Default for SplashPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for SplashPlugin {
    fn manifest(&self) -> Manifest {
        let mut contributions = Contributions::default();
        contributions.screens = vec![ScreenSpec {
            id: "splash".into(),
            layout: ScreenLayout::Fullscreen { hide_chrome: false },
        }];
        contributions.slash_commands = vec![SlashSpec {
            name: "splash".into(),
            summary: rust_i18n::t!("slash.splash-summary").to_string(),
            args_hint: None,
            requires_arg: false,
            suppress_prompt_segments: vec![],
        }];

        Manifest {
            id: PluginId::new("internal:splash").expect("valid built-in id"),
            name: "Splash".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: rust_i18n::t!("plugin.splash-description").to_string(),
            kind: PluginKind::Optional,
            contributions,
        }
    }

    async fn handle_slash(
        &mut self,
        _name: &str,
        _args: Vec<String>,
    ) -> Result<Vec<Effect>, PluginError> {
        Ok(vec![Effect::OpenScreen {
            id: "splash".into(),
            args: ScreenArgs::None,
        }])
    }

    fn create_screen(&self, id: &str, _args: ScreenArgs) -> Result<Box<dyn Screen>, PluginError> {
        if id != "splash" {
            return Err(PluginError::ScreenNotFound(id.to_string()));
        }
        Ok(Box::new(SplashScreen::new()))
    }
}

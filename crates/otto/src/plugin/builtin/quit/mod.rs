//! `internal:exit` — shuts down the application cleanly.

use async_trait::async_trait;
use otto_plugin::{
    Contributions, Effect, Manifest, Plugin, PluginError, PluginId, PluginKind, SlashSpec,
};

/// Plugin that registers the `/exit` slash command.
///
/// `/exit` emits [`Effect::Quit`], which `apply_effects` maps to
/// [`crate::app::App::request_quit`] (setting `should_quit = true` so the
/// event loop exits on its next tick).
///
/// Registered as [`PluginKind::Core`] so the plugins-manager screen
/// refuses to disable it — disabling `/exit` would leave the user with no
/// in-band way to leave the TUI from the command palette.
pub struct ExitPlugin;

impl ExitPlugin {
    /// Construct a new [`ExitPlugin`].
    pub fn new() -> Self {
        Self
    }
}

impl Default for ExitPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for ExitPlugin {
    fn manifest(&self) -> Manifest {
        let mut contributions = Contributions::default();
        contributions.slash_commands = vec![SlashSpec {
            name: "exit".into(),
            summary: rust_i18n::t!("slash.exit-summary").to_string(),
            args_hint: None,
            requires_arg: false,
            suppress_prompt_segments: vec![],
        }];
        Manifest {
            id: PluginId::new("internal:exit").expect("valid built-in id"),
            name: "Exit".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: rust_i18n::t!("plugin.exit-description").to_string(),
            kind: PluginKind::Core,
            contributions,
        }
    }

    async fn handle_slash(&mut self, _: &str, _: Vec<String>) -> Result<Vec<Effect>, PluginError> {
        Ok(vec![Effect::Quit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/exit` returns exactly one [`Effect::Quit`] — regression test for
    /// the post-v0.9 hotfix where `/exit` was missing from the plugin
    /// surface entirely and `Effect::RunSlash { name: "exit", .. }` from
    /// the palette hit `SlashError::Unknown`.
    #[tokio::test]
    async fn exit_returns_quit_effect() {
        let mut p = ExitPlugin::new();
        let effs = p.handle_slash("exit", vec![]).await.unwrap();
        assert_eq!(effs.len(), 1);
        assert!(matches!(effs[0], Effect::Quit));
    }

    #[tokio::test]
    async fn manifest_marks_exit_as_core() {
        let p = ExitPlugin::new();
        let m = p.manifest();
        assert_eq!(m.id.as_str(), "internal:exit");
        assert!(matches!(m.kind, PluginKind::Core));
        assert_eq!(m.contributions.slash_commands.len(), 1);
        assert_eq!(m.contributions.slash_commands[0].name, "exit");
    }
}

//! `internal:connect` — provider picker. Always opens the interactive
//! provider picker screen (`connect.picker`), regardless of arguments.

pub mod screen;

use async_trait::async_trait;
use otto_plugin::{
    Contributions, Effect, HookKind, HostEvent, Manifest, Plugin, PluginError, PluginId,
    PluginKind, ProviderId, Screen, ScreenArgs, ScreenLayout, ScreenSpec, SlashSpec,
};

use screen::ConnectPickerScreen;

/// Core plugin that exposes `/connect`.
///
/// Regardless of any arguments supplied, this always pushes the
/// `connect.picker` screen so the user can choose a provider interactively.
///
/// The plugin keeps an in-memory list of provider candidates, refreshed
/// whenever a [`HostEvent::ProviderRegistered`] arrives via [`Plugin::on_event`].
pub struct ConnectPlugin {
    /// (id, display_name) pairs collected from
    /// [`HostEvent::ProviderRegistered`] hooks; passed to the picker screen
    /// every time `/connect` opens it.
    candidates: Vec<(ProviderId, String)>,
}

impl ConnectPlugin {
    /// Construct a new `ConnectPlugin` with no candidates yet.
    pub fn new() -> Self {
        Self { candidates: vec![] }
    }

    /// Test-only accessor for the candidate list.
    #[cfg(test)]
    pub fn candidates(&self) -> &[(ProviderId, String)] {
        &self.candidates
    }
}

impl Default for ConnectPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for ConnectPlugin {
    fn manifest(&self) -> Manifest {
        let mut contributions = Contributions::default();
        contributions.slash_commands = vec![SlashSpec {
            name: "connect".into(),
            summary: rust_i18n::t!("slash.connect-summary").to_string(),
            args_hint: None,
            requires_arg: false,
            suppress_prompt_segments: vec![],
        }];
        contributions.screens = vec![ScreenSpec {
            id: "connect.picker".into(),
            layout: ScreenLayout::CenteredModal {
                width_pct: 50,
                height_pct: 50,
                title: Some(rust_i18n::t!("picker.connect.modal-title").to_string()),
            },
        }];
        // Subscribe to ProviderRegistered so we can keep our candidate
        // list current as provider plugins announce constructed clients.
        contributions.hooks = vec![HookKind::ProviderRegistered];

        Manifest {
            id: PluginId::new("internal:connect").expect("valid built-in id"),
            name: "Connect".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: rust_i18n::t!("plugin.connect-description").to_string(),
            kind: PluginKind::Core,
            contributions,
        }
    }

    async fn handle_slash(
        &mut self,
        _: &str,
        _args: Vec<String>,
    ) -> Result<Vec<Effect>, PluginError> {
        // Always open the interactive provider picker, regardless of any
        // arguments the user may have typed. The arg-routing path (which
        // dispatched `/connect <provider>` directly) was removed in issue #82
        // to unify on a single entry-point.
        Ok(vec![Effect::OpenScreen {
            id: "connect.picker".into(),
            args: ScreenArgs::ConnectPicker,
        }])
    }

    async fn on_event(&mut self, event: HostEvent) -> Result<Vec<Effect>, PluginError> {
        if let HostEvent::ProviderRegistered { id, display_name } = event {
            // Deduplicate: if this id is already in the candidate list, just
            // refresh the display name in place rather than appending a
            // duplicate row.
            if let Some(existing) = self.candidates.iter_mut().find(|(pid, _)| pid == &id) {
                existing.1 = display_name;
            } else {
                self.candidates.push((id, display_name));
            }
        }
        Ok(vec![])
    }

    fn create_screen(&self, id: &str, args: ScreenArgs) -> Result<Box<dyn Screen>, PluginError> {
        match (id, args) {
            ("connect.picker", ScreenArgs::ConnectPicker) => Ok(Box::new(
                ConnectPickerScreen::with_candidates(self.candidates.clone()),
            )),
            (other, _) => Err(PluginError::ScreenNotFound(other.to_string())),
        }
    }
}

/// Returns `true` when `name` belongs to the private `connect …` namespace
/// (i.e. names that start with `"connect "`). This namespace is shared by
/// the picker-screen dispatch slashes (`connect <provider_id>`) and the
/// palette filter; it is not part of the public slash-command surface.
pub(crate) fn is_internal_connect_namespace(name: &str) -> bool {
    name.starts_with("connect ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn handle_slash_always_opens_picker() {
        let mut p = ConnectPlugin::new();
        // No args → picker.
        let effs = p.handle_slash("connect", vec![]).await.unwrap();
        assert!(
            matches!(&effs[0], Effect::OpenScreen { id, .. } if id == "connect.picker"),
            "no-args case must open picker, got {effs:?}"
        );
        // Valid provider id arg → still picker (no direct routing).
        let effs = p
            .handle_slash("connect", vec!["anthropic".into()])
            .await
            .unwrap();
        assert!(
            matches!(&effs[0], Effect::OpenScreen { id, .. } if id == "connect.picker"),
            "valid-id case must open picker, got {effs:?}"
        );
        // Junk arg → still picker (no validation error).
        let effs = p
            .handle_slash("connect", vec!["bad provider".into()])
            .await
            .unwrap();
        assert!(
            matches!(&effs[0], Effect::OpenScreen { id, .. } if id == "connect.picker"),
            "junk-arg case must open picker, got {effs:?}"
        );
    }

    #[tokio::test]
    async fn on_event_provider_registered_appends_candidate() {
        let mut p = ConnectPlugin::new();
        let id = ProviderId::new("anthropic").unwrap();
        let effs = p
            .on_event(HostEvent::ProviderRegistered {
                id: id.clone(),
                display_name: "Anthropic".into(),
            })
            .await
            .unwrap();
        assert!(effs.is_empty(), "on_event must not emit effects yet");
        assert_eq!(p.candidates().len(), 1);
        assert_eq!(p.candidates()[0].0, id);
        assert_eq!(p.candidates()[0].1, "Anthropic");
    }

    #[tokio::test]
    async fn on_event_dedupes_repeated_registrations() {
        let mut p = ConnectPlugin::new();
        let id = ProviderId::new("anthropic").unwrap();
        for label in ["Anthropic", "Anthropic (Claude)"] {
            p.on_event(HostEvent::ProviderRegistered {
                id: id.clone(),
                display_name: label.into(),
            })
            .await
            .unwrap();
        }
        assert_eq!(p.candidates().len(), 1, "duplicate id must dedupe");
        assert_eq!(p.candidates()[0].1, "Anthropic (Claude)");
    }

    #[tokio::test]
    async fn on_event_ignores_other_events() {
        let mut p = ConnectPlugin::new();
        p.on_event(HostEvent::HostStarting).await.unwrap();
        assert!(p.candidates().is_empty());
    }

    #[test]
    fn manifest_subscribes_to_provider_registered() {
        let p = ConnectPlugin::new();
        let m = p.manifest();
        assert!(
            m.contributions
                .hooks
                .contains(&HookKind::ProviderRegistered)
        );
    }

    #[test]
    fn is_internal_connect_namespace_matches_prefixed_names() {
        assert!(is_internal_connect_namespace("connect anthropic"));
        assert!(is_internal_connect_namespace("connect my-provider"));
        assert!(!is_internal_connect_namespace("connect"));
        assert!(!is_internal_connect_namespace("connectpicker"));
        assert!(!is_internal_connect_namespace("disconnect foo"));
    }
}

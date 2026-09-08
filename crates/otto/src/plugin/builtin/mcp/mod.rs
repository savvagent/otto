//! `internal:mcp` — manage user-configured MCP tool servers.

pub mod screen;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use otto_host::ToolServerStatus;
use otto_plugin::{
    Contributions, Effect, Manifest, Plugin, PluginError, PluginId, PluginKind, Screen, ScreenArgs,
    ScreenLayout, ScreenSpec, SlashSpec,
};

use crate::config_file::McpServerEntry;
use crate::{HostSlot, McpManagerSeed};

use screen::McpManagerScreen;

const SCREEN_ID: &str = "mcp.manager";

#[async_trait]
pub(crate) trait McpManagerOps: Send + Sync {
    async fn read_statuses(&self) -> Vec<ToolServerStatus>;
    async fn write_server(&self, entry: &McpServerEntry) -> Result<(), String>;
    async fn remove_server(&self, name: &str) -> Result<bool, String>;
    async fn save_secret(&self, name: &str, secret: &str) -> Result<(), String>;
    async fn delete_secret(&self, name: &str) -> Result<(), String>;
    async fn begin_oauth(
        &self,
        name: &str,
        url: &str,
        requested_scopes: &[String],
    ) -> Result<crate::mcp_oauth::BeginAuthorizationResult, String>;
    async fn poll_oauth(
        &self,
        name: &str,
    ) -> Result<crate::mcp_oauth::PollAuthorizationResult, String>;
    async fn clear_oauth(&self, name: &str) -> Result<(), String>;
}

struct RealMcpManagerOps {
    host_slot: HostSlot,
    config_path: PathBuf,
    pending_oauth: tokio::sync::Mutex<
        std::collections::HashMap<String, crate::mcp_oauth::PendingMcpOAuthSession>,
    >,
}

impl RealMcpManagerOps {
    fn new(host_slot: HostSlot) -> Self {
        Self {
            host_slot,
            config_path: crate::config_file::ConfigFile::default_path(),
            pending_oauth: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

#[async_trait]
impl McpManagerOps for RealMcpManagerOps {
    async fn read_statuses(&self) -> Vec<ToolServerStatus> {
        let Some(host) = crate::current_host(&self.host_slot).await else {
            return vec![];
        };
        host.tool_server_statuses().await
    }

    async fn write_server(&self, entry: &McpServerEntry) -> Result<(), String> {
        crate::mcp_config_writer::add_server(
            &self.config_path,
            crate::mcp_config_writer::entry_table(entry),
        )
        .map_err(|err| format!("failed to update {}: {err}", self.config_path.display()))
    }

    async fn remove_server(&self, name: &str) -> Result<bool, String> {
        self.clear_oauth(name).await?;
        crate::mcp_config_writer::remove_server(&self.config_path, name)
            .map_err(|err| format!("failed to update {}: {err}", self.config_path.display()))
    }

    async fn save_secret(&self, name: &str, secret: &str) -> Result<(), String> {
        crate::creds::mcp_save(name, secret)
            .map_err(|err| format!("failed to save keyring secret for `{name}`: {err}"))
    }

    async fn delete_secret(&self, name: &str) -> Result<(), String> {
        crate::creds::mcp_delete(name)
            .map_err(|err| format!("failed to delete keyring secret for `{name}`: {err}"))
    }

    async fn begin_oauth(
        &self,
        name: &str,
        url: &str,
        requested_scopes: &[String],
    ) -> Result<crate::mcp_oauth::BeginAuthorizationResult, String> {
        self.clear_oauth(name).await?;
        let (session, result) =
            crate::mcp_oauth::begin_authorization(name, url, requested_scopes).await?;
        self.pending_oauth
            .lock()
            .await
            .insert(name.to_string(), session);
        Ok(result)
    }

    async fn poll_oauth(
        &self,
        name: &str,
    ) -> Result<crate::mcp_oauth::PollAuthorizationResult, String> {
        let Some(mut session) = self.pending_oauth.lock().await.remove(name) else {
            return Ok(crate::mcp_oauth::PollAuthorizationResult::NotStarted);
        };
        let result = match session.poll().await {
            Ok(result) => result,
            Err(err) => {
                session.shutdown().await;
                return Err(err);
            }
        };
        match result {
            crate::mcp_oauth::PollAuthorizationResult::Pending { .. } => {
                self.pending_oauth
                    .lock()
                    .await
                    .insert(name.to_string(), session);
            }
            crate::mcp_oauth::PollAuthorizationResult::Completed { .. }
            | crate::mcp_oauth::PollAuthorizationResult::Failed { .. }
            | crate::mcp_oauth::PollAuthorizationResult::NotStarted => {
                session.shutdown().await;
            }
        }
        Ok(result)
    }

    async fn clear_oauth(&self, name: &str) -> Result<(), String> {
        if let Some(session) = self.pending_oauth.lock().await.remove(name) {
            session.shutdown().await;
        }
        Ok(())
    }
}

/// Core plugin that exposes `/mcp` and the MCP-server manager screen.
pub struct McpPlugin {
    seed: McpManagerSeed,
    statuses: Vec<ToolServerStatus>,
    ops: Arc<dyn McpManagerOps>,
}

impl McpPlugin {
    pub fn new(seed: McpManagerSeed, statuses: Vec<ToolServerStatus>, host_slot: HostSlot) -> Self {
        Self::with_ops(seed, statuses, Arc::new(RealMcpManagerOps::new(host_slot)))
    }

    fn with_ops(
        seed: McpManagerSeed,
        statuses: Vec<ToolServerStatus>,
        ops: Arc<dyn McpManagerOps>,
    ) -> Self {
        Self {
            seed,
            statuses,
            ops,
        }
    }
}

#[async_trait]
impl Plugin for McpPlugin {
    fn manifest(&self) -> Manifest {
        let mut contributions = Contributions::default();
        contributions.slash_commands = vec![SlashSpec {
            name: "mcp".into(),
            summary: "Manage configured MCP servers".into(),
            args_hint: None,
            requires_arg: false,
            suppress_prompt_segments: vec![],
        }];
        contributions.screens = vec![ScreenSpec {
            id: SCREEN_ID.into(),
            layout: ScreenLayout::CenteredModal {
                width_pct: 70,
                height_pct: 70,
                title: Some("MCP servers".into()),
            },
        }];
        Manifest {
            id: PluginId::new("internal:mcp").expect("valid built-in id"),
            name: "MCP servers".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Manage user-configured MCP tool servers".into(),
            kind: PluginKind::Core,
            contributions,
        }
    }

    async fn handle_slash(
        &mut self,
        name: &str,
        args: Vec<String>,
    ) -> Result<Vec<Effect>, PluginError> {
        if name != "mcp" {
            return Ok(vec![]);
        }
        if !args.is_empty() {
            return Err(PluginError::InvalidArgs("/mcp takes no args".into()));
        }
        Ok(vec![Effect::OpenScreen {
            id: SCREEN_ID.into(),
            args: ScreenArgs::None,
        }])
    }

    fn create_screen(&self, id: &str, args: ScreenArgs) -> Result<Box<dyn Screen>, PluginError> {
        match (id, args) {
            (SCREEN_ID, ScreenArgs::None) => Ok(Box::new(McpManagerScreen::new(
                self.seed.clone(),
                self.statuses.clone(),
                Arc::clone(&self.ops),
            ))),
            (SCREEN_ID, _other) => Err(PluginError::InvalidArgs("/mcp takes no args".into())),
            (other, _) => Err(PluginError::ScreenNotFound(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct StubOps {
        statuses: Mutex<Vec<ToolServerStatus>>,
    }

    #[async_trait]
    impl McpManagerOps for StubOps {
        async fn read_statuses(&self) -> Vec<ToolServerStatus> {
            self.statuses.lock().unwrap().clone()
        }

        async fn write_server(&self, _entry: &McpServerEntry) -> Result<(), String> {
            Ok(())
        }

        async fn remove_server(&self, _name: &str) -> Result<bool, String> {
            Ok(true)
        }

        async fn save_secret(&self, _name: &str, _secret: &str) -> Result<(), String> {
            Ok(())
        }

        async fn delete_secret(&self, _name: &str) -> Result<(), String> {
            Ok(())
        }

        async fn begin_oauth(
            &self,
            _name: &str,
            _url: &str,
            _requested_scopes: &[String],
        ) -> Result<crate::mcp_oauth::BeginAuthorizationResult, String> {
            Err("not implemented in stub".into())
        }

        async fn poll_oauth(
            &self,
            _name: &str,
        ) -> Result<crate::mcp_oauth::PollAuthorizationResult, String> {
            Ok(crate::mcp_oauth::PollAuthorizationResult::NotStarted)
        }

        async fn clear_oauth(&self, _name: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn plugin() -> McpPlugin {
        McpPlugin::with_ops(
            McpManagerSeed::default(),
            vec![],
            Arc::new(StubOps {
                statuses: Mutex::new(vec![]),
            }),
        )
    }

    #[test]
    fn manifest_exposes_slash_and_screen() {
        let plugin = plugin();
        let manifest = plugin.manifest();
        assert_eq!(manifest.id.as_str(), "internal:mcp");
        assert_eq!(manifest.kind, PluginKind::Core);
        assert_eq!(manifest.contributions.slash_commands.len(), 1);
        assert_eq!(manifest.contributions.slash_commands[0].name, "mcp");
        assert_eq!(manifest.contributions.screens.len(), 1);
        assert_eq!(manifest.contributions.screens[0].id, SCREEN_ID);
    }

    #[tokio::test]
    async fn bare_slash_opens_screen() {
        let mut plugin = plugin();
        let effects = plugin.handle_slash("mcp", vec![]).await.unwrap();
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            Effect::OpenScreen { id, args } => {
                assert_eq!(id, SCREEN_ID);
                assert!(matches!(args, ScreenArgs::None));
            }
            other => panic!("expected OpenScreen, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn slash_rejects_args() {
        let mut plugin = plugin();
        let err = plugin
            .handle_slash("mcp", vec!["extra".into()])
            .await
            .unwrap_err();
        assert!(matches!(err, PluginError::InvalidArgs(_)));
    }

    #[test]
    fn create_screen_returns_manager_screen() {
        let plugin = plugin();
        let screen = plugin.create_screen(SCREEN_ID, ScreenArgs::None).unwrap();
        assert_eq!(screen.id(), SCREEN_ID);
    }
}

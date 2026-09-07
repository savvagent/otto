use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use otto_host::{ConnectState, ToolServerStatus};
use otto_plugin::{
    Effect, KeyCodePortable, KeyEventPortable, PluginError, Region, Screen, StyledLine, StyledSpan,
    TextMods, ThemeColor,
};

use super::McpManagerOps;
use crate::config_file::{McpAuthMode, McpServerEntry};
use crate::{McpManagerSeed, McpServerSummary};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DraftTransport {
    Stdio,
    Http,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddField {
    Name,
    Transport,
    Target,
    Args,
    SecretEnv,
    Secret,
}

#[derive(Debug, Clone)]
struct AddDraft {
    name: String,
    transport: DraftTransport,
    target: String,
    args: String,
    secret_env: String,
    secret: String,
}

impl Default for AddDraft {
    fn default() -> Self {
        Self {
            name: String::new(),
            transport: DraftTransport::Stdio,
            target: String::new(),
            args: String::new(),
            secret_env: String::new(),
            secret: String::new(),
        }
    }
}

impl AddDraft {
    fn visible_fields(&self) -> Vec<AddField> {
        match self.transport {
            DraftTransport::Stdio => vec![
                AddField::Name,
                AddField::Transport,
                AddField::Target,
                AddField::Args,
                AddField::SecretEnv,
                AddField::Secret,
            ],
            DraftTransport::Http => vec![
                AddField::Name,
                AddField::Transport,
                AddField::Target,
                AddField::Secret,
            ],
        }
    }

    fn build_entry(
        &self,
        existing_names: &HashSet<String>,
    ) -> Result<(McpServerEntry, Option<String>), String> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err("name is required".into());
        }

        let secret = if self.secret.is_empty() {
            None
        } else {
            Some(self.secret.clone())
        };

        let entry = match self.transport {
            DraftTransport::Stdio => {
                let command = self.target.trim().to_string();
                if command.is_empty() {
                    return Err("command is required for stdio servers".into());
                }
                let mut env = HashMap::new();
                if secret.is_some() {
                    let key = self.secret_env.trim().to_string();
                    if key.is_empty() {
                        return Err("secret env var is required when a secret is provided".into());
                    }
                    env.insert(key, "keyring".to_string());
                }
                McpServerEntry::Stdio {
                    name,
                    command,
                    args: split_args(self.args.trim()),
                    env,
                }
            }
            DraftTransport::Http => {
                let url = self.target.trim().to_string();
                if url.is_empty() {
                    return Err("url is required for http servers".into());
                }
                McpServerEntry::Http {
                    name,
                    url,
                    auth: if secret.is_some() {
                        McpAuthMode::Bearer
                    } else {
                        McpAuthMode::None
                    },
                }
            }
        };
        entry.validate(existing_names)?;
        Ok((entry, secret))
    }
}

#[derive(Debug, Clone)]
struct ServerRow {
    name: String,
    transport: &'static str,
    state_text: String,
    state_color: ThemeColor,
}

enum Mode {
    List,
    Adding { draft: AddDraft, field_index: usize },
}

/// Per-open MCP manager screen.
pub struct McpManagerScreen {
    configured: Vec<McpServerSummary>,
    skip_notes: HashMap<String, String>,
    statuses: Vec<ToolServerStatus>,
    rows: Vec<ServerRow>,
    cursor: usize,
    notes: Vec<String>,
    mode: Mode,
    ops: Arc<dyn McpManagerOps>,
}

impl McpManagerScreen {
    pub fn new(
        seed: McpManagerSeed,
        statuses: Vec<ToolServerStatus>,
        ops: Arc<dyn McpManagerOps>,
    ) -> Self {
        let mut screen = Self {
            configured: seed.configured,
            skip_notes: seed.skip_notes.into_iter().collect(),
            statuses,
            rows: vec![],
            cursor: 0,
            notes: vec![],
            mode: Mode::List,
            ops,
        };
        screen.rebuild_rows();
        screen
    }

    fn rebuild_rows(&mut self) {
        let status_by_name: HashMap<_, _> = self
            .statuses
            .iter()
            .map(|status| (status.name.as_str(), status))
            .collect();
        self.rows = self
            .configured
            .iter()
            .map(|configured| {
                build_row(
                    configured,
                    status_by_name.get(configured.name.as_str()).copied(),
                    self.skip_notes.get(&configured.name),
                )
            })
            .collect();
        if self.cursor >= self.rows.len() {
            self.cursor = self.rows.len().saturating_sub(1);
        }
    }

    fn current_name(&self) -> Option<&str> {
        self.rows.get(self.cursor).map(|row| row.name.as_str())
    }

    fn push_note_effect(text: impl Into<String>) -> Effect {
        Effect::PushNote {
            line: StyledLine::plain(text.into()),
        }
    }

    async fn submit_add(&mut self) -> Result<Vec<Effect>, PluginError> {
        let Mode::Adding { draft, .. } = &self.mode else {
            return Ok(vec![]);
        };
        let existing_names = self
            .configured
            .iter()
            .map(|summary| summary.name.clone())
            .collect::<HashSet<_>>();
        let (entry, secret) = match draft.build_entry(&existing_names) {
            Ok(built) => built,
            Err(err) => return Ok(vec![Self::push_note_effect(err)]),
        };
        let name = match &entry {
            McpServerEntry::Stdio { name, .. } | McpServerEntry::Http { name, .. } => name.clone(),
        };
        let transport = match &entry {
            McpServerEntry::Stdio { .. } => "stdio",
            McpServerEntry::Http { .. } => "http",
        };
        self.ops
            .write_server(&entry)
            .await
            .map_err(PluginError::Internal)?;
        let mut effects = vec![Self::push_note_effect("Restart otto to apply changes.")];
        if let Some(secret) = secret {
            if let Err(err) = self.ops.save_secret(&name, &secret).await {
                effects.push(Self::push_note_effect(format!(
                    "server added; credential could not be saved: {err}"
                )));
            }
        }
        self.configured.push(McpServerSummary {
            name: name.clone(),
            transport,
        });
        self.skip_notes
            .insert(name, "restart otto to apply changes".to_string());
        self.mode = Mode::List;
        self.rebuild_rows();
        Ok(effects)
    }

    async fn refresh_statuses(&mut self) -> Result<Vec<Effect>, PluginError> {
        self.statuses = self.ops.read_statuses().await;
        self.rebuild_rows();
        Ok(vec![])
    }

    async fn remove_current(&mut self) -> Result<Vec<Effect>, PluginError> {
        let Some(name) = self.current_name().map(str::to_string) else {
            return Ok(vec![]);
        };
        let removed = self
            .ops
            .remove_server(&name)
            .await
            .map_err(PluginError::Internal)?;
        if !removed {
            return Ok(vec![Self::push_note_effect(format!(
                "server `{name}` was not found in config.toml"
            ))]);
        }
        self.configured.retain(|summary| summary.name != name);
        self.skip_notes.remove(&name);
        self.statuses.retain(|status| status.name != name);
        self.rebuild_rows();
        let mut effects = vec![Self::push_note_effect("Restart otto to apply changes.")];
        if let Err(err) = self.ops.delete_secret(&name).await {
            effects.push(Self::push_note_effect(format!(
                "server removed; stale credential could not be deleted: {err}"
            )));
        }
        Ok(effects)
    }

    async fn on_add_key(&mut self, key: KeyEventPortable) -> Result<Vec<Effect>, PluginError> {
        let (field, fields_len) = match &self.mode {
            Mode::Adding { draft, field_index } => {
                let fields = draft.visible_fields();
                (
                    *fields.get(*field_index).unwrap_or(&AddField::Name),
                    fields.len(),
                )
            }
            Mode::List => return Ok(vec![]),
        };
        if key.code == KeyCodePortable::Esc {
            self.mode = Mode::List;
            return Ok(vec![]);
        }
        match key.code {
            KeyCodePortable::Up | KeyCodePortable::BackTab => {
                if let Mode::Adding { field_index, .. } = &mut self.mode {
                    *field_index = field_index.saturating_sub(1);
                }
                Ok(vec![])
            }
            KeyCodePortable::Down | KeyCodePortable::Tab => {
                if let Mode::Adding { field_index, .. } = &mut self.mode {
                    let max = fields_len.saturating_sub(1);
                    if *field_index < max {
                        *field_index += 1;
                    }
                }
                Ok(vec![])
            }
            KeyCodePortable::Left => {
                if let Mode::Adding { draft, field_index } = &mut self.mode {
                    if field == AddField::Transport {
                        draft.transport = DraftTransport::Stdio;
                        let max = draft.visible_fields().len().saturating_sub(1);
                        *field_index = (*field_index).min(max);
                    }
                }
                Ok(vec![])
            }
            KeyCodePortable::Right => {
                if let Mode::Adding { draft, field_index } = &mut self.mode {
                    if field == AddField::Transport {
                        draft.transport = DraftTransport::Http;
                        let max = draft.visible_fields().len().saturating_sub(1);
                        *field_index = (*field_index).min(max);
                    }
                }
                Ok(vec![])
            }
            KeyCodePortable::Char(' ') if field == AddField::Transport => {
                if let Mode::Adding { draft, field_index } = &mut self.mode {
                    draft.transport = toggle_transport(draft.transport);
                    let max = draft.visible_fields().len().saturating_sub(1);
                    *field_index = (*field_index).min(max);
                }
                Ok(vec![])
            }
            KeyCodePortable::Char('h') | KeyCodePortable::Char('H')
                if field == AddField::Transport =>
            {
                if let Mode::Adding { draft, .. } = &mut self.mode {
                    draft.transport = DraftTransport::Http;
                }
                Ok(vec![])
            }
            KeyCodePortable::Char('s') | KeyCodePortable::Char('S')
                if field == AddField::Transport =>
            {
                if let Mode::Adding { draft, .. } = &mut self.mode {
                    draft.transport = DraftTransport::Stdio;
                }
                Ok(vec![])
            }
            KeyCodePortable::Backspace => {
                if let Mode::Adding { draft, .. } = &mut self.mode {
                    if let Some(value) = field_value_mut(draft, field) {
                        value.pop();
                    }
                }
                Ok(vec![])
            }
            KeyCodePortable::Enter => {
                if let Mode::Adding { field_index, .. } = &mut self.mode {
                    if *field_index + 1 >= fields_len {
                        return self.submit_add().await;
                    }
                    *field_index += 1;
                }
                Ok(vec![])
            }
            KeyCodePortable::Char(c) => {
                if let Mode::Adding { draft, .. } = &mut self.mode {
                    if let Some(value) = field_value_mut(draft, field) {
                        value.push(c);
                    }
                }
                Ok(vec![])
            }
            _ => Ok(vec![]),
        }
    }
}

#[async_trait]
impl Screen for McpManagerScreen {
    fn id(&self) -> String {
        "mcp.manager".to_string()
    }

    fn render(&self, _region: Region) -> Vec<StyledLine> {
        match &self.mode {
            Mode::List => {
                let mut lines = vec![
                    StyledLine::plain("Configured MCP servers"),
                    StyledLine::plain(""),
                ];
                if self.rows.is_empty() {
                    lines.push(StyledLine::plain("No MCP servers configured."));
                } else {
                    for (index, row) in self.rows.iter().enumerate() {
                        let marker = if index == self.cursor { "▶ " } else { "  " };
                        lines.push(StyledLine {
                            spans: vec![
                                StyledSpan {
                                    text: format!("{marker}{:<18} {:<5} ", row.name, row.transport),
                                    fg: Some(if index == self.cursor {
                                        ThemeColor::Accent
                                    } else {
                                        ThemeColor::Fg
                                    }),
                                    bg: None,
                                    modifiers: TextMods {
                                        bold: index == self.cursor,
                                        ..Default::default()
                                    },
                                },
                                StyledSpan {
                                    text: row.state_text.clone(),
                                    fg: Some(row.state_color),
                                    bg: None,
                                    modifiers: TextMods::default(),
                                },
                            ],
                        });
                    }
                }
                if !self.notes.is_empty() {
                    lines.push(StyledLine::plain(""));
                    lines.push(StyledLine::plain("Session notes:"));
                    for note in &self.notes {
                        lines.push(StyledLine::plain(format!("- {note}")));
                    }
                }
                lines
            }
            Mode::Adding { draft, field_index } => {
                let fields = draft.visible_fields();
                let mut lines = vec![
                    StyledLine::plain("Add MCP server"),
                    StyledLine::plain(""),
                    StyledLine::plain("Enter confirms the current field. Esc cancels."),
                    StyledLine::plain(""),
                ];
                for (index, field) in fields.iter().enumerate() {
                    let marker = if index == *field_index { "▶ " } else { "  " };
                    lines.push(StyledLine::plain(format!(
                        "{marker}{}: {}",
                        field_label(*field),
                        field_display_value(draft, *field)
                    )));
                }
                lines
            }
        }
    }

    async fn on_key(&mut self, key: KeyEventPortable) -> Result<Vec<Effect>, PluginError> {
        match &self.mode {
            Mode::List => match key.code {
                KeyCodePortable::Esc => Ok(vec![Effect::CloseScreen]),
                KeyCodePortable::Up | KeyCodePortable::Char('k') => {
                    self.cursor = self.cursor.saturating_sub(1);
                    Ok(vec![])
                }
                KeyCodePortable::Down | KeyCodePortable::Char('j') => {
                    let max = self.rows.len().saturating_sub(1);
                    if self.cursor < max {
                        self.cursor += 1;
                    }
                    Ok(vec![])
                }
                KeyCodePortable::Char('a') => {
                    self.mode = Mode::Adding {
                        draft: AddDraft::default(),
                        field_index: 0,
                    };
                    Ok(vec![])
                }
                KeyCodePortable::Char('d') | KeyCodePortable::Delete => self.remove_current().await,
                KeyCodePortable::Char('r') => self.refresh_statuses().await,
                _ => Ok(vec![]),
            },
            Mode::Adding { .. } => self.on_add_key(key).await,
        }
    }

    fn tips(&self) -> Vec<StyledLine> {
        match &self.mode {
            Mode::List => vec![StyledLine::plain(
                "a add · d delete · r refresh status · Esc close · remote servers are not sandboxed"
                    .to_string(),
            )],
            Mode::Adding { .. } => vec![StyledLine::plain(
                "Tab move · Space toggle transport · secret input is masked".to_string(),
            )],
        }
    }
}

fn build_row(
    configured: &McpServerSummary,
    live_status: Option<&ToolServerStatus>,
    skip_note: Option<&String>,
) -> ServerRow {
    match live_status.map(|status| &status.state) {
        Some(ConnectState::Connected) => ServerRow {
            name: configured.name.clone(),
            transport: configured.transport,
            state_text: "connected".into(),
            state_color: ThemeColor::Success,
        },
        Some(ConnectState::Failed { reason }) => ServerRow {
            name: configured.name.clone(),
            transport: configured.transport,
            state_text: format!("failed: {reason}"),
            state_color: ThemeColor::Warning,
        },
        None => ServerRow {
            name: configured.name.clone(),
            transport: configured.transport,
            state_text: format!(
                "not started: {}",
                skip_note
                    .cloned()
                    .unwrap_or_else(|| "restart otto to apply changes".to_string())
            ),
            state_color: ThemeColor::Muted,
        },
    }
}

fn field_label(field: AddField) -> &'static str {
    match field {
        AddField::Name => "Name",
        AddField::Transport => "Transport",
        AddField::Target => "Command / URL",
        AddField::Args => "Args",
        AddField::SecretEnv => "Secret env var",
        AddField::Secret => "Secret",
    }
}

fn field_display_value(draft: &AddDraft, field: AddField) -> String {
    match field {
        AddField::Name => draft.name.clone(),
        AddField::Transport => match draft.transport {
            DraftTransport::Stdio => "stdio".into(),
            DraftTransport::Http => "http".into(),
        },
        AddField::Target => draft.target.clone(),
        AddField::Args => draft.args.clone(),
        AddField::SecretEnv => draft.secret_env.clone(),
        AddField::Secret => {
            if draft.secret.is_empty() {
                String::new()
            } else {
                "•".repeat(draft.secret.chars().count())
            }
        }
    }
}

fn field_value_mut(draft: &mut AddDraft, field: AddField) -> Option<&mut String> {
    match field {
        AddField::Name => Some(&mut draft.name),
        AddField::Target => Some(&mut draft.target),
        AddField::Args => Some(&mut draft.args),
        AddField::SecretEnv => Some(&mut draft.secret_env),
        AddField::Secret => Some(&mut draft.secret),
        AddField::Transport => None,
    }
}

fn toggle_transport(current: DraftTransport) -> DraftTransport {
    match current {
        DraftTransport::Stdio => DraftTransport::Http,
        DraftTransport::Http => DraftTransport::Stdio,
    }
}

fn split_args(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use otto_plugin::KeyMods;
    use std::sync::Mutex;

    #[derive(Clone)]
    enum StubOp {
        Write(String),
        SaveSecret(String, String),
        Remove(String),
        DeleteSecret(String),
    }

    struct StubOps {
        statuses: Mutex<Vec<ToolServerStatus>>,
        remove_result: Mutex<Result<bool, String>>,
        log: Mutex<Vec<StubOp>>,
    }

    impl StubOps {
        fn new(statuses: Vec<ToolServerStatus>) -> Self {
            Self {
                statuses: Mutex::new(statuses),
                remove_result: Mutex::new(Ok(true)),
                log: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait]
    impl McpManagerOps for StubOps {
        async fn read_statuses(&self) -> Vec<ToolServerStatus> {
            self.statuses.lock().unwrap().clone()
        }

        async fn write_server(&self, entry: &McpServerEntry) -> Result<(), String> {
            let name = match entry {
                McpServerEntry::Stdio { name, .. } | McpServerEntry::Http { name, .. } => name,
            };
            self.log.lock().unwrap().push(StubOp::Write(name.clone()));
            Ok(())
        }

        async fn remove_server(&self, name: &str) -> Result<bool, String> {
            self.log
                .lock()
                .unwrap()
                .push(StubOp::Remove(name.to_string()));
            self.remove_result.lock().unwrap().clone()
        }

        async fn save_secret(&self, name: &str, secret: &str) -> Result<(), String> {
            self.log
                .lock()
                .unwrap()
                .push(StubOp::SaveSecret(name.to_string(), secret.to_string()));
            Ok(())
        }

        async fn delete_secret(&self, name: &str) -> Result<(), String> {
            self.log
                .lock()
                .unwrap()
                .push(StubOp::DeleteSecret(name.to_string()));
            Err("keyring unavailable".into())
        }
    }

    fn key(code: KeyCodePortable) -> KeyEventPortable {
        KeyEventPortable {
            code,
            modifiers: KeyMods::default(),
        }
    }

    fn screen(ops: Arc<StubOps>) -> McpManagerScreen {
        McpManagerScreen::new(
            McpManagerSeed {
                configured: vec![McpServerSummary {
                    name: "remote".into(),
                    transport: "http",
                }],
                skip_notes: vec![("remote".into(), "missing keyring secret".into())],
            },
            vec![],
            ops,
        )
    }

    #[test]
    fn render_merges_configured_and_skip_notes() {
        let ops = Arc::new(StubOps::new(vec![]));
        let screen = screen(ops);
        let joined = screen
            .render(Region {
                x: 0,
                y: 0,
                width: 80,
                height: 20,
            })
            .into_iter()
            .flat_map(|line| line.spans.into_iter().map(|span| span.text))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("remote"));
        assert!(joined.contains("not started: missing keyring secret"));
    }

    #[tokio::test]
    async fn add_flow_writes_config_then_secret() {
        let ops = Arc::new(StubOps::new(vec![]));
        let mut screen = McpManagerScreen::new(McpManagerSeed::default(), vec![], ops.clone());
        screen
            .on_key(key(KeyCodePortable::Char('a')))
            .await
            .unwrap();
        for ch in "demo".chars() {
            screen.on_key(key(KeyCodePortable::Char(ch))).await.unwrap();
        }
        screen.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        screen.on_key(key(KeyCodePortable::Right)).await.unwrap();
        screen.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        for ch in "https://example.test/mcp".chars() {
            screen.on_key(key(KeyCodePortable::Char(ch))).await.unwrap();
        }
        screen.on_key(key(KeyCodePortable::Enter)).await.unwrap();
        for ch in "secret-token".chars() {
            screen.on_key(key(KeyCodePortable::Char(ch))).await.unwrap();
        }
        let effects = screen.on_key(key(KeyCodePortable::Enter)).await.unwrap();

        let log = ops.log.lock().unwrap().clone();
        assert!(matches!(log[0], StubOp::Write(ref name) if name == "demo"));
        assert!(
            matches!(log[1], StubOp::SaveSecret(ref name, ref secret) if name == "demo" && secret == "secret-token")
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::PushNote { .. }))
        );
    }

    #[tokio::test]
    async fn delete_flow_removes_config_before_secret_and_reports_stale_credential() {
        let ops = Arc::new(StubOps::new(vec![]));
        let mut screen = screen(ops.clone());
        let effects = screen
            .on_key(key(KeyCodePortable::Char('d')))
            .await
            .unwrap();

        let log = ops.log.lock().unwrap().clone();
        assert!(matches!(log[0], StubOp::Remove(ref name) if name == "remote"));
        assert!(matches!(log[1], StubOp::DeleteSecret(ref name) if name == "remote"));
        assert_eq!(screen.rows.len(), 0);
        assert_eq!(effects.len(), 2);
    }
}

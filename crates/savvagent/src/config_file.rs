//! ~/.savvagent/config.toml schema, load, save, and migration marker.
//! Single source of truth for non-routing knobs (startup connect policy,
//! per-provider connect timeout, migration_v1_done marker).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use savvagent_host::StartupConnectPolicy;
use savvagent_protocol::ProviderId;
use serde::{Deserialize, Serialize};

/// Typed version of the `startup.policy` config key. Serialises to/from
/// kebab-case strings (`"opt-in"`, `"all"`, `"none"`, `"last-used"`).
/// An unrecognised string in the TOML will fail to deserialise; the
/// `load_or_default` caller logs a warning and falls back to the default
/// rather than silently choosing `OptIn`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StartupPolicyKind {
    /// Only providers in `startup_providers` are auto-connected.
    #[default]
    OptIn,
    /// Every registered provider is auto-connected.
    All,
    /// No auto-connect; pool starts empty.
    None,
    /// Auto-connect the provider(s) from last session.
    LastUsed,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub startup: StartupSection,
    #[serde(default)]
    pub migration: MigrationSection,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "transport", rename_all = "lowercase")]
pub enum McpServerEntry {
    Stdio {
        name: String,
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    Http {
        name: String,
        url: String,
        #[serde(default)]
        auth: McpAuthMode,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum McpAuthMode {
    #[default]
    None,
    Bearer,
    Oauth,
}

#[derive(Debug, Clone, Default)]
pub struct LoadedConfig {
    pub config: ConfigFile,
    pub mcp_server_diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupSection {
    #[serde(default)]
    pub policy: StartupPolicyKind,
    #[serde(default)]
    pub startup_providers: Vec<String>,
    #[serde(default = "default_timeout")]
    pub connect_timeout_ms: u64,
}

impl Default for StartupSection {
    fn default() -> Self {
        Self {
            policy: StartupPolicyKind::default(),
            startup_providers: Vec::new(),
            connect_timeout_ms: default_timeout(),
        }
    }
}

fn default_timeout() -> u64 {
    3000
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MigrationSection {
    #[serde(default)]
    pub v1_done: bool,
}

impl ConfigFile {
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".savvagent")
            .join("config.toml")
    }

    /// Load from `path`, falling back to [`Self::default`] on file-not-found
    /// or parse error. Parse errors are logged at `warn` level.
    pub fn load_or_default(path: &Path) -> LoadedConfig {
        let Ok(contents) = std::fs::read_to_string(path) else {
            return LoadedConfig::default();
        };
        let value = match contents.parse::<toml::Value>() {
            Ok(value) => value,
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "config.toml parse failed; falling back to defaults"
                );
                return LoadedConfig::default();
            }
        };

        #[derive(Debug, Default, Deserialize)]
        struct BaseConfig {
            #[serde(default)]
            startup: StartupSection,
            #[serde(default)]
            migration: MigrationSection,
        }

        let base = match BaseConfig::deserialize(value.clone()) {
            Ok(base) => base,
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "config.toml parse failed; falling back to defaults"
                );
                return LoadedConfig::default();
            }
        };

        let mut config = ConfigFile {
            startup: base.startup,
            migration: base.migration,
            mcp_servers: Vec::new(),
        };
        let mut diagnostics = Vec::new();

        if let Some(entries) = value.get("mcp_servers").and_then(toml::Value::as_array) {
            for (idx, entry) in entries.iter().cloned().enumerate() {
                match McpServerEntry::deserialize(entry.clone()) {
                    Ok(server) => config.mcp_servers.push(server),
                    Err(err) => {
                        diagnostics.push(format!("{}: {err}", mcp_server_entry_label(idx, &entry)))
                    }
                }
            }
        }

        LoadedConfig {
            config,
            mcp_server_diagnostics: diagnostics,
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, text)
    }

    pub fn to_startup_policy(&self) -> StartupConnectPolicy {
        let ids: Vec<ProviderId> = self
            .startup
            .startup_providers
            .iter()
            .filter_map(|s| ProviderId::new(s).ok())
            .collect();
        match self.startup.policy {
            StartupPolicyKind::All => StartupConnectPolicy::All,
            StartupPolicyKind::None => StartupConnectPolicy::None,
            StartupPolicyKind::LastUsed => StartupConnectPolicy::LastUsed(ids),
            StartupPolicyKind::OptIn => StartupConnectPolicy::OptIn(ids),
        }
    }
}

impl McpServerEntry {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn name(&self) -> &str {
        match self {
            Self::Stdio { name, .. } | Self::Http { name, .. } => name,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn validate(&self, seen_names: &HashSet<String>) -> Result<(), String> {
        let name = self.name();
        if name.is_empty() {
            return Err("mcp server name must not be empty".into());
        }
        if name.contains(':') {
            return Err(format!(
                "mcp server name `{name}` must not contain `:` because it is reserved for keyring namespaces"
            ));
        }
        if seen_names.contains(name) {
            return Err(format!("duplicate mcp server name `{name}`"));
        }
        match self {
            Self::Stdio { env, .. } => {
                let mut keyring_vars = env
                    .iter()
                    .filter_map(|(key, value)| (value == "keyring").then_some(key.as_str()));
                if let (Some(first), Some(second)) = (keyring_vars.next(), keyring_vars.next()) {
                    return Err(format!(
                        "only one keyring-backed env value is supported per server in this version; found `{first}` and `{second}`"
                    ));
                }
            }
            Self::Http {
                auth: McpAuthMode::Oauth,
                ..
            } => {
                return Err(
                    "oauth is not yet supported; use auth = \"bearer\" or omit auth".into(),
                );
            }
            Self::Http { .. } => {}
        }
        Ok(())
    }
}

fn mcp_server_entry_label(idx: usize, entry: &toml::Value) -> String {
    let prefix = format!("mcp_servers[{idx}]");
    match entry.get("name").and_then(toml::Value::as_str) {
        Some(name) => format!("{prefix} (name = \"{name}\")"),
        None => prefix,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn missing_file_returns_default() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let cfg = ConfigFile::load_or_default(&path).config;
        assert_eq!(cfg.startup.policy, StartupPolicyKind::OptIn);
        assert!(cfg.startup.startup_providers.is_empty());
        assert!(!cfg.migration.v1_done);
    }

    #[test]
    fn round_trip_preserves_fields() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let mut cfg = ConfigFile::default();
        cfg.startup.policy = StartupPolicyKind::OptIn;
        cfg.startup.startup_providers = vec!["anthropic".into(), "gemini".into()];
        cfg.startup.connect_timeout_ms = 4000;
        cfg.migration.v1_done = true;
        cfg.save(&path).unwrap();

        let loaded = ConfigFile::load_or_default(&path).config;
        assert_eq!(loaded.startup.policy, StartupPolicyKind::OptIn);
        assert_eq!(
            loaded.startup.startup_providers,
            vec!["anthropic", "gemini"]
        );
        assert_eq!(loaded.startup.connect_timeout_ms, 4000);
        assert!(loaded.migration.v1_done);
    }

    #[test]
    fn policy_string_maps_correctly() {
        let mut cfg = ConfigFile::default();
        cfg.startup.policy = StartupPolicyKind::All;
        assert!(matches!(cfg.to_startup_policy(), StartupConnectPolicy::All));
        cfg.startup.policy = StartupPolicyKind::None;
        assert!(matches!(
            cfg.to_startup_policy(),
            StartupConnectPolicy::None
        ));
        cfg.startup.policy = StartupPolicyKind::OptIn;
        cfg.startup.startup_providers = vec!["anthropic".into()];
        match cfg.to_startup_policy() {
            StartupConnectPolicy::OptIn(ids) => assert_eq!(ids.len(), 1),
            _ => panic!(),
        }
    }

    #[test]
    fn invalid_policy_string_falls_back_to_default() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            "[startup]\npolicy = \"invalid-typo\"\nconnect_timeout_ms = 5000\n",
        )
        .unwrap();
        let cfg = ConfigFile::load_or_default(&path).config;
        // Falls back entirely to default on parse error.
        assert_eq!(cfg.startup.policy, StartupPolicyKind::OptIn);
        assert_eq!(cfg.startup.connect_timeout_ms, default_timeout());
    }

    #[test]
    fn malformed_mcp_server_row_is_skipped_without_losing_other_config() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[startup]
policy = "all"
connect_timeout_ms = 4321

[migration]
v1_done = true

[[mcp_servers]]
name = "good"
transport = "stdio"
command = "/bin/echo"
args = ["hello"]

[[mcp_servers]]
name = "bad"
transport = "bogus"
"#,
        )
        .unwrap();

        let loaded = ConfigFile::load_or_default(&path);
        assert_eq!(loaded.config.startup.policy, StartupPolicyKind::All);
        assert_eq!(loaded.config.startup.connect_timeout_ms, 4321);
        assert!(loaded.config.migration.v1_done);
        assert_eq!(loaded.config.mcp_servers.len(), 1);
        assert_eq!(loaded.mcp_server_diagnostics.len(), 1);
        assert!(loaded.mcp_server_diagnostics[0].contains("mcp_servers[1]"));
        assert!(loaded.mcp_server_diagnostics[0].contains("bad"));
    }

    #[test]
    fn oauth_is_tolerantly_loaded_but_rejected_by_validate() {
        let entry: McpServerEntry = toml::from_str(
            r#"
transport = "http"
name = "remote"
url = "https://example.test/mcp"
auth = "oauth"
"#,
        )
        .unwrap();
        let err = entry.validate(&HashSet::new()).unwrap_err();
        assert!(err.contains("not yet supported"));
    }

    #[test]
    fn validate_rejects_duplicate_names() {
        let entry = McpServerEntry::Stdio {
            name: "dupe".into(),
            command: "echo".into(),
            args: Vec::new(),
            env: HashMap::new(),
        };
        let seen = HashSet::from(["dupe".to_string()]);
        let err = entry.validate(&seen).unwrap_err();
        assert!(err.contains("duplicate"));
        assert!(err.contains("dupe"));
    }

    #[test]
    fn validate_rejects_multiple_keyring_env_vars() {
        let entry = McpServerEntry::Stdio {
            name: "server".into(),
            command: "echo".into(),
            args: Vec::new(),
            env: HashMap::from([
                ("FIRST".into(), "keyring".into()),
                ("SECOND".into(), "keyring".into()),
            ]),
        };
        let err = entry.validate(&HashSet::new()).unwrap_err();
        assert!(err.contains("FIRST"));
        assert!(err.contains("SECOND"));
    }
}

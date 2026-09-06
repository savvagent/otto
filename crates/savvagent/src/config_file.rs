//! ~/.savvagent/config.toml schema, load, save, and migration marker.
//! Single source of truth for non-routing knobs (startup connect policy,
//! per-provider connect timeout, migration_v1_done marker).

use std::path::{Path, PathBuf};

use crate::plugin::builtin::{
    language::catalog::is_supported as is_supported_language, themes::catalog::Theme,
};
use savvagent_host::StartupConnectPolicy;
use savvagent_protocol::ProviderId;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use toml::Table;

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
    pub language: LanguageSection,
    #[serde(default)]
    pub theme: ThemeSection,
    #[serde(default)]
    pub update: UpdateSection,
    #[serde(default)]
    pub migration: MigrationSection,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageSection {
    #[serde(
        default = "default_language_code",
        deserialize_with = "deserialize_language_code"
    )]
    pub code: String,
}

impl Default for LanguageSection {
    fn default() -> Self {
        Self {
            code: default_language_code(),
        }
    }
}

fn default_language_code() -> String {
    "en".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeSection {
    #[serde(
        default = "default_theme_name",
        deserialize_with = "deserialize_theme_name"
    )]
    pub name: String,
}

impl Default for ThemeSection {
    fn default() -> Self {
        Self {
            name: default_theme_name(),
        }
    }
}

fn default_theme_name() -> String {
    "dark".to_string()
}

pub(crate) fn deserialize_language_code<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let code = String::deserialize(deserializer)?;
    if is_supported_language(&code) {
        Ok(code)
    } else {
        Err(serde::de::Error::custom(format!(
            "unsupported language `{code}`"
        )))
    }
}

fn deserialize_theme_name<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let name = String::deserialize(deserializer)?;
    if Theme::from_name(&name).is_some() {
        Ok(name)
    } else {
        Err(serde::de::Error::custom(format!("unknown theme `{name}`")))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSection {
    #[serde(default = "default_update_periodic_interval_secs")]
    pub periodic_interval_secs: u64,
    #[serde(default)]
    pub disabled: bool,
}

impl Default for UpdateSection {
    fn default() -> Self {
        Self {
            periodic_interval_secs: default_update_periodic_interval_secs(),
            disabled: false,
        }
    }
}

impl UpdateSection {
    pub fn effective_periodic_interval_secs(&self) -> u64 {
        match self.periodic_interval_secs {
            0 => default_update_periodic_interval_secs(),
            interval => interval,
        }
    }
}

fn default_update_periodic_interval_secs() -> u64 {
    300
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
    pub fn load_or_default(path: &Path) -> Self {
        let Ok(contents) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        Self::load_from_toml_str(path, &contents)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        write_atomic(path, &text)
    }

    pub fn save_language_section(path: &Path, language: LanguageSection) -> std::io::Result<()> {
        save_section(path, "language", &language)
    }

    pub fn save_theme_section(path: &Path, theme: ThemeSection) -> std::io::Result<()> {
        save_section(path, "theme", &theme)
    }

    pub fn save_update_section(path: &Path, update: UpdateSection) -> std::io::Result<()> {
        save_section(path, "update", &update)
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

    pub fn effective_update_periodic_interval_secs(&self) -> u64 {
        self.update.effective_periodic_interval_secs()
    }

    fn load_from_toml_str(path: &Path, contents: &str) -> Self {
        let root = match toml::from_str::<toml::Table>(contents) {
            Ok(root) => root,
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "config.toml parse failed; falling back to defaults"
                );
                return Self::default();
            }
        };

        Self {
            startup: parse_section(path, &root, "startup"),
            language: parse_section(path, &root, "language"),
            theme: parse_section(path, &root, "theme"),
            update: parse_section(path, &root, "update"),
            migration: parse_section(path, &root, "migration"),
        }
    }
}

fn parse_section<T>(path: &Path, root: &toml::Table, section: &'static str) -> T
where
    T: DeserializeOwned + Default,
{
    let Some(value) = root.get(section) else {
        return T::default();
    };

    match value.clone().try_into() {
        Ok(parsed) => parsed,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                section,
                error = %error,
                "config.toml section parse failed; falling back to section defaults"
            );
            T::default()
        }
    }
}

fn load_root_for_save(path: &Path) -> std::io::Result<Table> {
    match std::fs::read_to_string(path) {
        Ok(contents) => toml::from_str::<Table>(&contents)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Table::new()),
        Err(error) => Err(error),
    }
}

fn save_section<T>(path: &Path, section: &'static str, value: &T) -> std::io::Result<()>
where
    T: Serialize,
{
    let mut root = load_root_for_save(path)?;
    let value = toml::Value::try_from(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    root.insert(section.to_string(), value);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(&root)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    write_atomic(path, &text)
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&tmp);
            Err(error)
        }
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
        let cfg = ConfigFile::load_or_default(&path);
        assert_eq!(cfg.startup.policy, StartupPolicyKind::OptIn);
        assert!(cfg.startup.startup_providers.is_empty());
        assert_eq!(cfg.language.code, "en");
        assert_eq!(cfg.theme.name, "dark");
        assert_eq!(cfg.update.periodic_interval_secs, 300);
        assert_eq!(cfg.update.effective_periodic_interval_secs(), 300);
        assert!(!cfg.update.disabled);
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
        cfg.language.code = "es".into();
        cfg.theme.name = "light".into();
        cfg.update.periodic_interval_secs = 900;
        cfg.update.disabled = true;
        cfg.migration.v1_done = true;
        cfg.save(&path).unwrap();

        let loaded = ConfigFile::load_or_default(&path);
        assert_eq!(loaded.startup.policy, StartupPolicyKind::OptIn);
        assert_eq!(
            loaded.startup.startup_providers,
            vec!["anthropic", "gemini"]
        );
        assert_eq!(loaded.startup.connect_timeout_ms, 4000);
        assert_eq!(loaded.language.code, "es");
        assert_eq!(loaded.theme.name, "light");
        assert_eq!(loaded.update.periodic_interval_secs, 900);
        assert_eq!(loaded.update.effective_periodic_interval_secs(), 900);
        assert!(loaded.update.disabled);
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
        let cfg = ConfigFile::load_or_default(&path);
        // Falls back entirely to default on parse error.
        assert_eq!(cfg.startup.policy, StartupPolicyKind::OptIn);
        assert_eq!(cfg.startup.connect_timeout_ms, default_timeout());
    }

    #[test]
    fn save_theme_section_preserves_existing_startup_and_migration_fields() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");

        let mut cfg = ConfigFile::default();
        cfg.startup.policy = StartupPolicyKind::LastUsed;
        cfg.startup.startup_providers = vec!["anthropic".into()];
        cfg.startup.connect_timeout_ms = 4321;
        cfg.migration.v1_done = true;
        cfg.language.code = "pt".into();
        cfg.theme.name = "dark".into();
        cfg.update.periodic_interval_secs = 600;
        cfg.save(&path).unwrap();

        ConfigFile::save_theme_section(
            &path,
            ThemeSection {
                name: "light".into(),
            },
        )
        .unwrap();

        let loaded = ConfigFile::load_or_default(&path);
        assert_eq!(loaded.startup.policy, StartupPolicyKind::LastUsed);
        assert_eq!(loaded.startup.startup_providers, vec!["anthropic"]);
        assert_eq!(loaded.startup.connect_timeout_ms, 4321);
        assert!(loaded.migration.v1_done);
        assert_eq!(loaded.language.code, "pt");
        assert_eq!(loaded.theme.name, "light");
        assert_eq!(loaded.update.periodic_interval_secs, 600);
    }

    #[test]
    fn save_theme_section_does_not_materialize_default_language_section() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[startup]
policy = "last-used"
startup_providers = ["anthropic"]
connect_timeout_ms = 5000
"#,
        )
        .unwrap();

        ConfigFile::save_theme_section(
            &path,
            ThemeSection {
                name: "light".into(),
            },
        )
        .unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[theme]"), "saved config: {text}");
        assert!(
            !text.contains("[language]"),
            "saving theme alone must not pin locale detection: {text}"
        );
    }

    #[test]
    fn save_theme_section_leaves_malformed_config_untouched() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let malformed = "[startup\npolicy = \"all\"\n";
        std::fs::write(&path, malformed).unwrap();

        let error = ConfigFile::save_theme_section(
            &path,
            ThemeSection {
                name: "light".into(),
            },
        )
        .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), malformed);
    }

    #[test]
    fn malformed_theme_entry_preserves_other_sections() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[startup]
policy = "last-used"
startup_providers = ["anthropic"]
connect_timeout_ms = 5000

[language]
code = "es"

[theme]
name = 42

[update]
periodic_interval_secs = 900
disabled = true

[migration]
v1_done = true
"#,
        )
        .unwrap();

        let cfg = ConfigFile::load_or_default(&path);

        assert_eq!(cfg.startup.policy, StartupPolicyKind::LastUsed);
        assert_eq!(cfg.startup.startup_providers, vec!["anthropic"]);
        assert_eq!(cfg.startup.connect_timeout_ms, 5000);
        assert_eq!(cfg.language.code, "es");
        assert_eq!(cfg.theme.name, "dark");
        assert_eq!(cfg.update.periodic_interval_secs, 900);
        assert!(cfg.update.disabled);
        assert!(cfg.migration.v1_done);
    }

    #[test]
    fn unsupported_language_entry_preserves_other_sections() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[startup]
policy = "last-used"
startup_providers = ["anthropic"]
connect_timeout_ms = 5000

[language]
code = "klingon"

[theme]
name = "light"

[update]
periodic_interval_secs = 900
disabled = true

[migration]
v1_done = true
"#,
        )
        .unwrap();

        let cfg = ConfigFile::load_or_default(&path);

        assert_eq!(cfg.startup.policy, StartupPolicyKind::LastUsed);
        assert_eq!(cfg.startup.startup_providers, vec!["anthropic"]);
        assert_eq!(cfg.startup.connect_timeout_ms, 5000);
        assert_eq!(cfg.language.code, "en");
        assert_eq!(cfg.theme.name, "light");
        assert_eq!(cfg.update.periodic_interval_secs, 900);
        assert!(cfg.update.disabled);
        assert!(cfg.migration.v1_done);
    }

    #[test]
    fn unknown_theme_entry_preserves_other_sections() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[startup]
policy = "last-used"
startup_providers = ["anthropic"]
connect_timeout_ms = 5000

[language]
code = "es"

[theme]
name = "not-a-real-theme"

[update]
periodic_interval_secs = 900
disabled = true

[migration]
v1_done = true
"#,
        )
        .unwrap();

        let cfg = ConfigFile::load_or_default(&path);

        assert_eq!(cfg.startup.policy, StartupPolicyKind::LastUsed);
        assert_eq!(cfg.startup.startup_providers, vec!["anthropic"]);
        assert_eq!(cfg.startup.connect_timeout_ms, 5000);
        assert_eq!(cfg.language.code, "es");
        assert_eq!(cfg.theme.name, "dark");
        assert_eq!(cfg.update.periodic_interval_secs, 900);
        assert!(cfg.update.disabled);
        assert!(cfg.migration.v1_done);
    }

    #[test]
    fn zero_update_interval_falls_back_to_default_effective_interval() {
        let mut cfg = ConfigFile::default();
        cfg.update.periodic_interval_secs = 0;

        assert_eq!(cfg.update.effective_periodic_interval_secs(), 300);
        assert_eq!(cfg.effective_update_periodic_interval_secs(), 300);
    }

    #[test]
    fn save_language_and_update_sections_round_trip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");

        ConfigFile::save_language_section(&path, LanguageSection { code: "hi".into() }).unwrap();
        ConfigFile::save_update_section(
            &path,
            UpdateSection {
                periodic_interval_secs: 1200,
                disabled: true,
            },
        )
        .unwrap();

        let loaded = ConfigFile::load_or_default(&path);
        assert_eq!(loaded.language.code, "hi");
        assert_eq!(loaded.update.periodic_interval_secs, 1200);
        assert_eq!(loaded.effective_update_periodic_interval_secs(), 1200);
        assert!(loaded.update.disabled);
    }

    #[test]
    fn save_language_section_preserves_unrelated_invalid_sections() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[theme]
name = "not-a-real-theme"
"#,
        )
        .unwrap();

        ConfigFile::save_language_section(&path, LanguageSection { code: "hi".into() }).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains(r#"name = "not-a-real-theme""#),
            "unrelated sections should be preserved verbatim: {text}"
        );
        assert!(
            text.contains(r#"code = "hi""#),
            "language save should still update its own section: {text}"
        );
    }

    #[test]
    fn save_cleans_up_atomic_tmp_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");

        let mut cfg = ConfigFile::default();
        cfg.theme.name = "light".into();
        cfg.save(&path).unwrap();

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            toml::to_string_pretty(&cfg).unwrap()
        );
        assert!(
            !path.with_extension("toml.tmp").exists(),
            "atomic save should rename away its sibling tmp file"
        );
    }

    #[test]
    fn save_section_cleans_up_atomic_tmp_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");

        ConfigFile::save_theme_section(
            &path,
            ThemeSection {
                name: "light".into(),
            },
        )
        .unwrap();

        assert!(
            !path.with_extension("toml.tmp").exists(),
            "atomic section save should rename away its sibling tmp file"
        );
    }
}

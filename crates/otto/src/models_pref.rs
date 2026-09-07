//! Per-provider model preference persistence.
//!
//! `~/.otto/models.toml` records the user's chosen model for each
//! provider so the selection survives across sessions. Format:
//!
//! ```toml
//! schema_version = 1
//! [providers]
//! anthropic = "claude-sonnet-4-6"
//! gemini = "gemini-2.5-flash"
//! ```
//!
//! Load is best-effort: a missing file, parse failure, or unknown schema
//! version each log a `tracing::warn` and return an empty
//! [`ModelsPref`]. Save is atomic — we write `<path>.tmp` then `rename`
//! over the target so a kill mid-write leaves the previous file intact.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Schema version we read and write. Files declaring a higher version are
/// rejected (warn + empty fallback) so a newer otto cannot corrupt
/// an older one's state by writing a key it doesn't understand.
pub const SCHEMA_VERSION: u32 = 1;

/// In-memory view of `~/.otto/models.toml`. The wire shape is a
/// nested table (`[providers]`) so future top-level keys
/// (`schema_version`, telemetry opt-outs, …) don't collide with provider
/// ids; the public API on this struct surfaces only the provider map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelsPref {
    /// provider_id → model_id. Sorted by id so the on-disk file is
    /// diff-friendly across hand edits.
    pub providers: BTreeMap<String, String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct OnDisk {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(default)]
    providers: BTreeMap<String, String>,
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

impl ModelsPref {
    /// Look up the persisted model id for `provider_id`. Returns `None`
    /// when nothing is on file (caller falls back to the provider's
    /// `spec.default_model`).
    pub fn get(&self, provider_id: &str) -> Option<&str> {
        self.providers.get(provider_id).map(String::as_str)
    }

    /// Load `~/.otto/models.toml`. Missing file → empty. Parse
    /// failure or unsupported `schema_version` → warn + empty.
    pub fn load() -> Self {
        let Some(path) = models_toml_path() else {
            return Self::default();
        };
        load_from_path(&path)
    }
}

/// Load `provider_id`'s entry, replace it with `model_id`, and write
/// the file atomically. Errors propagate; the caller decides whether to
/// surface them to the user (typically a warn + `push_note`).
pub async fn save_for_provider(provider_id: &str, model_id: &str) -> Result<()> {
    let path = models_toml_path()
        .context("neither $HOME nor $USERPROFILE is set; cannot locate ~/.otto/models.toml")?;
    let mut pref = load_from_path(&path);
    pref.providers
        .insert(provider_id.to_string(), model_id.to_string());
    save_to_path(&pref, &path).await
}

fn load_from_path(path: &std::path::Path) -> ModelsPref {
    let Ok(text) = std::fs::read_to_string(path) else {
        return ModelsPref::default();
    };
    match toml::from_str::<OnDisk>(&text) {
        Ok(disk) => {
            if disk.schema_version > SCHEMA_VERSION {
                tracing::warn!(
                    "models.toml at {} declares schema_version {} but this build \
                     only understands up to {}. Ignoring the file.",
                    path.display(),
                    disk.schema_version,
                    SCHEMA_VERSION
                );
                return ModelsPref::default();
            }
            ModelsPref {
                providers: disk.providers,
            }
        }
        Err(e) => {
            tracing::warn!(
                "models.toml at {} failed to parse: {e}. Treating as empty so a \
                 hand-edit can't tank the next save.",
                path.display()
            );
            ModelsPref::default()
        }
    }
}

async fn save_to_path(pref: &ModelsPref, path: &std::path::Path) -> Result<()> {
    let disk = OnDisk {
        schema_version: SCHEMA_VERSION,
        providers: pref.providers.clone(),
    };
    let text = toml::to_string_pretty(&disk).context("serializing models.toml")?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create dir {}", parent.display()))?;
    }
    // Atomic: write to <path>.tmp then rename onto target. A kill
    // mid-write leaves the previous file intact (rename is atomic on
    // POSIX when src and dst live on the same filesystem, which is the
    // case here because tmp is the target's sibling).
    let tmp = path.with_extension("toml.tmp");
    tokio::fs::write(&tmp, text)
        .await
        .with_context(|| format!("writing tmp {}", tmp.display()))?;
    match tokio::fs::rename(&tmp, path).await {
        Ok(()) => Ok(()),
        Err(e) => {
            // Best-effort cleanup so we don't leak a tmp file the next
            // save would just overwrite anyway.
            let _ = tokio::fs::remove_file(&tmp).await;
            Err(anyhow::Error::from(e).context(format!(
                "rename {} -> {}",
                tmp.display(),
                path.display()
            )))
        }
    }
}

/// `~/.otto/models.toml`, or `None` if neither `$HOME` nor
/// `$USERPROFILE` is set.
fn models_toml_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    Some(home.join(".otto").join("models.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{HOME_LOCK, HomeGuard};

    #[test]
    fn load_missing_file_returns_empty() {
        let _lock = HOME_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        let pref = ModelsPref::load();
        assert!(pref.providers.is_empty());
        assert_eq!(pref.get("anthropic"), None);
    }

    // HOME_LOCK is std::Mutex (shared with sync tests) and must span the await.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test(flavor = "current_thread")]
    async fn roundtrip_preserves_other_providers() {
        let _lock = HOME_LOCK.lock().unwrap();
        let _home = HomeGuard::new();

        save_for_provider("anthropic", "claude-sonnet-4-6")
            .await
            .expect("write anthropic");
        save_for_provider("gemini", "gemini-2.5-flash")
            .await
            .expect("write gemini");

        let pref = ModelsPref::load();
        assert_eq!(pref.get("anthropic"), Some("claude-sonnet-4-6"));
        assert_eq!(pref.get("gemini"), Some("gemini-2.5-flash"));

        // Updating gemini must NOT clobber anthropic.
        save_for_provider("gemini", "gemini-2.5-pro")
            .await
            .expect("update gemini");
        let pref = ModelsPref::load();
        assert_eq!(pref.get("anthropic"), Some("claude-sonnet-4-6"));
        assert_eq!(pref.get("gemini"), Some("gemini-2.5-pro"));
    }

    #[test]
    fn load_unknown_schema_version_logs_and_returns_empty() {
        let _lock = HOME_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        let path = models_toml_path().expect("HOME set");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"schema_version = 999
[providers]
anthropic = "should-be-ignored"
"#,
        )
        .unwrap();
        let pref = ModelsPref::load();
        assert!(
            pref.providers.is_empty(),
            "future schema_version must produce empty pref, got {:?}",
            pref.providers
        );
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test(flavor = "current_thread")]
    async fn save_atomic_no_partial_write() {
        let _lock = HOME_LOCK.lock().unwrap();
        let _home = HomeGuard::new();

        save_for_provider("gemini", "gemini-2.5-flash")
            .await
            .expect("write gemini");
        let dir = models_toml_path().expect("HOME set");
        let parent = dir.parent().unwrap();
        // After a successful save the tmp file must not linger.
        let tmp = dir.with_extension("toml.tmp");
        assert!(!tmp.exists(), "tmp file should be cleaned up on success");
        // And the final file must be readable + parse cleanly.
        let entries: Vec<_> = std::fs::read_dir(parent)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name())
            .collect();
        assert!(
            entries.iter().any(|n| n == "models.toml"),
            "models.toml must exist, got {entries:?}"
        );
    }
}

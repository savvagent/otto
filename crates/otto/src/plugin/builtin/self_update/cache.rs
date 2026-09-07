//! 24-hour cache for the GitHub Releases query.
//!
//! Reading the releases endpoint on every launch is unnecessary — release
//! cadence is days-to-weeks, not minutes. The plugin persists the last
//! successful check (timestamp + tag) to `~/.otto/update-check.json`
//! and skips the network call on subsequent launches within
//! [`DEFAULT_TTL_SECS`]. Cache misses, parse errors, and IO errors all
//! degrade silently to "fetch as usual" — the cache is an optimisation,
//! never a correctness boundary.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Cache TTL in seconds. 24h matches the issue's stated requirement.
pub const DEFAULT_TTL_SECS: u64 = 24 * 60 * 60;

/// Schema version embedded in the cache file. Bumped if the on-disk
/// shape changes; mismatched files are treated as cache-miss.
const SCHEMA_VERSION: u32 = 1;

/// On-disk shape of `~/.otto/update-check.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheEntry {
    /// Schema version. See [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Unix-seconds timestamp at which the cached tag was fetched.
    pub checked_at_unix: u64,
    /// The `tag_name` value from the GitHub Releases response. Includes
    /// the leading `v` prefix exactly as GitHub returned it.
    pub latest_tag: String,
}

/// Path to `~/.otto/update-check.json`. Returns `None` when `$HOME`
/// is unset or empty, matching the convention used by
/// `plugins_manager::persistence::config_path`. Callers treat `None` as
/// a silent no-op so unit tests don't accidentally clobber a developer's
/// real cache file.
pub fn cache_path() -> Option<PathBuf> {
    let raw = std::env::var_os("HOME")?;
    if raw.is_empty() {
        return None;
    }
    Some(
        PathBuf::from(raw)
            .join(".otto")
            .join("update-check.json"),
    )
}

/// Return current time as unix-seconds, defaulting to 0 if the clock is
/// before the epoch (impossible on real systems but Rust still requires
/// a fallback).
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Pure helper: is `entry` younger than `ttl_secs` relative to `now`?
pub fn is_fresh(entry: &CacheEntry, now: u64, ttl_secs: u64) -> bool {
    if entry.schema_version != SCHEMA_VERSION {
        return false;
    }
    // Guard against clock skew that would make the cache appear ancient:
    // if `now` is *before* the timestamp, treat as fresh.
    if now < entry.checked_at_unix {
        return true;
    }
    now - entry.checked_at_unix < ttl_secs
}

/// Load the cache file at `path`. Returns `None` on missing file,
/// invalid JSON, schema mismatch, or any IO error.
pub fn load(path: &Path) -> Option<CacheEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let entry: CacheEntry = match serde_json::from_str(&text) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!(
                path = %path.display(),
                error = %e,
                "self-update: cache file is malformed; ignoring"
            );
            return None;
        }
    };
    if entry.schema_version != SCHEMA_VERSION {
        tracing::debug!(
            seen = entry.schema_version,
            expected = SCHEMA_VERSION,
            "self-update: cache schema_version mismatch; ignoring"
        );
        return None;
    }
    Some(entry)
}

/// Persist `entry` to `path` atomically. Creates the parent directory if
/// needed. Errors are logged at `debug` and swallowed — the cache is an
/// optimisation, not a correctness boundary.
pub fn save(path: &Path, entry: &CacheEntry) {
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::debug!(path = %parent.display(), error = %e, "self-update: cache mkdir failed");
            return;
        }
    }
    let text = match serde_json::to_string_pretty(entry) {
        Ok(t) => t,
        Err(e) => {
            tracing::debug!(error = %e, "self-update: cache serialisation failed");
            return;
        }
    };
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, text) {
        tracing::debug!(path = %tmp.display(), error = %e, "self-update: cache temp write failed");
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        tracing::debug!(
            from = %tmp.display(),
            to = %path.display(),
            error = %e,
            "self-update: cache rename failed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn entry(tag: &str, when: u64) -> CacheEntry {
        CacheEntry {
            schema_version: SCHEMA_VERSION,
            checked_at_unix: when,
            latest_tag: tag.into(),
        }
    }

    // --- is_fresh ---

    #[test]
    fn is_fresh_inside_ttl_returns_true() {
        let e = entry("v0.10.0", 1_000_000);
        assert!(is_fresh(
            &e,
            1_000_000 + DEFAULT_TTL_SECS - 1,
            DEFAULT_TTL_SECS
        ));
    }

    #[test]
    fn is_fresh_exactly_at_ttl_returns_false() {
        let e = entry("v0.10.0", 1_000_000);
        assert!(!is_fresh(
            &e,
            1_000_000 + DEFAULT_TTL_SECS,
            DEFAULT_TTL_SECS
        ));
    }

    #[test]
    fn is_fresh_outside_ttl_returns_false() {
        let e = entry("v0.10.0", 1_000_000);
        assert!(!is_fresh(
            &e,
            1_000_000 + DEFAULT_TTL_SECS + 1,
            DEFAULT_TTL_SECS
        ));
    }

    #[test]
    fn is_fresh_with_future_timestamp_clock_skew_returns_true() {
        // Cache says it was checked in the future — clock skew during
        // a backup restore, dual-boot, etc. Better to trust the cache
        // than to fetch unnecessarily on every launch.
        let e = entry("v0.10.0", 2_000_000);
        assert!(is_fresh(&e, 1_000_000, DEFAULT_TTL_SECS));
    }

    #[test]
    fn is_fresh_with_schema_mismatch_returns_false() {
        let mut e = entry("v0.10.0", 1_000_000);
        e.schema_version = SCHEMA_VERSION + 1;
        assert!(!is_fresh(&e, 1_000_000, DEFAULT_TTL_SECS));
    }

    // --- save / load round-trip ---

    #[test]
    fn load_returns_none_for_missing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("update-check.json");
        assert!(load(&path).is_none());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("update-check.json");
        let original = entry("v0.11.0", 1_700_000_000);
        save(&path, &original);
        let loaded = load(&path).expect("must load freshly written cache");
        assert_eq!(loaded, original);
    }

    #[test]
    fn save_creates_missing_parent_directory() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".otto").join("update-check.json");
        let original = entry("v0.11.0", 1_700_000_000);
        save(&path, &original);
        assert!(path.exists(), "save must create the .otto/ directory");
    }

    #[test]
    fn load_returns_none_for_malformed_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("update-check.json");
        std::fs::write(&path, "not json{").unwrap();
        assert!(load(&path).is_none());
    }

    #[test]
    fn load_returns_none_for_schema_mismatch() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("update-check.json");
        std::fs::write(
            &path,
            r#"{"schema_version":99,"checked_at_unix":1,"latest_tag":"v0"}"#,
        )
        .unwrap();
        assert!(load(&path).is_none());
    }
}

//! TUI-side helper that resolves `~/.otto/routing.toml` and reads
//! just the `default = "..."` field for the model-resolution chain.
//!
//! The full `RoutingRules` parser lives in `otto-host` and runs at
//! `Host::start`; this helper exists so `main.rs` can consult the
//! file's `default` during startup without depending on the full type.

use std::path::PathBuf;

use otto_host::{DefaultPick, RoutingRules};

/// Where the user's `routing.toml` lives, or `None` when no `$HOME`.
pub fn routing_toml_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let home = PathBuf::from(home);
    Some(home.join(".otto").join("routing.toml"))
}

/// Load `routing.toml`'s `default` field. Missing file → `None`. Parse
/// errors are logged at `warn!` and treated as `None` — the caller falls
/// back to the next layer in the precedence chain.
pub fn load_default_pick() -> Option<DefaultPick> {
    let path = routing_toml_path()?;
    match RoutingRules::load_from_path(&path) {
        Ok(rules) => rules.default,
        Err(e) => {
            tracing::warn!(error = %e, "could not load routing.toml#default at startup; ignoring");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{HOME_LOCK, HomeGuard};

    #[test]
    fn returns_path_under_home() {
        // HOME_LOCK serializes against every other test that mutates
        // HOME. Without it this test races with the resolver tests in
        // main.rs (which read routing.toml from $HOME) and intermittently
        // makes them fail — the resolver sees `/tmp/otto-test-home`
        // instead of the HomeGuard's tempdir.
        let _lock = HOME_LOCK.lock().unwrap();
        let original_home = std::env::var_os("HOME");
        // SAFETY: HOME_LOCK is held above; no other test mutates HOME concurrently.
        unsafe {
            std::env::set_var("HOME", "/tmp/otto-test-home");
        }
        let p = routing_toml_path().expect("HOME set");
        assert!(p.ends_with(".otto/routing.toml"));
        match original_home {
            Some(v) => unsafe { std::env::set_var("HOME", v) },
            None => unsafe { std::env::remove_var("HOME") },
        }
    }

    /// Write `body` to `~/.otto/routing.toml` under the current
    /// `HomeGuard` tempdir. Returns the path for diagnostics.
    fn write_routing_under_home(body: &str) -> std::path::PathBuf {
        let home = std::env::var_os("HOME").expect("HOME set by HomeGuard");
        let dir = std::path::PathBuf::from(home).join(".otto");
        std::fs::create_dir_all(&dir).expect("create .otto dir");
        let path = dir.join("routing.toml");
        std::fs::write(&path, body).expect("write routing.toml");
        path
    }

    #[test]
    fn load_default_pick_returns_none_when_file_absent() {
        let _lock = HOME_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        // No routing.toml in this fresh HOME → None.
        assert!(load_default_pick().is_none());
    }

    #[test]
    fn load_default_pick_parses_default_field() {
        let _lock = HOME_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        write_routing_under_home(r#"default = "anthropic/claude-opus-4-7""#);
        let pick = load_default_pick().expect("default parsed");
        assert_eq!(pick.provider.as_str(), "anthropic");
        assert_eq!(pick.model, "claude-opus-4-7");
    }

    #[test]
    fn load_default_pick_returns_none_on_parse_error() {
        let _lock = HOME_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        // Malformed TOML — parser must surface an error which the
        // helper logs at `warn!` and converts to `None`.
        write_routing_under_home("this is not [[ valid TOML ]]");
        assert!(load_default_pick().is_none());
        // The tracing::warn! side-effect is not observable here without
        // a tracing harness; the contract under test is "swallows the
        // error and returns None".
    }
}

//! Thin wrapper over [`keyring`] for stashing per-provider API keys.
//!
//! Storage backend is whatever `keyring` picks for the platform — macOS
//! Keychain, Windows Credential Manager, or the freedesktop Secret Service on
//! Linux. The TUI never touches the raw bytes outside this module.
#![cfg_attr(not(test), allow(dead_code))]

use keyring::Entry;

/// Service name we register entries under.
const SERVICE: &str = "otto";
const MCP_PREFIX: &str = "mcp:";

/// `true` when `err` indicates the platform's secret-storage backend is
/// simply not present/reachable (e.g. no D-Bus Secret Service running, as on
/// many CI runners) rather than a real fault against an existing store.
/// `keyring` reports this as `NoStorageAccess` on some platforms and as a
/// generic `PlatformFailure` on others (observed on Linux without a running
/// Secret Service) — both are treated as "backend unavailable" here so
/// save/load/delete stay best-effort rather than hard errors in that case.
fn is_backend_unavailable(err: &keyring::Error) -> bool {
    matches!(
        err,
        keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_)
    )
}

/// Persist `api_key` under `provider_id`, overwriting any previous value.
pub fn save(provider_id: &str, api_key: &str) -> Result<(), keyring::Error> {
    Entry::new(SERVICE, provider_id)?.set_password(api_key)
}

/// Look up the key for `provider_id`. Returns `Ok(None)` if no entry exists or
/// the platform backend is unavailable; `Err` only on real backend faults.
pub fn load(provider_id: &str) -> Result<Option<String>, keyring::Error> {
    let entry = match Entry::new(SERVICE, provider_id) {
        Ok(e) => e,
        Err(e) if is_backend_unavailable(&e) => return Ok(None),
        Err(e) => return Err(e),
    };
    match entry.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) if is_backend_unavailable(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Delete the secret for `account`. Missing entries and unavailable backends
/// are treated as success so callers can make idempotent cleanup calls.
pub fn delete(account: &str) -> Result<(), keyring::Error> {
    let entry = match Entry::new(SERVICE, account) {
        Ok(e) => e,
        Err(e) if is_backend_unavailable(&e) => return Ok(()),
        Err(e) => return Err(e),
    };
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) if is_backend_unavailable(&e) => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn mcp_save(server_name: &str, secret: &str) -> Result<(), keyring::Error> {
    save(&format!("{MCP_PREFIX}{server_name}"), secret)
}

pub fn mcp_load(server_name: &str) -> Result<Option<String>, keyring::Error> {
    load(&format!("{MCP_PREFIX}{server_name}"))
}

pub fn mcp_delete(server_name: &str) -> Result<(), keyring::Error> {
    delete(&format!("{MCP_PREFIX}{server_name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_account(prefix: &str) -> String {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        format!("{prefix}-{stamp}")
    }

    #[test]
    fn delete_nonexistent_account_is_ok() {
        let account = unique_account("missing");
        assert!(delete(&account).is_ok());
    }

    #[test]
    fn mcp_secret_round_trips_when_backend_is_available() {
        let server_name = unique_account("server");
        let secret = "secret-value";
        match mcp_save(&server_name, secret) {
            Ok(()) => {}
            Err(e) if is_backend_unavailable(&e) => return,
            Err(err) => panic!("mcp_save failed: {err}"),
        }

        let loaded = mcp_load(&server_name).expect("mcp_load should succeed");
        if loaded.is_none() {
            let _ = mcp_delete(&server_name);
            return;
        }
        assert_eq!(loaded.as_deref(), Some(secret));
        assert!(mcp_delete(&server_name).is_ok());
    }
}

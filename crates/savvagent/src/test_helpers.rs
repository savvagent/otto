//! Test-only utilities shared across the savvagent crate's unit test modules.
//!
//! `HOME_LOCK` serialises access to the global home-directory env variables,
//! so any test that uses [`HomeGuard`] (which rewrites them to a temp dir)
//! holds the lock for its lifetime. Both `app::tests` and
//! `theme_command_tests` must import this single mutex — keeping per-module
//! copies would let tests in one module race tests in the other on the
//! process-wide env.
//!
//! Some production paths prefer `HOME`, while others fall back to
//! `USERPROFILE`. To keep every home-rooted path inside a test sandbox, the
//! guards manage BOTH variables on every platform.

#![cfg(test)]

use std::sync::Mutex;

/// Process-wide lock serialising every test that mutates the home-directory
/// env variables (`HOME` everywhere; `USERPROFILE` on Windows).
pub static HOME_LOCK: Mutex<()> = Mutex::new(());

/// RAII guard: on construction, redirects the platform's home-dir env
/// variables to a fresh tmpdir; on drop, restores the previous values (or
/// unsets them). Must be held while the test touches home-rooted paths
/// (e.g. `~/.savvagent/config.toml`, `~/.savvagent/trusted-projects.json`).
pub struct HomeGuard {
    _td: tempfile::TempDir,
    prev_home: Option<std::ffi::OsString>,
    prev_userprofile: Option<std::ffi::OsString>,
}

impl HomeGuard {
    pub fn new() -> Self {
        let td = tempfile::TempDir::new().expect("tempdir");
        let prev_home = std::env::var_os("HOME");
        let prev_userprofile = std::env::var_os("USERPROFILE");
        // SAFETY: setting these env vars is unsafe in Rust 2024 because it
        // mutates process-global state. We hold HOME_LOCK for the lifetime
        // of the guard, so no other test reads them concurrently.
        unsafe {
            std::env::set_var("HOME", td.path());
            std::env::set_var("USERPROFILE", td.path());
        }
        Self {
            _td: td,
            prev_home,
            prev_userprofile,
        }
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        // SAFETY: see HomeGuard::new — we still hold HOME_LOCK here.
        unsafe {
            match &self.prev_home {
                Some(p) => std::env::set_var("HOME", p),
                None => std::env::remove_var("HOME"),
            }
            match &self.prev_userprofile {
                Some(p) => std::env::set_var("USERPROFILE", p),
                None => std::env::remove_var("USERPROFILE"),
            }
        }
    }
}

/// RAII guard: temporarily clears the platform's home-directory env vars so
/// code paths that intentionally require an explicit HOME/USERPROFILE behave
/// like "no home configured" even on systems where `dirs::home_dir()` would
/// otherwise fall back to the account database.
pub struct NoHomeGuard {
    prev_home: Option<std::ffi::OsString>,
    prev_userprofile: Option<std::ffi::OsString>,
}

impl NoHomeGuard {
    pub fn new() -> Self {
        let prev_home = std::env::var_os("HOME");
        let prev_userprofile = std::env::var_os("USERPROFILE");
        // SAFETY: HOME_LOCK is held for the guard lifetime.
        unsafe {
            std::env::remove_var("HOME");
            std::env::remove_var("USERPROFILE");
        }
        Self {
            prev_home,
            prev_userprofile,
        }
    }
}

impl Drop for NoHomeGuard {
    fn drop(&mut self) {
        // SAFETY: see NoHomeGuard::new — HOME_LOCK is still held here.
        unsafe {
            match &self.prev_home {
                Some(p) => std::env::set_var("HOME", p),
                None => std::env::remove_var("HOME"),
            }
            match &self.prev_userprofile {
                Some(p) => std::env::set_var("USERPROFILE", p),
                None => std::env::remove_var("USERPROFILE"),
            }
        }
    }
}

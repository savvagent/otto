//! Install-method detection for `internal:self-update`.
//!
//! The plugin needs to distinguish two cases when deciding whether to run
//! the version check and offer `/update`:
//!
//! - [`InstallMethod::Installed`] — any binary outside the workspace
//!   `target/` tree. Both `cargo install` users and cargo-dist tarball
//!   users land here, because cargo-dist's shell/powershell installers
//!   drop binaries into `$CARGO_HOME/bin`, the same place `cargo install`
//!   would.
//! - [`InstallMethod::Dev`] — the binary lives under a Cargo workspace's
//!   `target/debug/` or `target/release/` directory. The plugin skips the
//!   network check entirely and `/update` prints a hint instead of
//!   touching the binary.
//!
//! The runtime entry point [`detect`] wraps [`std::env::current_exe`].
//! The pure helper [`detect_from_path`] takes an explicit path so unit
//! tests can exercise both branches without touching the filesystem.

use std::path::{Path, PathBuf};

/// How the running `otto` binary was installed, from the plugin's
/// point of view. The distinction drives whether the self-update path is
/// available or short-circuited to a "hint, don't touch the binary"
/// behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMethod {
    /// Binary lives outside any workspace `target/` tree. Treated as a
    /// real cargo-dist install — update path is enabled.
    Installed,
    /// Binary lives under `target/debug/` or `target/release/` inside a
    /// Cargo workspace. Update path is short-circuited to a hint.
    Dev,
}

/// Detect [`InstallMethod`] for the current process. Returns
/// [`InstallMethod::Installed`] on any `current_exe()` failure — the
/// safer default in the presence of unexpected platform quirks is to
/// allow the update check, since the plugin's own opt-out gates (env
/// var, CLI flag) still apply downstream.
pub fn detect() -> InstallMethod {
    match std::env::current_exe() {
        Ok(p) => detect_from_path(&p),
        Err(_) => InstallMethod::Installed,
    }
}

/// Pure detection helper: classifies `exe_path` by inspecting its parent
/// directory. A path whose immediate parent is named `debug` or `release`
/// and whose grandparent is named `target` is considered a dev build;
/// everything else is treated as installed.
pub fn detect_from_path(exe_path: &Path) -> InstallMethod {
    let parent = exe_path.parent();
    let grandparent = parent.and_then(Path::parent);
    let parent_name = parent.and_then(Path::file_name).and_then(|n| n.to_str());
    let grandparent_name = grandparent
        .and_then(Path::file_name)
        .and_then(|n| n.to_str());

    if matches!(parent_name, Some("debug" | "release"))
        && matches!(grandparent_name, Some("target"))
    {
        InstallMethod::Dev
    } else {
        InstallMethod::Installed
    }
}

/// Convenience for logging / diagnostic surfaces: returns the running
/// binary path if available. Not consumed by the production code path
/// today; exposed so PR 4 can include it in `/update`'s error notes
/// without re-walking `current_exe()`.
#[allow(dead_code)]
pub fn current_exe_path() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn dev_build_under_target_debug_is_dev() {
        let exe = p("/home/u/proj/target/debug/otto");
        assert_eq!(detect_from_path(&exe), InstallMethod::Dev);
    }

    #[test]
    fn dev_build_under_target_release_is_dev() {
        let exe = p("/home/u/proj/target/release/otto");
        assert_eq!(detect_from_path(&exe), InstallMethod::Dev);
    }

    #[test]
    fn cargo_bin_path_is_installed() {
        let exe = p("/home/u/.cargo/bin/otto");
        assert_eq!(detect_from_path(&exe), InstallMethod::Installed);
    }

    #[test]
    fn system_bin_path_is_installed() {
        let exe = p("/usr/local/bin/otto");
        assert_eq!(detect_from_path(&exe), InstallMethod::Installed);
    }

    #[test]
    fn windows_cargo_bin_path_is_installed() {
        // POSIX-style on test paths is fine — the helper only inspects
        // component names, not platform separators.
        let exe = p("C:/Users/u/.cargo/bin/otto.exe");
        assert_eq!(detect_from_path(&exe), InstallMethod::Installed);
    }

    #[test]
    fn target_subdir_not_named_debug_or_release_is_installed() {
        // `target/wasm32-unknown-unknown/release/otto` would put the
        // parent as `release` and grandparent as `wasm32-unknown-unknown`,
        // which is not `target`. Treat as installed since cross-target
        // dev builds aren't the common case the plugin guards against.
        let exe = p("/home/u/proj/target/wasm32-unknown-unknown/release/otto");
        assert_eq!(detect_from_path(&exe), InstallMethod::Installed);
    }

    #[test]
    fn target_directly_in_root_with_debug_is_still_dev() {
        let exe = p("/target/debug/otto");
        assert_eq!(detect_from_path(&exe), InstallMethod::Dev);
    }

    #[test]
    fn bare_filename_is_installed() {
        let exe = p("otto");
        assert_eq!(detect_from_path(&exe), InstallMethod::Installed);
    }
}

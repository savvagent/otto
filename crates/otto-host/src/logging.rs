//! Logging helpers for tool MCP server spawns.
//!
//! Tool subprocesses inherit stderr from the parent by default. When the
//! parent is the TUI, that bleeds tracing output across ratatui's alternate
//! screen. We redirect each child's stderr to a per-tool append-only log
//! file under `~/.otto/logs/tools/` so the diagnostics survive without
//! corrupting the terminal.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

/// Default directory used by [`tool_stderr_log_file`] when `$HOME` resolves.
///
/// Layout: `~/.otto/logs/tools/<binary-basename>.log`.
fn tools_log_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".otto").join("logs").join("tools"))
}

/// Open (creating, append) a per-tool stderr log file for `command`.
///
/// Returns the open `File` on success. Callers wrap it with
/// `std::process::Stdio::from(file)` and pass that as the child's stderr.
/// On any I/O failure the caller should fall back to `Stdio::null()` — the
/// goal is "never bleed into the TUI", never "fail to spawn".
pub(crate) fn tool_stderr_log_file(command: &Path) -> io::Result<File> {
    let dir =
        tools_log_dir().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is unset"))?;
    std::fs::create_dir_all(&dir)?;
    let stem = command
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("tool");
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{stem}.log")))
}

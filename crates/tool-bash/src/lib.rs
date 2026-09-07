//! Bash command execution as a Model Context Protocol stdio server.
//!
//! Exposes one tool, [`run`](BashTools::run), that an agent host can call
//! over MCP. The bundled `otto-tool-bash` binary wraps [`BashTools`] in
//! an `rmcp` stdio transport — see the shim in `crates/otto/src/bin/`.
//!
//! # Layer 1 path containment
//!
//! Construct via [`BashTools::with_root`] (or set
//! `OTTO_TOOL_BASH_ROOT` for the bundled binary) to confine the tool's
//! working directory to a single project root. With containment on:
//!
//! - `cwd = None` → the project root is used.
//! - `cwd` is canonicalized; if it doesn't lie under the root (e.g. via
//!   `..` or a symlink), the call is rejected.
//!
//! There is **no allowlist of commands** in this tool — gating which
//! commands actually run is the host's responsibility (M9 PR 1's
//! `PermissionPolicy` puts every `run` call through the modal by default).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{
        router::tool::ToolRouter,
        wrapper::{Json, Parameters},
    },
    model::{Implementation, ProtocolVersion, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};

/// Default per-call timeout in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
/// Hard upper bound on per-call timeout. Calls asking for more are clamped.
pub const MAX_TIMEOUT_MS: u64 = 300_000;
/// Per-stream output cap. Bytes past this are dropped and the matching
/// `*_truncated` flag is set on the response.
pub const MAX_STREAM_BYTES: usize = 1 << 20;

// ---------------------------------------------------------------------------
// Tool input/output types
// ---------------------------------------------------------------------------

/// Arguments to `run`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct RunInput {
    /// Shell command to execute. Passed to `bash -c` verbatim.
    pub command: String,
    /// Working directory for the command. With containment on, must
    /// canonicalize to a path under the project root; defaults to the root
    /// when unset. Without containment, defaults to the server's CWD.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Per-call timeout in milliseconds. Defaults to
    /// [`DEFAULT_TIMEOUT_MS`]; clamped to [`MAX_TIMEOUT_MS`].
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// Result of `run`.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunOutput {
    /// Process exit code. `-1` if the process was killed by a signal or the
    /// timeout fired before exit.
    pub exit_code: i32,
    /// Captured stdout, possibly truncated. UTF-8 lossy decoded.
    pub stdout: String,
    /// Captured stderr, possibly truncated. UTF-8 lossy decoded.
    pub stderr: String,
    /// Wall-clock time from spawn to wait, in milliseconds.
    pub elapsed_ms: u64,
    /// True if `stdout` was truncated to [`MAX_STREAM_BYTES`].
    pub stdout_truncated: bool,
    /// True if `stderr` was truncated to [`MAX_STREAM_BYTES`].
    pub stderr_truncated: bool,
    /// True if the process was killed because the timeout fired.
    pub timed_out: bool,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors a tool handler can surface to the caller.
#[derive(Debug, thiserror::Error)]
pub enum BashToolError {
    /// Caller passed an invalid argument (empty command, bad cwd, …).
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// Resolved cwd falls outside the configured project root.
    #[error("cwd is outside project root: {path}")]
    OutsideRoot {
        /// The offending path, as supplied by the caller.
        path: String,
    },
    /// Spawn or wait failed at the OS level.
    #[error("{op}: {source}")]
    Io {
        /// Short label describing the operation that failed.
        op: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
}

impl From<BashToolError> for ErrorData {
    fn from(err: BashToolError) -> Self {
        match err {
            BashToolError::InvalidArgument(_) => ErrorData::invalid_params(err.to_string(), None),
            BashToolError::OutsideRoot { .. } => ErrorData::invalid_request(err.to_string(), None),
            BashToolError::Io { .. } => ErrorData::internal_error(err.to_string(), None),
        }
    }
}

fn io_err(op: &str, source: std::io::Error) -> BashToolError {
    BashToolError::Io {
        op: op.to_string(),
        source,
    }
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// MCP server exposing the `run` tool.
#[derive(Debug, Clone)]
pub struct BashTools {
    #[allow(dead_code)] // Read by the `#[tool_handler]` macro expansion.
    tool_router: ToolRouter<Self>,
    /// When `Some`, `cwd` is confined to this canonicalized directory and
    /// `cwd = None` resolves to the root rather than the process CWD.
    root: Option<PathBuf>,
}

impl Default for BashTools {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl BashTools {
    /// Construct a server with no path containment. `cwd = None` resolves
    /// to the process CWD; arbitrary cwds are accepted.
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            root: None,
        }
    }

    /// Construct a containment-mode server. `cwd` is confined to `root`,
    /// even via `..` or symlinks. `root` must exist and is canonicalized
    /// once at construction.
    pub fn with_root(root: impl AsRef<Path>) -> std::io::Result<Self> {
        let canon = std::fs::canonicalize(root.as_ref())?;
        Ok(Self {
            tool_router: Self::tool_router(),
            root: Some(canon),
        })
    }

    /// Returns the configured project root, if containment is enabled.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Run `command` through `bash -c` and return its exit code, captured
    /// streams, and timing.
    #[tool(
        name = "run",
        description = "Run a shell command via `bash -c`. Returns exit_code, stdout, stderr, and elapsed_ms. Use cwd to set the working directory; with containment on it must lie under the project root."
    )]
    pub async fn run(
        &self,
        Parameters(input): Parameters<RunInput>,
    ) -> Result<Json<RunOutput>, ErrorData> {
        if input.command.trim().is_empty() {
            return Err(BashToolError::InvalidArgument("command is empty".into()).into());
        }

        let cwd = self.resolve_cwd(input.cwd.as_deref())?;
        let timeout_ms = input
            .timeout_ms
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .min(MAX_TIMEOUT_MS);

        let mut cmd = tokio::process::Command::new("bash");
        cmd.arg("-c")
            .arg(&input.command)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(c) = &cwd {
            cmd.current_dir(c);
        }

        let started = Instant::now();
        let child = cmd.spawn().map_err(|e| io_err("spawn bash", e))?;

        let wait = child.wait_with_output();
        let result = tokio::time::timeout(Duration::from_millis(timeout_ms), wait).await;
        let elapsed_ms = started.elapsed().as_millis() as u64;

        let (exit_code, stdout_raw, stderr_raw, timed_out) = match result {
            Ok(Ok(out)) => (
                out.status.code().unwrap_or(-1),
                out.stdout,
                out.stderr,
                false,
            ),
            Ok(Err(e)) => return Err(io_err("wait bash", e).into()),
            Err(_) => {
                // Timeout fired — `child` was dropped by `wait_with_output`'s
                // future; `kill_on_drop=true` reaps it. Best effort
                // reporting only.
                (-1, Vec::new(), Vec::new(), true)
            }
        };

        let (stdout, stdout_truncated) = truncate_lossy(stdout_raw);
        let (stderr, stderr_truncated) = truncate_lossy(stderr_raw);

        Ok(Json(RunOutput {
            exit_code,
            stdout,
            stderr,
            elapsed_ms,
            stdout_truncated,
            stderr_truncated,
            timed_out,
        }))
    }

    fn resolve_cwd(&self, raw: Option<&str>) -> Result<Option<PathBuf>, BashToolError> {
        match (&self.root, raw) {
            // No containment, no cwd → use server CWD (returning None lets
            // tokio's Command inherit it).
            (None, None) => Ok(None),
            // No containment, explicit cwd → take it as-is.
            (None, Some(c)) => Ok(Some(PathBuf::from(c))),
            // Containment, no cwd → use the project root.
            (Some(root), None) => Ok(Some(root.clone())),
            // Containment + explicit cwd → resolve, refuse traversal,
            // canonicalize, require under-root.
            (Some(root), Some(raw)) => {
                let input = Path::new(raw);
                if path_has_parent_dir(input) {
                    return Err(BashToolError::OutsideRoot {
                        path: raw.to_string(),
                    });
                }
                let candidate = if input.is_absolute() {
                    input.to_path_buf()
                } else {
                    root.join(input)
                };
                let canon =
                    std::fs::canonicalize(&candidate).map_err(|e| io_err("canonicalize cwd", e))?;
                if !is_within(&canon, root) {
                    return Err(BashToolError::OutsideRoot {
                        path: raw.to_string(),
                    });
                }
                Ok(Some(canon))
            }
        }
    }
}

#[tool_handler]
impl ServerHandler for BashTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::default())
            .with_server_info(
                Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
                    .with_description("Otto bash tool (run)"),
            )
            .with_instructions(
                "Bash tool server for Otto. Hosts should keep the policy default \
                 of `bash: ask` so every invocation goes through a permission prompt; \
                 the tool itself enforces no allowlist.",
            )
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn path_has_parent_dir(path: &Path) -> bool {
    use std::path::Component;
    path.components().any(|c| matches!(c, Component::ParentDir))
}

fn is_within(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

/// UTF-8-lossy decode `bytes`, truncating to [`MAX_STREAM_BYTES`] first.
/// The returned bool indicates whether truncation occurred.
fn truncate_lossy(mut bytes: Vec<u8>) -> (String, bool) {
    let truncated = bytes.len() > MAX_STREAM_BYTES;
    if truncated {
        bytes.truncate(MAX_STREAM_BYTES);
    }
    (String::from_utf8_lossy(&bytes).into_owned(), truncated)
}

// ---------------------------------------------------------------------------
// Binary entry point
// ---------------------------------------------------------------------------

/// Serve [`BashTools`] over a stdio MCP transport. Shared between the
/// `otto-tool-bash` binary in the `otto` crate and any future
/// standalone packaging.
pub async fn run() -> anyhow::Result<()> {
    use rmcp::{ServiceExt, transport::stdio};

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    tracing::info!(
        "otto-tool-bash {} starting on stdio",
        env!("CARGO_PKG_VERSION")
    );

    let tools = build_tools_from_env();
    let service = tools.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

fn build_tools_from_env() -> BashTools {
    let env_root = std::env::var("OTTO_TOOL_BASH_ROOT")
        .ok()
        .filter(|s| !s.is_empty());
    if let Some(root) = env_root {
        match BashTools::with_root(&root) {
            Ok(t) => {
                tracing::info!(root = %root, "tool-bash containment enabled (env)");
                return t;
            }
            Err(e) => {
                tracing::warn!(
                    root = %root,
                    error = %e,
                    "OTTO_TOOL_BASH_ROOT failed to canonicalize; falling back to CWD",
                );
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir()
        && let Ok(t) = BashTools::with_root(&cwd)
    {
        tracing::info!(root = %cwd.display(), "tool-bash containment enabled (cwd)");
        return t;
    }
    tracing::warn!("tool-bash running without containment (no usable root)");
    BashTools::new()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[cfg(unix)]
    #[tokio::test]
    async fn run_echoes_to_stdout() {
        let tools = BashTools::new();
        let out = tools
            .run(Parameters(RunInput {
                command: "echo hello".into(),
                cwd: None,
                timeout_ms: None,
            }))
            .await
            .unwrap();
        assert_eq!(out.0.exit_code, 0);
        assert_eq!(out.0.stdout.trim(), "hello");
        assert!(out.0.stderr.is_empty());
        assert!(!out.0.timed_out);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn nonzero_exit_is_reported() {
        let tools = BashTools::new();
        let out = tools
            .run(Parameters(RunInput {
                command: "exit 7".into(),
                cwd: None,
                timeout_ms: None,
            }))
            .await
            .unwrap();
        assert_eq!(out.0.exit_code, 7);
    }

    #[tokio::test]
    async fn rejects_empty_command() {
        let tools = BashTools::new();
        assert!(
            tools
                .run(Parameters(RunInput {
                    command: "   ".into(),
                    cwd: None,
                    timeout_ms: None,
                }))
                .await
                .is_err()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_kills_long_running_command() {
        let tools = BashTools::new();
        let out = tools
            .run(Parameters(RunInput {
                command: "sleep 5".into(),
                cwd: None,
                timeout_ms: Some(100),
            }))
            .await
            .unwrap();
        assert!(out.0.timed_out, "expected timed_out=true");
        assert_eq!(out.0.exit_code, -1);
        assert!(out.0.elapsed_ms < 1_000, "elapsed={}", out.0.elapsed_ms);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cwd_defaults_to_project_root_with_containment() {
        let dir = tempdir().unwrap();
        tokio::fs::write(dir.path().join("marker.txt"), b"yes")
            .await
            .unwrap();

        let tools = BashTools::with_root(dir.path()).unwrap();
        let out = tools
            .run(Parameters(RunInput {
                command: "cat marker.txt".into(),
                cwd: None,
                timeout_ms: None,
            }))
            .await
            .unwrap();
        assert_eq!(out.0.exit_code, 0);
        assert_eq!(out.0.stdout, "yes");
    }

    #[tokio::test]
    async fn cwd_outside_root_is_rejected() {
        let inside = tempdir().unwrap();
        let outside = tempdir().unwrap();

        let tools = BashTools::with_root(inside.path()).unwrap();
        let err = tools
            .run(Parameters(RunInput {
                command: "pwd".into(),
                cwd: Some(outside.path().to_string_lossy().into_owned()),
                timeout_ms: None,
            }))
            .await
            .err()
            .expect("expected error");
        assert!(
            err.message.to_lowercase().contains("outside"),
            "{}",
            err.message
        );
    }

    #[tokio::test]
    async fn cwd_parent_traversal_is_rejected() {
        let inside = tempdir().unwrap();
        let tools = BashTools::with_root(inside.path()).unwrap();
        let err = tools
            .run(Parameters(RunInput {
                command: "pwd".into(),
                cwd: Some("../escape".into()),
                timeout_ms: None,
            }))
            .await
            .err()
            .expect("expected error");
        assert!(
            err.message.to_lowercase().contains("outside"),
            "{}",
            err.message
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn relative_cwd_resolves_under_root() {
        let inside = tempdir().unwrap();
        tokio::fs::create_dir(inside.path().join("sub"))
            .await
            .unwrap();
        tokio::fs::write(inside.path().join("sub/note.txt"), b"abc")
            .await
            .unwrap();

        let tools = BashTools::with_root(inside.path()).unwrap();
        let out = tools
            .run(Parameters(RunInput {
                command: "cat note.txt".into(),
                cwd: Some("sub".into()),
                timeout_ms: None,
            }))
            .await
            .unwrap();
        assert_eq!(out.0.stdout, "abc");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_clamps_to_max() {
        // We can't easily prove the clamp by black-box test, so just check
        // that an absurdly large request still returns quickly via the
        // command finishing on its own.
        let tools = BashTools::new();
        let out = tools
            .run(Parameters(RunInput {
                command: "echo ok".into(),
                cwd: None,
                timeout_ms: Some(u64::MAX),
            }))
            .await
            .unwrap();
        assert_eq!(out.0.exit_code, 0);
        assert!(!out.0.timed_out);
    }
}

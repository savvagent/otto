//! Code search tools as a Model Context Protocol stdio server.
//!
//! Exposes one tool that an agent host can call over MCP:
//!
//! - [`search`](GrepTools::search) — regex search rooted at a directory,
//!   returning structured `{file, line, column, text}` matches.
//!
//! Built on BurntSushi's `grep-regex` / `grep-searcher` for matching and
//! `ignore` for gitignore-aware traversal — gitignore semantics, dot-file
//! skipping, and `.git/` exclusion are inherited for free.
//!
//! Layer-1 path containment via [`GrepTools::with_root`] (or set
//! `OTTO_TOOL_GREP_ROOT` for the bundled binary) is **non-optional**:
//! every path in the result is canonicalized and required to lie within
//! the configured root. Sensitive paths (`.env*`, `.ssh/`, anything
//! containing `credential` case-insensitively) are filtered server-side
//! before serialization.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub use search::{SearchInput, SearchMatch, SearchOutput};

mod search;

use std::path::{Path, PathBuf};

use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{
        router::tool::ToolRouter,
        wrapper::{Json, Parameters},
    },
    model::{Implementation, ProtocolVersion, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};

/// Default cap on matches returned by `search`. Override per call via
/// [`SearchInput::max_results`].
pub const DEFAULT_MAX_SEARCH_MATCHES: u32 = 1024;

/// Errors a tool handler can surface to the caller.
#[derive(Debug, thiserror::Error)]
pub enum GrepToolError {
    /// Underlying filesystem I/O failed.
    #[error("{op}: {source}")]
    Io {
        /// Short label describing the operation that failed.
        op: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Caller passed an invalid argument.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// Pattern failed to compile.
    #[error("invalid regex pattern: {0}")]
    Regex(String),
    /// Glob filter failed to compile.
    #[error("invalid glob filter: {0}")]
    Glob(String),
    /// Resolved path falls outside the configured project root.
    #[error("path is outside project root: {path}")]
    OutsideRoot {
        /// The offending path, as supplied by the caller.
        path: String,
    },
}

impl From<GrepToolError> for ErrorData {
    fn from(err: GrepToolError) -> Self {
        match err {
            GrepToolError::InvalidArgument(_)
            | GrepToolError::Regex(_)
            | GrepToolError::Glob(_) => ErrorData::invalid_params(err.to_string(), None),
            GrepToolError::OutsideRoot { .. } => ErrorData::invalid_request(err.to_string(), None),
            GrepToolError::Io { .. } => ErrorData::internal_error(err.to_string(), None),
        }
    }
}

/// MCP server exposing the search tool.
#[derive(Debug, Clone)]
pub struct GrepTools {
    #[allow(dead_code)] // Read by the `#[tool_handler]` macro expansion.
    tool_router: ToolRouter<Self>,
    /// When `Some`, the search tool confines inputs to this canonicalized
    /// directory. When `None`, results are unrestricted (intended for unit
    /// tests only — production binaries always carry a root).
    root: Option<PathBuf>,
}

impl Default for GrepTools {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl GrepTools {
    /// Construct a new server instance with no path containment. Intended
    /// for in-crate unit tests; production binaries should use
    /// [`with_root`](Self::with_root).
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            root: None,
        }
    }

    /// Construct a containment-mode server. The search tool will refuse to
    /// surface matches outside `root`, even via `..` or symlinks. `root`
    /// must exist and is canonicalized once at construction.
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

    /// Run a regex search rooted at the configured project root.
    #[tool(
        name = "search",
        description = "Regex search across files under a project root, returning {file, line, column, text} matches. Honors .gitignore by default."
    )]
    pub async fn search(
        &self,
        Parameters(input): Parameters<SearchInput>,
    ) -> Result<Json<SearchOutput>, ErrorData> {
        let root = self.root.clone();
        let result = tokio::task::spawn_blocking(move || search::run(root.as_deref(), input))
            .await
            .map_err(|e| GrepToolError::InvalidArgument(format!("search task panicked: {e}")))??;
        Ok(Json(result))
    }
}

#[tool_handler]
impl ServerHandler for GrepTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::default())
            .with_server_info(
                Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
                    .with_description(
                        "Otto code-search tool (search). \
                         Layer-1 path containment is non-optional in production builds.",
                    ),
            )
            .with_instructions(
                "Code search tool server for Otto. Layer-1 path containment is \
                 non-optional in production builds; sensitive paths are filtered \
                 server-side before results are returned.",
            )
    }
}

/// Serve [`GrepTools`] over a stdio MCP transport. Shared between the
/// `otto-tool-grep` binary and the bundled shim in the `otto`
/// crate's release archive.
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
        "otto-tool-grep {} starting on stdio",
        env!("CARGO_PKG_VERSION")
    );

    let tools = build_tools_from_env()?;
    let service = tools.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

/// Build a [`GrepTools`] honoring `OTTO_TOOL_GREP_ROOT`. Falls back to
/// the process CWD. Layer-1 path containment is non-optional: if neither
/// the env var nor CWD can be canonicalized into a valid root, this
/// returns an error so the binary exits non-zero rather than silently
/// serving without containment.
fn build_tools_from_env() -> anyhow::Result<GrepTools> {
    let env_root = std::env::var("OTTO_TOOL_GREP_ROOT")
        .ok()
        .filter(|s| !s.is_empty());
    if let Some(root) = env_root {
        // The operator deliberately set the env var. If we can't honor it,
        // fail closed rather than silently picking a different directory —
        // that would be a privilege-confusion vector.
        return match GrepTools::with_root(&root) {
            Ok(t) => {
                tracing::info!(root = %root, "tool-grep containment enabled (env)");
                Ok(t)
            }
            Err(e) => {
                tracing::error!(
                    root = %root,
                    error = %e,
                    "OTTO_TOOL_GREP_ROOT failed to canonicalize; refusing to start",
                );
                Err(anyhow::anyhow!(
                    "OTTO_TOOL_GREP_ROOT={root} failed to canonicalize: {e}"
                ))
            }
        };
    }
    let cwd = std::env::current_dir().map_err(|e| {
        tracing::error!(error = %e, "current_dir() failed; refusing to start without containment");
        anyhow::anyhow!("current_dir() failed: {e}")
    })?;
    match GrepTools::with_root(&cwd) {
        Ok(t) => {
            tracing::info!(root = %cwd.display(), "tool-grep containment enabled (cwd)");
            Ok(t)
        }
        Err(e) => {
            tracing::error!(
                root = %cwd.display(),
                error = %e,
                "CWD failed to canonicalize; refusing to start without containment",
            );
            Err(anyhow::anyhow!(
                "cwd {} failed to canonicalize: {e}",
                cwd.display()
            ))
        }
    }
}

#[cfg(test)]
mod mcp_tests {
    use super::*;
    use rmcp::handler::server::wrapper::Parameters;
    use tempfile::tempdir;

    #[tokio::test]
    async fn mcp_search_round_trip() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn main() {}\n").unwrap();

        let tools = GrepTools::with_root(dir.path()).unwrap();
        let out = tools
            .search(Parameters(SearchInput {
                pattern: "fn ".into(),
                ..Default::default()
            }))
            .await
            .unwrap();

        assert_eq!(out.0.matches.len(), 1);
        assert_eq!(out.0.matches[0].file, "a.rs");
    }
}

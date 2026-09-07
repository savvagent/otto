//! Bundled `otto-tool-grep` binary. Delegates to [`tool_grep::run`]
//! so the release archive ships the search tool alongside the TUI under
//! one installer.

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tool_grep::run().await
}

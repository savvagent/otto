//! Bundled `otto-tool-bash` binary. Delegates to [`tool_bash::run`] so
//! the release archive ships the bash tool alongside the TUI under one
//! installer.

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tool_bash::run().await
}

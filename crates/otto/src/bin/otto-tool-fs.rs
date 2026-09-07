//! Bundled `otto-tool-fs` binary. Delegates to [`tool_fs::run`] so the
//! release archive ships fs tools alongside the TUI under one installer.

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tool_fs::run().await
}

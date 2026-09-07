//! Bundled `otto-deepseek` binary. Delegates to [`provider_deepseek::run`]
//! so the release archive ships the standalone provider HTTP server alongside
//! the TUI under one installer.

#[tokio::main]
async fn main() -> std::process::ExitCode {
    provider_deepseek::run().await
}

//! Bundled `otto-grok` binary. Delegates to [`provider_grok::run`]
//! so the release archive ships the standalone provider HTTP server alongside
//! the TUI under one installer.

#[tokio::main]
async fn main() -> std::process::ExitCode {
    provider_grok::run().await
}

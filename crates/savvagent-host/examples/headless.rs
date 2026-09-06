//! Headless smoke-test for the host: connect to a real `provider-anthropic`
//! Streamable HTTP server and a real `tool-fs` stdio server, then run one
//! turn against whatever prompt is on the command line.
//!
//! Usage:
//!
//! ```bash
//! # Start a provider-anthropic server in another shell first.
//! ANTHROPIC_API_KEY=... cargo run -p provider-anthropic
//!
//! cargo run -p savvagent-host --example headless -- "List my Cargo.toml"
//! ```
//!
//! Configuration env vars:
//!
//! - `SAVVAGENT_PROVIDER_URL` (default `http://127.0.0.1:8787/mcp`)
//! - `SAVVAGENT_MODEL`        (default `claude-haiku-4-5`)
//! - `SAVVAGENT_TOOL_FS_BIN`  (default: looked up on `$PATH` as `savvagent-tool-fs`)

use std::path::PathBuf;
use std::process::ExitCode;

use savvagent_host::{Host, HostConfig, ProviderEndpoint, ToolEndpoint};

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,rmcp=warn")),
        )
        .with_target(false)
        .init();

    let prompt: String = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    if prompt.is_empty() {
        eprintln!("usage: headless <prompt>");
        return ExitCode::from(2);
    }

    let url = std::env::var("SAVVAGENT_PROVIDER_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8787/mcp".to_string());
    let model = std::env::var("SAVVAGENT_MODEL").unwrap_or_else(|_| "claude-haiku-4-5".to_string());
    let tool_bin: PathBuf = std::env::var("SAVVAGENT_TOOL_FS_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("savvagent-tool-fs"));

    let config = HostConfig::new(ProviderEndpoint::StreamableHttp { url }, model)
        .with_tool(ToolEndpoint::Stdio {
            name: "fs".to_string(),
            command: tool_bin,
            args: vec![],
            env: Default::default(),
        })
        .with_project_root(std::env::current_dir().unwrap_or_else(|_| ".".into()));

    let host = match Host::start(config).await {
        Ok(h) => h,
        Err(e) => {
            eprintln!("startup failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    match host.run_turn(prompt).await {
        Ok(outcome) => {
            for (i, call) in outcome.tool_calls.iter().enumerate() {
                eprintln!(
                    "[tool {i}] {} ({:?}) → {} chars",
                    call.name,
                    call.status,
                    call.result.len()
                );
            }
            eprintln!("[iterations] {}", outcome.iterations);
            println!("{}", outcome.text);
            host.shutdown().await;
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("turn failed: {e}");
            host.shutdown().await;
            ExitCode::FAILURE
        }
    }
}

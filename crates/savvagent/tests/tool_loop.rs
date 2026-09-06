//! Acceptance test for PRD §M4 — "list current dir, then read Cargo.toml,
//! then summarize" succeeds end-to-end.
//!
//! Architecture:
//!
//! - A `ScriptedProvider` (mock [`ProviderClient`]) emits a fixed sequence of
//!   assistant turns: `list_dir` → `read_file` → final text. It receives the
//!   real conversation history (including tool results) on each call and
//!   asserts the host wired everything correctly.
//! - The real `savvagent-tool-fs` binary (built from this crate) is spawned
//!   as a child stdio MCP server pointed at a temp directory. Cargo sets
//!   `CARGO_BIN_EXE_savvagent-tool-fs` for tests in the same package as the
//!   bin, so we don't have to walk the workspace looking for it.
//! - The host glues them together. We assert the loop converges and the trace
//!   of tool calls matches the script.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use savvagent_host::{Host, HostConfig, ProviderEndpoint, ToolCallStatus, ToolEndpoint};
use savvagent_mcp::ProviderClient;
use savvagent_protocol::{
    CompleteRequest, CompleteResponse, ContentBlock, ProviderError, Role, StopReason, StreamEvent,
    Usage,
};
use serde_json::json;
use tempfile::tempdir;
use tokio::sync::mpsc;

fn tool_fs_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_savvagent-tool-fs"))
}

/// One scripted assistant turn.
struct ScriptStep {
    /// Closure that returns the assistant content blocks for this turn. The
    /// closure receives the request the host sent, so the script can sanity-
    /// check inputs (history, tool defs, …) before responding.
    response: ResponseFn,
    stop_reason: StopReason,
}

type ResponseFn = Box<dyn Fn(&CompleteRequest) -> Vec<ContentBlock> + Send + Sync>;

struct ScriptedProvider {
    steps: Vec<ScriptStep>,
    cursor: AtomicUsize,
    seen: Arc<tokio::sync::Mutex<Vec<CompleteRequest>>>,
}

impl ScriptedProvider {
    fn new(steps: Vec<ScriptStep>) -> Self {
        Self {
            steps,
            cursor: AtomicUsize::new(0),
            seen: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl ProviderClient for ScriptedProvider {
    async fn complete(
        &self,
        req: CompleteRequest,
        _events: Option<mpsc::Sender<StreamEvent>>,
    ) -> Result<CompleteResponse, ProviderError> {
        let idx = self.cursor.fetch_add(1, Ordering::SeqCst);
        self.seen.lock().await.push(req.clone());
        let step = self.steps.get(idx).unwrap_or_else(|| {
            panic!(
                "ScriptedProvider exhausted: complete() called {} times, only {} steps configured",
                idx + 1,
                self.steps.len()
            )
        });
        let content = (step.response)(&req);
        Ok(CompleteResponse {
            id: format!("msg_{idx}"),
            model: req.model,
            content,
            stop_reason: step.stop_reason,
            stop_sequence: None,
            usage: Usage::default(),
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn list_then_read_then_summarize() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_test_writer()
        .try_init();

    // 1. Lay out a tempdir that mimics a tiny Rust project.
    let project = tempdir().unwrap();
    let project_path = project.path().to_path_buf();
    std::fs::write(
        project_path.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(project_path.join("README.md"), "# demo\n").unwrap();

    let project_str = project_path.to_string_lossy().into_owned();
    let cargo_toml = project_path.join("Cargo.toml");
    let cargo_toml_str = cargo_toml.to_string_lossy().into_owned();

    // 2. Build the script: list_dir → read_file → summary.
    let cargo_toml_for_step2 = cargo_toml_str.clone();
    let project_for_step1 = project_str.clone();
    let script = vec![
        ScriptStep {
            response: Box::new(move |req: &CompleteRequest| {
                // Sanity: the host must advertise the four tool-fs tools.
                let names: Vec<&str> = req.tools.iter().map(|t| t.name.as_str()).collect();
                assert!(names.contains(&"list_dir"), "missing list_dir in {names:?}");
                assert!(names.contains(&"read_file"));
                vec![ContentBlock::ToolUse {
                    id: "toolu_1".into(),
                    name: "list_dir".into(),
                    input: json!({ "path": project_for_step1 }),
                }]
            }),
            stop_reason: StopReason::ToolUse,
        },
        ScriptStep {
            response: Box::new(move |req: &CompleteRequest| {
                // After step 1, history must contain a tool_result for toolu_1.
                let last = req.messages.last().expect("messages non-empty");
                assert_eq!(last.role, Role::User);
                let has_result = last.content.iter().any(|b| {
                    matches!(b, ContentBlock::ToolResult { tool_use_id, is_error, .. }
                             if tool_use_id == "toolu_1" && !is_error)
                });
                assert!(has_result, "expected tool_result for toolu_1");
                vec![ContentBlock::ToolUse {
                    id: "toolu_2".into(),
                    name: "read_file".into(),
                    input: json!({ "path": cargo_toml_for_step2 }),
                }]
            }),
            stop_reason: StopReason::ToolUse,
        },
        ScriptStep {
            response: Box::new(|req: &CompleteRequest| {
                // Step 3: history should now include both tool_results.
                let tool_result_count = req
                    .messages
                    .iter()
                    .flat_map(|m| m.content.iter())
                    .filter(|b| matches!(b, ContentBlock::ToolResult { .. }))
                    .count();
                assert_eq!(tool_result_count, 2, "expected two tool_results in history");
                vec![ContentBlock::Text {
                    text: "Summary: the project is named `demo` at version 0.1.0.".into(),
                }]
            }),
            stop_reason: StopReason::EndTurn,
        },
    ];
    let provider = ScriptedProvider::new(script);

    // 3. Build the host with a stub provider endpoint (with_components
    //    bypasses connect for the provider but still spawns tools normally).
    let bin = tool_fs_bin();
    let config = HostConfig::new(
        ProviderEndpoint::StreamableHttp {
            url: "http://unused".into(),
        },
        "claude-test",
    )
    .with_tool(ToolEndpoint::Stdio {
        name: "fs".to_string(),
        command: bin,
        args: vec![],
        env: Default::default(),
    })
    .with_project_root(project_path.clone());

    let host = Host::with_components(config, Box::new(provider))
        .await
        .expect("host start");

    // 4. Run the turn and assert.
    let outcome = host
        .run_turn("Summarize this project")
        .await
        .expect("run_turn");

    assert_eq!(outcome.iterations, 3);
    assert!(
        outcome.text.contains("demo"),
        "final text should mention `demo`: {}",
        outcome.text
    );
    assert_eq!(outcome.tool_calls.len(), 2);
    assert_eq!(outcome.tool_calls[0].name, "list_dir");
    assert_eq!(outcome.tool_calls[0].status, ToolCallStatus::Ok);
    assert!(outcome.tool_calls[0].result.contains("Cargo.toml"));
    assert_eq!(outcome.tool_calls[1].name, "read_file");
    assert_eq!(outcome.tool_calls[1].status, ToolCallStatus::Ok);
    assert!(outcome.tool_calls[1].result.contains("demo"));

    // 5. Conversation history should have grown by user + 3 assistant + 2
    //    tool-result-bearing user turns = 6 messages.
    let messages = host.messages().await;
    assert_eq!(messages.len(), 6, "{:#?}", messages);

    host.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn loop_limit_kicks_in() {
    // Provider that never says end_turn: just keeps emitting list_dir calls.
    let project = tempdir().unwrap();
    let project_str = project.path().to_string_lossy().into_owned();
    let provider_path = project_str.clone();
    let runaway = ScriptedProvider::new(
        (0..10)
            .map(|i| {
                let p = provider_path.clone();
                ScriptStep {
                    response: Box::new(move |_| {
                        vec![ContentBlock::ToolUse {
                            id: format!("toolu_{i}"),
                            name: "list_dir".into(),
                            input: json!({ "path": p }),
                        }]
                    }),
                    stop_reason: StopReason::ToolUse,
                }
            })
            .collect(),
    );

    let bin = tool_fs_bin();
    let config = HostConfig::new(
        ProviderEndpoint::StreamableHttp {
            url: "http://unused".into(),
        },
        "claude-test",
    )
    .with_tool(ToolEndpoint::Stdio {
        name: "fs".to_string(),
        command: bin,
        args: vec![],
        env: Default::default(),
    })
    .with_max_iterations(3);

    let host = Host::with_components(config, Box::new(runaway))
        .await
        .expect("host start");
    let err = host
        .run_turn("go forever")
        .await
        .expect_err("expected loop-limit error");
    assert!(err.to_string().contains("3 iterations"), "{err}");
    host.shutdown().await;
}

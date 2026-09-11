//! Acceptance tests for `GrokMcpServer`.
//!
//! Each test stands up a fake Grok HTTP server, builds the SPP MCP server
//! around an [`GrokProvider`] pointed at it, and drives it through an
//! `rmcp` Streamable HTTP **client**. Mirrors `provider-gemini`'s
//! `tests/integration.rs`. Covers:
//!
//! - non-streaming `complete` round-trip
//! - streaming `complete` with progress notifications + final result
//!   (quarantined behind `--ignored` for the same `rmcp` progress-dispatch
//!   race that the gemini test hits — see issue #1)
//! - `complete` tool advertised in `list_tools`

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use futures::stream;
use otto_protocol::{CompleteRequest, CompleteResponse, STREAM_EVENT_KIND, StreamEvent};
use provider_grok::{GrokMcpServer, provider_for_tests};
use rmcp::{
    ClientHandler, ServiceExt,
    model::{
        CallToolRequestParams, ClientInfo, Meta, NumberOrString, ProgressNotificationParam,
        ProgressToken,
    },
    service::NotificationContext,
    transport::{
        StreamableHttpClientTransport,
        streamable_http_client::StreamableHttpClientTransportConfig,
        streamable_http_server::{
            StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
        },
    },
};
use serde_json::json;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Fake Grok backend
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum FakeMode {
    Json(serde_json::Value),
    Sse(&'static str),
}

async fn fake_chat_completions(
    State(mode): State<FakeMode>,
    Json(_body): Json<serde_json::Value>,
) -> Response {
    match mode {
        FakeMode::Json(v) => Json(v).into_response(),
        FakeMode::Sse(text) => sse_response(text),
    }
}

fn sse_response(text: &'static str) -> Response {
    let body = axum::body::Body::from_stream(stream::iter([Ok::<_, std::convert::Infallible>(
        bytes::Bytes::from_static(text.as_bytes()),
    )]));
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(body)
        .unwrap()
}

async fn spawn_fake_grok(mode: FakeMode) -> String {
    let app = Router::new()
        .route("/chat/completions", post(fake_chat_completions))
        .with_state(mode);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

// ---------------------------------------------------------------------------
// MCP server harness
// ---------------------------------------------------------------------------

async fn spawn_mcp_server(grok_base: &str) -> String {
    let provider = provider_for_tests(grok_base.to_string());
    let provider = Arc::new(provider);
    let provider_for_factory = provider.clone();
    let svc: StreamableHttpService<GrokMcpServer, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(GrokMcpServer::from_shared(provider_for_factory.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let app = Router::new().nest_service("/mcp", svc);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}/mcp")
}

// ---------------------------------------------------------------------------
// Capturing client (collects progress notifications onto a channel)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct CapturingClient {
    info: ClientInfo,
    tx: mpsc::UnboundedSender<ProgressNotificationParam>,
}

impl ClientHandler for CapturingClient {
    fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _ctx: NotificationContext<rmcp::RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        let tx = self.tx.clone();
        async move {
            let _ = tx.send(params);
        }
    }
    fn get_info(&self) -> ClientInfo {
        self.info.clone()
    }
}

fn make_client() -> (
    CapturingClient,
    mpsc::UnboundedReceiver<ProgressNotificationParam>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        CapturingClient {
            info: ClientInfo::default(),
            tx,
        },
        rx,
    )
}

fn req_text(prompt: &str) -> serde_json::Map<String, serde_json::Value> {
    let req = CompleteRequest::text("grok-4-fast", prompt, 16);
    let v = serde_json::to_value(&req).expect("serialize req");
    v.as_object().unwrap().clone()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn non_streaming_complete_round_trips() {
    let canned = json!({
        "id": "chatcmpl-test-1",
        "model": "grok-4-fast",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "hi back"
            },
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7}
    });
    let upstream = spawn_fake_grok(FakeMode::Json(canned)).await;
    let mcp_url = spawn_mcp_server(&upstream).await;

    let (client, _rx) = make_client();
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(mcp_url),
    );
    let svc = client.serve(transport).await.expect("client init");

    let mut args = req_text("hi");
    args.insert("stream".into(), json!(false));
    let result = svc
        .call_tool(CallToolRequestParams::new("complete").with_arguments(args))
        .await
        .expect("call_tool");

    let resp: CompleteResponse = result.into_typed().expect("structured response");
    assert_eq!(resp.id, "chatcmpl-test-1");
    assert_eq!(resp.model, "grok-4-fast");
    assert!(matches!(
        resp.content.first(),
        Some(otto_protocol::ContentBlock::Text { text }) if text == "hi back"
    ));
    assert_eq!(resp.usage.input_tokens, 5);
    assert_eq!(resp.usage.output_tokens, 2);

    svc.cancel().await.ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn non_streaming_tool_call_round_trips() {
    let canned = json!({
        "id": "chatcmpl-test-tool",
        "model": "grok-4",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_xyz",
                    "type": "function",
                    "function": {
                        "name": "ls",
                        "arguments": "{\"path\":\"/tmp\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 12, "completion_tokens": 5, "total_tokens": 17}
    });
    let upstream = spawn_fake_grok(FakeMode::Json(canned)).await;
    let mcp_url = spawn_mcp_server(&upstream).await;

    let (client, _rx) = make_client();
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(mcp_url),
    );
    let svc = client.serve(transport).await.expect("client init");

    let mut args = req_text("list /tmp");
    args.insert("stream".into(), json!(false));
    let result = svc
        .call_tool(CallToolRequestParams::new("complete").with_arguments(args))
        .await
        .expect("call_tool");

    let resp: CompleteResponse = result.into_typed().expect("structured response");
    assert_eq!(resp.stop_reason, otto_protocol::StopReason::ToolUse);
    match resp.content.first() {
        Some(otto_protocol::ContentBlock::ToolUse { id, name, input }) => {
            assert_eq!(id, "call_xyz");
            assert_eq!(name, "ls");
            assert_eq!(input["path"], "/tmp");
        }
        other => panic!("expected tool_use block, got {other:?}"),
    }

    svc.cancel().await.ok();
}

const FROZEN_SSE: &str = concat!(
    "data: {\"id\":\"chatcmpl-frozen\",\"model\":\"grok-4-fast\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"\"},\"finish_reason\":null}]}\n\n",
    "data: {\"id\":\"chatcmpl-frozen\",\"model\":\"grok-4-fast\",\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
    "data: {\"id\":\"chatcmpl-frozen\",\"model\":\"grok-4-fast\",\"choices\":[{\"delta\":{\"content\":\" world\"},\"finish_reason\":null}]}\n\n",
    "data: {\"id\":\"chatcmpl-frozen\",\"model\":\"grok-4-fast\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":2,\"total_tokens\":9}}\n\n",
    "data: [DONE]\n\n",
);

// Quarantined for v0.1.0: same rmcp progress-dispatch race as the
// gemini-side test (text-payload swap and/or missing trailing event,
// observed on all three CI platforms). Re-enable once issue #1 is fixed.
// Run manually with `cargo test -- --ignored`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "rmcp progress-dispatch race; tracked in issue #1"]
async fn streaming_complete_emits_progress_and_final_response() {
    let upstream = spawn_fake_grok(FakeMode::Sse(FROZEN_SSE)).await;
    let mcp_url = spawn_mcp_server(&upstream).await;

    let (client, mut rx) = make_client();
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(mcp_url),
    );
    let svc = client.serve(transport).await.expect("client init");

    let mut args = req_text("hi");
    args.insert("stream".into(), json!(true));
    let mut params = CallToolRequestParams::new("complete").with_arguments(args);
    params.meta = Some(Meta::with_progress_token(ProgressToken(
        NumberOrString::String("tok-1".into()),
    )));

    let result = svc.call_tool(params).await.expect("call_tool");
    let resp: CompleteResponse = result.into_typed().expect("structured response");
    assert_eq!(resp.id, "chatcmpl-frozen");
    assert_eq!(resp.usage.output_tokens, 2);
    assert!(matches!(
        resp.content.first(),
        Some(otto_protocol::ContentBlock::Text { text }) if text == "hello world"
    ));

    // Drain progress notifications. Worst-case bound — the channel closes
    // the moment the rmcp progress forwarder is aborted in the happy path.
    // See the rmcp ProgressDispatcher note in CLAUDE.md.
    let mut events: Vec<StreamEvent> = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(p)) => {
                let msg = p.message.expect("notification carries message JSON");
                let v: serde_json::Value = serde_json::from_str(&msg).expect("notification JSON");
                assert_eq!(v["kind"], STREAM_EVENT_KIND, "wrong kind: {v}");
                let event: StreamEvent =
                    serde_json::from_value(v["event"].clone()).expect("StreamEvent");
                events.push(event);
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    let names: Vec<&'static str> = events
        .iter()
        .map(|e| match e {
            StreamEvent::MessageStart { .. } => "message_start",
            StreamEvent::ContentBlockStart { .. } => "content_block_start",
            StreamEvent::ContentBlockDelta { .. } => "content_block_delta",
            StreamEvent::ContentBlockStop { .. } => "content_block_stop",
            StreamEvent::MessageDelta { .. } => "message_delta",
            StreamEvent::MessageStop => "message_stop",
            StreamEvent::Ping => "ping",
            StreamEvent::Warning { .. } => "warning",
        })
        .collect();
    assert_eq!(
        names.first().copied(),
        Some("message_start"),
        "first event must be message_start: {events:#?}"
    );
    assert_eq!(
        names.last().copied(),
        Some("message_stop"),
        "last event must be message_stop: {events:#?}"
    );
    assert!(
        names.contains(&"content_block_start"),
        "missing content_block_start: {events:#?}"
    );
    assert!(
        names.contains(&"content_block_stop"),
        "missing content_block_stop: {events:#?}"
    );

    // Concatenated text deltas should reconstruct the message text.
    let concat: String = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ContentBlockDelta {
                delta: otto_protocol::BlockDelta::TextDelta { text },
                ..
            } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(concat, "hello world");

    svc.cancel().await.ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn complete_tool_is_advertised() {
    let upstream = spawn_fake_grok(FakeMode::Json(json!({}))).await;
    let mcp_url = spawn_mcp_server(&upstream).await;
    let (client, _rx) = make_client();
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(mcp_url),
    );
    let svc = client.serve(transport).await.expect("client init");

    let tools = svc.list_all_tools().await.expect("list_tools");
    assert!(
        tools.iter().any(|t| t.name == "complete"),
        "expected `complete` tool, got {:?}",
        tools.iter().map(|t| t.name.to_string()).collect::<Vec<_>>()
    );
    svc.cancel().await.ok();
}

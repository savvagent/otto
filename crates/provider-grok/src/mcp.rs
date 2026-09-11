//! MCP server wrapper around [`GrokProvider`].
//!
//! Exposes a single SPP `complete` tool over an `rmcp` Streamable HTTP server.
//! For streaming requests, [`StreamEvent`]s are forwarded as MCP
//! `notifications/progress` whose `message` field carries the SPP event JSON
//! keyed by [`STREAM_EVENT_KIND`](otto_protocol::STREAM_EVENT_KIND).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use otto_mcp::{EmitError, ProviderHandler, StreamEmitter};
use otto_protocol::{
    self as spp, COMPLETE_TOOL_NAME, CompleteRequest, LIST_MODELS_TOOL_NAME, STREAM_EVENT_KIND,
    StreamEvent,
};
use rmcp::{
    ErrorData, Peer, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Implementation, Meta, ProgressNotificationParam, ProgressToken,
        ProtocolVersion, ServerCapabilities, ServerInfo,
    },
    tool, tool_handler, tool_router,
};

use crate::GrokProvider;

/// MCP server that exposes [`GrokProvider`] as an SPP-conformant
/// `complete` tool. Multiple in-flight calls are safe; the underlying provider
/// is shared via `Arc`.
#[derive(Clone)]
pub struct GrokMcpServer {
    provider: Arc<GrokProvider>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for GrokMcpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GrokMcpServer").finish_non_exhaustive()
    }
}

impl GrokMcpServer {
    /// Wrap a provider for service over MCP.
    pub fn new(provider: GrokProvider) -> Self {
        Self {
            provider: Arc::new(provider),
            tool_router: Self::tool_router(),
        }
    }

    /// Wrap a shared provider.
    pub fn from_shared(provider: Arc<GrokProvider>) -> Self {
        Self {
            provider,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl GrokMcpServer {
    /// SPP `complete` tool. See `crates/otto-protocol/SPEC.md`.
    #[tool(
        name = "complete",
        description = "Run a completion against Grok's Chat Completions API."
    )]
    pub async fn complete(
        &self,
        Parameters(req): Parameters<CompleteRequest>,
        meta: Meta,
        peer: Peer<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let want_stream = req.stream;
        let token = if want_stream {
            meta.get_progress_token()
        } else {
            None
        };
        tracing::info!(
            model = %req.model,
            messages = req.messages.len(),
            tools = req.tools.len(),
            stream = want_stream,
            has_token = token.is_some(),
            "complete tool invoked"
        );

        let emitter: Option<PeerEmitter> = token.map(|t| PeerEmitter::new(peer, t));
        let emit_ref: Option<&dyn StreamEmitter> =
            emitter.as_ref().map(|e| e as &dyn StreamEmitter);

        let result = self.provider.complete(req, emit_ref).await;
        match &result {
            Ok(resp) => tracing::info!(
                stop_reason = ?resp.stop_reason,
                blocks = resp.content.len(),
                "complete tool returning OK"
            ),
            Err(e) => tracing::warn!(
                kind = ?e.kind,
                msg = %e.message,
                "complete tool returning provider error"
            ),
        }
        match result {
            Ok(resp) => {
                let value = serde_json::to_value(&resp).map_err(|e| {
                    ErrorData::internal_error(
                        format!("failed to serialize CompleteResponse: {e}"),
                        None,
                    )
                })?;
                Ok(CallToolResult::structured(value))
            }
            Err(spp_err) => {
                let value = serde_json::to_value(&spp_err).map_err(|e| {
                    ErrorData::internal_error(
                        format!("failed to serialize ProviderError: {e}"),
                        None,
                    )
                })?;
                Ok(CallToolResult::structured_error(value))
            }
        }
    }

    /// SPP `list_models` tool. Queries Grok's `/models` endpoint and
    /// returns every chat-capable listed model — unlike DeepSeek's
    /// unfiltered catalog, Grok's `/models` mixes in non-chat (image/
    /// imagine/voice) model ids, which `GrokProvider::list_models`
    /// filters out before returning.
    #[tool(
        name = "list_models",
        description = "List models this provider can serve."
    )]
    pub async fn list_models_tool(&self) -> Result<CallToolResult, ErrorData> {
        match self.provider.list_models().await {
            Ok(resp) => {
                let value = serde_json::to_value(&resp).map_err(|e| {
                    ErrorData::internal_error(
                        format!("failed to serialize ListModelsResponse: {e}"),
                        None,
                    )
                })?;
                Ok(CallToolResult::structured(value))
            }
            Err(spp_err) => {
                let value = serde_json::to_value(&spp_err).map_err(|e| {
                    ErrorData::internal_error(
                        format!("failed to serialize ProviderError: {e}"),
                        None,
                    )
                })?;
                Ok(CallToolResult::structured_error(value))
            }
        }
    }
}

#[tool_handler]
impl ServerHandler for GrokMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::default())
            .with_server_info(
                Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
                    .with_description(format!(
                        "Otto Grok provider — SPP {} via the `{}` tool",
                        spp::SPP_VERSION,
                        COMPLETE_TOOL_NAME
                    )),
            )
            .with_instructions(format!(
                "SPP-conformant Grok provider. Call `{}` with a CompleteRequest. \
                 For streaming, attach a progress token in `_meta`; events arrive as \
                 `notifications/progress` with `message` carrying the SPP StreamEvent JSON \
                 (kind `{}`). Call `{}` (no arguments) for the model list.",
                COMPLETE_TOOL_NAME, STREAM_EVENT_KIND, LIST_MODELS_TOOL_NAME
            ))
    }
}

/// [`StreamEmitter`] that forwards each [`StreamEvent`] as an MCP
/// `notifications/progress`.
struct PeerEmitter {
    peer: Peer<RoleServer>,
    token: ProgressToken,
    counter: AtomicU64,
}

impl PeerEmitter {
    fn new(peer: Peer<RoleServer>, token: ProgressToken) -> Self {
        Self {
            peer,
            token,
            counter: AtomicU64::new(0),
        }
    }
}

#[async_trait]
impl StreamEmitter for PeerEmitter {
    async fn emit(&self, event: StreamEvent) -> Result<(), EmitError> {
        let payload = serde_json::json!({
            "kind": STREAM_EVENT_KIND,
            "event": event,
        });
        let message =
            serde_json::to_string(&payload).map_err(|e| EmitError::Transport(e.to_string()))?;
        let progress = self.counter.fetch_add(1, Ordering::Relaxed) as f64;
        self.peer
            .notify_progress(ProgressNotificationParam {
                progress_token: self.token.clone(),
                progress,
                total: None,
                message: Some(message),
            })
            .await
            .map_err(|e| match e {
                rmcp::ServiceError::TransportClosed => EmitError::Disconnected,
                other => EmitError::Transport(other.to_string()),
            })
    }
}

#[cfg(test)]
mod mcp_tests {
    use super::*;
    use crate::{DEFAULT_MODEL, GrokProvider};
    use axum::{Json, Router, routing::get};
    use otto_protocol::ListModelsResponse;
    use serde_json::json;

    /// `list_models_tool` is the MCP wrapper around `GrokProvider::list_models`.
    /// Stand up a tiny axum mock for `/models` and verify the wrapper
    /// returns a structured `ListModelsResponse` payload with every listed
    /// (chat-capable) id present — this mock only serves chat ids, so the
    /// image/imagine/voice filter has nothing to exclude here; that
    /// filter's own behavior is covered separately in `lib.rs`'s
    /// `list_models_filters_image_imagine_and_voice_ids` test.
    #[tokio::test]
    async fn list_models_tool_returns_structured_set() {
        let app = Router::new().route(
            "/models",
            get(|| async {
                Json(json!({
                    "data": [
                        {"id": "grok-4", "object": "model"},
                        {"id": "grok-4-fast", "object": "model"}
                    ]
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let provider = GrokProvider::builder()
            .api_key("test")
            .base_url(format!("http://{addr}"))
            .build()
            .unwrap();
        let server = GrokMcpServer::new(provider);
        let result = server.list_models_tool().await.unwrap();
        assert_ne!(result.is_error, Some(true), "tool returned error result");
        let body = result
            .structured_content
            .expect("list_models_tool must populate structured_content");
        let resp: ListModelsResponse =
            serde_json::from_value(body).expect("structured payload decodes as ListModelsResponse");
        let ids: Vec<&str> = resp.models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"grok-4"), "{ids:?}");
        assert!(ids.contains(&"grok-4-fast"), "{ids:?}");
        assert_eq!(resp.default_model_id, Some(DEFAULT_MODEL.to_string()));
    }
}

//! [`ProviderClient`] implementation backed by `rmcp` Streamable HTTP.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use otto_mcp::ProviderClient;
use otto_protocol::{
    COMPLETE_TOOL_NAME, CompleteRequest, CompleteResponse, ErrorKind, LIST_MODELS_TOOL_NAME,
    ListModelsResponse, ProviderError, STREAM_EVENT_KIND, StreamEvent,
};
use rmcp::{
    ClientHandler, RoleClient, ServiceExt,
    handler::client::progress::ProgressDispatcher,
    model::{
        CallToolRequestParams, ClientInfo, Meta, NumberOrString, ProgressNotificationParam,
        ProgressToken,
    },
    service::{NotificationContext, RunningService},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use tokio::sync::mpsc;

/// `rmcp`-backed [`ProviderClient`] that holds a single Streamable HTTP MCP
/// session for the duration of the host's life.
pub struct RmcpProviderClient {
    service: RunningService<RoleClient, ProgressClient>,
    progress: ProgressDispatcher,
    counter: AtomicU64,
}

#[derive(Clone)]
struct ProgressClient {
    info: ClientInfo,
    progress: ProgressDispatcher,
}

impl ClientHandler for ProgressClient {
    fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _ctx: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        let progress = self.progress.clone();
        async move {
            progress.handle_notification(params).await;
        }
    }
    fn get_info(&self) -> ClientInfo {
        self.info.clone()
    }
}

impl RmcpProviderClient {
    /// Connect to a Streamable HTTP MCP server at `url`. Performs the MCP
    /// handshake immediately.
    pub async fn connect(url: &str) -> Result<Self> {
        let progress = ProgressDispatcher::new();
        let handler = ProgressClient {
            info: ClientInfo::default(),
            progress: progress.clone(),
        };
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(url.to_string()),
        );
        let service = handler.serve(transport).await?;
        Ok(Self {
            service,
            progress,
            counter: AtomicU64::new(0),
        })
    }

    /// Drop the MCP session.
    pub async fn shutdown(self) {
        if let Err(e) = self.service.cancel().await {
            tracing::warn!("error closing provider session: {e}");
        }
    }
}

#[async_trait]
impl ProviderClient for RmcpProviderClient {
    async fn complete(
        &self,
        req: CompleteRequest,
        events: Option<mpsc::Sender<StreamEvent>>,
    ) -> Result<CompleteResponse, ProviderError> {
        let want_stream = req.stream && events.is_some();
        let token = if want_stream {
            let n = self.counter.fetch_add(1, Ordering::Relaxed);
            Some(ProgressToken(NumberOrString::String(
                format!("otto-host-{n}").into(),
            )))
        } else {
            None
        };

        let args = match serde_json::to_value(&req)
            .map_err(|e| internal(format!("encode CompleteRequest: {e}")))?
        {
            serde_json::Value::Object(m) => m,
            _ => {
                return Err(internal(
                    "CompleteRequest did not encode as a JSON object".into(),
                ));
            }
        };
        let mut params =
            CallToolRequestParams::new(COMPLETE_TOOL_NAME.to_string()).with_arguments(args);
        if let Some(t) = token.clone() {
            params.meta = Some(Meta::with_progress_token(t));
        }

        // Subscribe BEFORE dispatching so we don't miss early progress events.
        let forwarder = if let (Some(token), Some(events)) = (&token, &events) {
            tracing::trace!(?token, "subscribing to progress");
            let subscriber = self.progress.subscribe(token.clone()).await;
            let events_tx = events.clone();
            Some(tokio::spawn(forward_progress(subscriber, events_tx)))
        } else {
            None
        };

        tracing::trace!("calling rmcp service.call_tool");
        let result = self.service.call_tool(params).await.map_err(|e| {
            tracing::warn!("call_tool errored: {e}");
            transport_error(e.to_string())
        });
        // The forwarder task parks on `subscriber.next()`, which never
        // closes on its own. Abort it now so the events sender clone it
        // holds is dropped and the caller's downstream channel can close.
        // By the time call_tool returns, the SSE response has been drained
        // and all relevant progress events are already in the channel.
        if let Some(task) = forwarder {
            task.abort();
        }
        let result = result?;
        tracing::trace!(is_error = ?result.is_error, "call_tool returned");

        if matches!(result.is_error, Some(true)) {
            return Err(parse_provider_error(result));
        }

        let resp: CompleteResponse = result
            .into_typed()
            .map_err(|e| internal(format!("decode CompleteResponse: {e}")))?;
        Ok(resp)
    }

    async fn list_models(&self) -> Result<ListModelsResponse, ProviderError> {
        let params = CallToolRequestParams::new(LIST_MODELS_TOOL_NAME.to_string());
        let result = self
            .service
            .call_tool(params)
            .await
            .map_err(|e| transport_error(e.to_string()))?;

        if matches!(result.is_error, Some(true)) {
            // Server returned a structured `ProviderError`. Surface it so the
            // host can distinguish "not advertised" (kind=Internal) from
            // genuine failures.
            return Err(parse_provider_error(result));
        }
        let v = result
            .structured_content
            .ok_or_else(|| internal("list_models returned no structured content".into()))?;
        serde_json::from_value(v).map_err(|e| internal(format!("decode ListModelsResponse: {e}")))
    }
}

async fn forward_progress(
    mut subscriber: rmcp::handler::client::progress::ProgressSubscriber,
    out: mpsc::Sender<StreamEvent>,
) {
    let mut count = 0u32;
    while let Some(p) = subscriber.next().await {
        count += 1;
        let Some(msg) = p.message else { continue };
        let v: serde_json::Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(e) => {
                tracing::trace!("dropping non-JSON progress message: {e}");
                continue;
            }
        };
        if v.get("kind").and_then(|k| k.as_str()) != Some(STREAM_EVENT_KIND) {
            continue;
        }
        match serde_json::from_value::<StreamEvent>(v["event"].clone()) {
            Ok(event) => {
                if out.send(event).await.is_err() {
                    break;
                }
            }
            Err(e) => tracing::trace!("dropping unparsable StreamEvent: {e}"),
        }
    }
    tracing::trace!(count, "forward_progress exiting");
}

fn parse_provider_error(result: rmcp::model::CallToolResult) -> ProviderError {
    if let Some(v) = result.structured_content.clone() {
        if let Ok(err) = serde_json::from_value::<ProviderError>(v) {
            return err;
        }
    }
    // Fall back to text content if structured_content is missing/malformed.
    let mut msg = String::new();
    for c in &result.content {
        if let Some(t) = c.as_text() {
            if !msg.is_empty() {
                msg.push('\n');
            }
            msg.push_str(&t.text);
        }
    }
    if msg.is_empty() {
        msg = "provider returned tool error with no payload".to_string();
    }
    ProviderError {
        kind: ErrorKind::Internal,
        message: msg,
        retry_after_ms: None,
        provider_code: None,
    }
}

fn internal(msg: String) -> ProviderError {
    ProviderError {
        kind: ErrorKind::Internal,
        message: msg,
        retry_after_ms: None,
        provider_code: None,
    }
}

fn transport_error(msg: String) -> ProviderError {
    ProviderError {
        kind: ErrorKind::Network,
        message: msg,
        retry_after_ms: None,
        provider_code: None,
    }
}

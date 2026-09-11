//! Grok Chat Completions API as a Otto SPP [`ProviderHandler`].
//!
//! Crate layout:
//!
//! - [`api`] — typed mirror of the relevant subset of Grok's
//!   `POST /chat/completions` request/response shapes.
//! - [`translate`] — pure functions converting between SPP and
//!   [`api`] types.
//! - [`stream`] — Grok SSE → SPP [`StreamEvent`](otto_protocol::StreamEvent)
//!   adapter.
//! - [`mcp`] — [`ProviderHandler`] MCP server wrapper.
//! - [`GrokProvider`] — [`ProviderHandler`] impl that wires the pieces
//!   together over an HTTP client.

#![allow(clippy::collapsible_if)] // pre-existing debt; many sites under rustc 1.95 new lint
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod api;
pub mod mcp;
pub mod stream;
pub mod translate;

pub use mcp::GrokMcpServer;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use otto_mcp::{ProviderHandler, StreamEmitter};
use otto_protocol::{
    CompleteRequest, CompleteResponse, ErrorKind, ListModelsResponse, ModelInfo, ProviderError,
    StreamEvent,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};

/// Default xAI Grok API base URL. Override via [`GrokProviderBuilder::base_url`]
/// to point at a proxy, a local mock, or a Grok-compatible endpoint.
pub const DEFAULT_BASE_URL: &str = "https://api.x.ai/v1";

/// Chat Completions endpoint path, relative to [`DEFAULT_BASE_URL`] (which
/// already carries the `/v1` prefix).
pub const CHAT_COMPLETIONS_PATH: &str = "/chat/completions";

/// Default model when none is specified by the host.
pub const DEFAULT_MODEL: &str = "grok-4";

/// SPP provider backed by Grok's Chat Completions endpoint.
pub struct GrokProvider {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

/// Builder for [`GrokProvider`]. Use [`GrokProvider::builder`].
pub struct GrokProviderBuilder {
    api_key: Option<String>,
    base_url: String,
    timeout: Duration,
}

impl GrokProvider {
    /// Start configuring an [`GrokProvider`].
    pub fn builder() -> GrokProviderBuilder {
        GrokProviderBuilder {
            api_key: None,
            base_url: DEFAULT_BASE_URL.to_string(),
            timeout: Duration::from_secs(120),
        }
    }
}

impl GrokProviderBuilder {
    /// Set the API key. If unset, [`build`](Self::build) reads `XAI_API_KEY`
    /// from the environment.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Override the API base URL (no trailing slash). Defaults to
    /// [`DEFAULT_BASE_URL`]. Set this to point at a proxy, a self-hosted
    /// Grok-compatible endpoint, or a local mock/test server.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Override the HTTP request timeout. Default is 120 s.
    pub fn timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }

    /// Build the provider, validating that an API key is available.
    pub fn build(self) -> Result<GrokProvider, BuildError> {
        let api_key = self
            .api_key
            .or_else(|| std::env::var("XAI_API_KEY").ok())
            .filter(|k| !k.is_empty())
            .ok_or(BuildError::MissingApiKey)?;
        let http = reqwest::Client::builder()
            .timeout(self.timeout)
            .https_only(self.base_url.starts_with("https://"))
            .build()
            .map_err(|e| BuildError::HttpClient(e.to_string()))?;
        Ok(GrokProvider {
            http,
            api_key,
            base_url: self.base_url,
        })
    }
}

/// [`GrokProviderBuilder::build`] failures.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// Neither the builder nor `XAI_API_KEY` provided an API key.
    #[error("XAI_API_KEY is not set and no api_key was provided")]
    MissingApiKey,
    /// The reqwest client failed to construct.
    #[error("failed to build HTTP client: {0}")]
    HttpClient(String),
}

#[async_trait]
impl ProviderHandler for GrokProvider {
    async fn list_models(&self) -> Result<ListModelsResponse, ProviderError> {
        let url = format!("{}/models", self.base_url);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .map_err(map_reqwest_error)?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(http_status_error("Grok /models", status, body));
        }
        #[derive(serde::Deserialize)]
        struct ModelsList {
            data: Vec<RawModel>,
        }
        #[derive(serde::Deserialize)]
        struct RawModel {
            id: String,
        }
        let raw: ModelsList = resp.json().await.map_err(|e| ProviderError {
            kind: ErrorKind::Internal,
            message: format!("failed to parse /models: {e}"),
            retry_after_ms: None,
            provider_code: None,
        })?;

        // Unlike DeepSeek's catalog (chat-only), xAI's `/v1/models` mixes
        // non-chat model families — image generation, "imagine", voice —
        // into the same id namespace as the chat models. Filter those out
        // by substring match on the id (case-insensitive) so `list_models`
        // only advertises chat-capable models.
        let models: Vec<ModelInfo> = raw
            .data
            .into_iter()
            .filter(|m| !is_non_chat_model_id(&m.id))
            .map(|m| ModelInfo {
                id: m.id.clone(),
                display_name: Some(m.id),
                context_window: None,
            })
            .collect();
        // Advertise DEFAULT_MODEL as the default only when it actually appears
        // in the list. Hosts treat `None` as "no default advertised".
        let default_model_id = models
            .iter()
            .any(|m| m.id == DEFAULT_MODEL)
            .then(|| DEFAULT_MODEL.to_string());
        Ok(ListModelsResponse {
            models,
            default_model_id,
        })
    }

    async fn complete(
        &self,
        req: CompleteRequest,
        emit: Option<&dyn StreamEmitter>,
    ) -> Result<CompleteResponse, ProviderError> {
        let want_stream = req.stream && emit.is_some();
        let body = translate::request_to_grok(&req, want_stream);
        let url = format!("{}{CHAT_COMPLETIONS_PATH}", self.base_url);

        tracing::trace!(%url, want_stream, "POSTing to Grok");
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(map_reqwest_error)?;
        tracing::trace!(status = %resp.status(), "Grok responded");

        if !resp.status().is_success() {
            return Err(parse_error_response(resp).await);
        }

        if want_stream {
            tracing::trace!("entering SSE consumer");
            let out = stream::consume_sse(resp, emit.unwrap()).await;
            tracing::trace!(ok = out.is_ok(), "SSE consumer returned");
            out
        } else {
            let raw: api::ChatCompletionResponse =
                resp.json().await.map_err(|e| ProviderError {
                    kind: ErrorKind::Internal,
                    message: format!("failed to parse response body: {e}"),
                    retry_after_ms: None,
                    provider_code: None,
                })?;
            Ok(translate::response_from_grok(raw))
        }
    }
}

/// Returns true when a model id names a non-chat model family — image
/// generation, "imagine", or voice — that xAI's `/v1/models` catalog mixes
/// in alongside chat-capable Grok models. Matched by substring,
/// case-insensitively, since xAI does not use a consistent id prefix for
/// these families.
fn is_non_chat_model_id(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    ["image", "imagine", "voice"]
        .iter()
        .any(|needle| lower.contains(needle))
}

fn map_reqwest_error(e: reqwest::Error) -> ProviderError {
    let kind = if e.is_timeout() || e.is_connect() {
        ErrorKind::Network
    } else {
        ErrorKind::Internal
    };
    ProviderError {
        kind,
        message: e.to_string(),
        retry_after_ms: None,
        provider_code: None,
    }
}

/// Map a Grok HTTP response status to the SPP `ErrorKind` it
/// represents. Shared by `parse_error_response` (`complete()`'s error
/// path) and `list_models`'s error path so both report the same
/// classification for the same status — previously `list_models`
/// hardcoded `ErrorKind::Network` for every non-2xx response,
/// misclassifying a bad API key (401) as a transient network error.
fn status_to_error_kind(status: u16) -> ErrorKind {
    match status {
        400 => ErrorKind::InvalidRequest,
        401 => ErrorKind::Authentication,
        403 => ErrorKind::PermissionDenied,
        404 => ErrorKind::ModelNotFound,
        413 => ErrorKind::ContextLengthExceeded,
        429 => ErrorKind::RateLimited,
        500 | 502 | 503 | 504 => ErrorKind::Overloaded,
        _ => ErrorKind::Internal,
    }
}

/// Build a [`ProviderError`] classified by HTTP status (see
/// [`status_to_error_kind`]), surfacing the response body alongside it. The
/// body is truncated at 512 bytes so a wall of JSON doesn't blow up the TUI
/// note line.
fn http_status_error(label: &str, status: reqwest::StatusCode, body: String) -> ProviderError {
    let truncated = if body.len() > 512 {
        // `body` is untrusted upstream content and may contain multi-byte
        // UTF-8 sequences; slicing at a raw byte offset can panic if 512
        // lands mid-codepoint, so walk back to the nearest char boundary.
        let mut end = 512;
        while end > 0 && !body.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &body[..end])
    } else {
        body
    };
    let message = if truncated.is_empty() {
        format!("{label} returned HTTP {status}")
    } else {
        format!("{label} returned HTTP {status}: {truncated}")
    };
    ProviderError {
        kind: status_to_error_kind(status.as_u16()),
        message,
        retry_after_ms: None,
        provider_code: None,
    }
}

async fn parse_error_response(resp: reqwest::Response) -> ProviderError {
    let status = resp.status();
    let retry_after_ms = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(|s| s * 1000);

    let kind = status_to_error_kind(status.as_u16());

    let body = resp.text().await.unwrap_or_default();
    let (message, provider_code) = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
        let msg = v
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .map(String::from)
            .unwrap_or_else(|| body.clone());
        let code = v
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(|t| t.as_str())
            .map(String::from);
        (msg, code)
    } else {
        (body, None)
    };

    ProviderError {
        kind,
        message,
        retry_after_ms,
        provider_code,
    }
}

/// Default endpoint path the Streamable HTTP server is mounted at.
pub const DEFAULT_MCP_PATH: &str = "/mcp";

/// Build the `axum::Router` that serves [`GrokMcpServer`] over MCP
/// Streamable HTTP at [`DEFAULT_MCP_PATH`].
pub fn router(provider: Arc<GrokProvider>) -> axum::Router {
    let provider_for_factory = provider.clone();
    let service = StreamableHttpService::new(
        move || Ok(GrokMcpServer::from_shared(provider_for_factory.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    axum::Router::new().nest_service(DEFAULT_MCP_PATH, service)
}

/// Default bind address for the standalone `otto-grok` binary.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:8791";

/// Run the standalone Grok MCP HTTP server. Reads `XAI_API_KEY`,
/// `OTTO_GROK_LISTEN`, and `GROK_BASE_URL` from the environment (a
/// `.env` file walking up from the CWD is honored).
pub async fn run() -> std::process::ExitCode {
    use std::env;
    use std::process::ExitCode;

    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let listen = env::var("OTTO_GROK_LISTEN").unwrap_or_else(|_| DEFAULT_LISTEN.to_string());
    let base_url = env::var("GROK_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());

    let provider = match GrokProvider::builder().base_url(base_url).build() {
        Ok(p) => Arc::new(p),
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let app = router(provider);

    let listener = match tokio::net::TcpListener::bind(&listen).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error binding {listen}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let local = listener.local_addr().expect("local_addr");
    tracing::info!(
        "otto-grok {} listening on http://{local}{DEFAULT_MCP_PATH}",
        env!("CARGO_PKG_VERSION")
    );

    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("ctrl-c received, shutting down");
    };
    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
    {
        eprintln!("server error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Build an [`GrokProvider`] aimed at a local mock server; for tests.
#[doc(hidden)]
pub fn provider_for_tests(base_url: impl Into<String>) -> GrokProvider {
    GrokProvider::builder()
        .api_key("test-key")
        .base_url(base_url)
        .build()
        .expect("test provider should build")
}

#[doc(hidden)]
pub fn _events_phantom(_: StreamEvent) {}

#[cfg(test)]
mod status_to_error_kind_tests {
    use super::*;

    #[test]
    fn maps_every_known_status_and_falls_back_to_internal() {
        assert!(matches!(
            status_to_error_kind(400),
            ErrorKind::InvalidRequest
        ));
        assert!(matches!(
            status_to_error_kind(401),
            ErrorKind::Authentication
        ));
        assert!(matches!(
            status_to_error_kind(403),
            ErrorKind::PermissionDenied
        ));
        assert!(matches!(
            status_to_error_kind(404),
            ErrorKind::ModelNotFound
        ));
        assert!(matches!(
            status_to_error_kind(413),
            ErrorKind::ContextLengthExceeded
        ));
        assert!(matches!(status_to_error_kind(429), ErrorKind::RateLimited));
        for status in [500, 502, 503, 504] {
            assert!(
                matches!(status_to_error_kind(status), ErrorKind::Overloaded),
                "status: {status}"
            );
        }
        // An unmapped/unusual status code falls through to `Internal`.
        assert!(matches!(status_to_error_kind(418), ErrorKind::Internal));
    }
}

#[cfg(test)]
mod http_status_error_tests {
    use super::*;

    /// A 512-byte truncation boundary landing mid-codepoint in a non-ASCII
    /// error body must not panic — it must back off to the nearest char
    /// boundary instead.
    #[test]
    fn truncates_multibyte_body_without_panicking() {
        // Build a body whose 512th byte falls inside a 3-byte UTF-8
        // sequence: 510 ASCII bytes followed by a run of "字" (3 bytes each).
        let mut body = "a".repeat(510);
        body.push_str(&"字".repeat(20));
        assert!(body.len() > 512);

        let err = http_status_error("test", reqwest::StatusCode::BAD_REQUEST, body);
        assert!(err.message.contains("HTTP 400"));
        assert!(err.message.contains('…'));
    }

    #[test]
    fn short_body_passes_through_unmodified() {
        let err = http_status_error(
            "test",
            reqwest::StatusCode::UNAUTHORIZED,
            "invalid_api_key".into(),
        );
        assert!(err.message.contains("invalid_api_key"));
        assert!(!err.message.contains('…'));
    }
}

#[cfg(test)]
mod list_models_tests {
    use super::*;
    use axum::{Json, Router, routing::get};
    use serde_json::json;

    #[tokio::test]
    async fn list_models_returns_all_listed_ids_unfiltered() {
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
        let resp = provider.list_models().await.unwrap();
        let ids: Vec<_> = resp.models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"grok-4"), "{ids:?}");
        assert!(ids.contains(&"grok-4-fast"), "{ids:?}");
        assert_eq!(ids.len(), 2, "{ids:?}");

        // grok-4 matches DEFAULT_MODEL so it must be advertised
        // as the default model id on the response envelope.
        assert_eq!(resp.default_model_id, Some(DEFAULT_MODEL.to_string()));
    }

    /// xAI's `/v1/models` mixes non-chat model families — image generation,
    /// "imagine", voice — into the same id namespace as the chat models.
    /// `list_models` must filter those out and return only chat-capable ids.
    #[tokio::test]
    async fn list_models_filters_image_imagine_and_voice_ids() {
        let app = Router::new().route(
            "/models",
            get(|| async {
                Json(json!({
                    "data": [
                        {"id": "grok-4", "object": "model"},
                        {"id": "grok-4-fast", "object": "model"},
                        {"id": "grok-imagine-image", "object": "model"},
                        {"id": "grok-voice-think-fast", "object": "model"}
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
        let resp = provider.list_models().await.unwrap();
        let ids: Vec<_> = resp.models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids.len(),
            2,
            "expected only the two chat-capable ids, got {ids:?}"
        );
        assert!(ids.contains(&"grok-4"), "{ids:?}");
        assert!(ids.contains(&"grok-4-fast"), "{ids:?}");
        assert!(!ids.contains(&"grok-imagine-image"), "{ids:?}");
        assert!(!ids.contains(&"grok-voice-think-fast"), "{ids:?}");
    }

    #[tokio::test]
    async fn list_models_default_model_id_none_when_default_missing() {
        // Mock that returns ids but does NOT include DEFAULT_MODEL.
        let app = Router::new().route(
            "/models",
            get(|| async {
                Json(json!({
                    "data": [
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
        let resp = provider.list_models().await.unwrap();
        assert!(!resp.models.iter().any(|m| m.id == DEFAULT_MODEL));
        assert_eq!(resp.default_model_id, None);
    }

    #[tokio::test]
    async fn list_models_propagates_http_failure() {
        let app = Router::new().route(
            "/models",
            get(|| async {
                (
                    axum::http::StatusCode::UNAUTHORIZED,
                    r#"{"error":"invalid_api_key"}"#,
                )
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
        let err = provider.list_models().await.expect_err("must fail on 401");
        assert!(
            matches!(err.kind, ErrorKind::Authentication),
            "kind: {:?}",
            err
        );
        assert!(err.message.contains("HTTP 401"), "msg: {}", err.message);
        // The response body must show up in the error so a user staring at
        // the TUI note can tell `invalid_api_key` from `model_overloaded`.
        assert!(
            err.message.contains("invalid_api_key"),
            "msg: {}",
            err.message
        );
    }

    #[tokio::test]
    async fn list_models_5xx_maps_to_overloaded() {
        let app = Router::new().route(
            "/models",
            get(|| async {
                (
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    r#"{"error":"model_overloaded"}"#,
                )
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
        let err = provider.list_models().await.expect_err("must fail on 503");
        assert!(matches!(err.kind, ErrorKind::Overloaded), "kind: {:?}", err);
    }
}

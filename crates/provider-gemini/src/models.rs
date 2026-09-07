//! Gemini `/v1beta/models` listing — filtered to models that support
//! `generateContent` and translated into SPP [`ListModelsResponse`].
//!
//! Mirrors the patterns in `provider-openai/src/lib.rs` and
//! `provider-anthropic/src/lib.rs`: GET the catalog endpoint, decode a
//! small private struct that names only the fields we care about, then
//! map into the SPP envelope.

use otto_protocol::{ErrorKind, ListModelsResponse, ModelInfo, ProviderError};
use serde::Deserialize;

use crate::{API_VERSION, GeminiProvider, map_reqwest_error, status_to_error_kind};

/// The model id we report as `default_model_id` when it appears in the
/// catalog. Keep in sync with `crates/otto/src/providers.rs`'s
/// Gemini `default_model`.
pub const DEFAULT_MODEL: &str = "gemini-2.5-flash";

#[derive(Debug, Deserialize)]
struct ModelsList {
    #[serde(default)]
    models: Vec<RawModel>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawModel {
    /// Always prefixed with `"models/"` on the wire (e.g.
    /// `"models/gemini-2.5-flash"`).
    name: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    supported_generation_methods: Vec<String>,
}

/// Query Gemini's `/v1beta/models` endpoint, filter to entries whose
/// `supportedGenerationMethods` contains `"generateContent"`, and return
/// a [`ListModelsResponse`] whose `default_model_id` is
/// [`DEFAULT_MODEL`] when present, otherwise the first surviving id.
pub async fn list_models(provider: &GeminiProvider) -> Result<ListModelsResponse, ProviderError> {
    let url = format!("{}/{API_VERSION}/models", provider.base_url);
    let resp = provider
        .http
        .get(&url)
        .header("x-goog-api-key", &provider.api_key)
        .send()
        .await
        .map_err(map_reqwest_error)?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let truncated = if body.len() > 512 {
            format!("{}…", &body[..512])
        } else {
            body
        };
        let message = if truncated.is_empty() {
            format!("Gemini /{API_VERSION}/models returned HTTP {status}")
        } else {
            format!("Gemini /{API_VERSION}/models returned HTTP {status}: {truncated}")
        };
        return Err(ProviderError {
            kind: status_to_error_kind(status.as_u16()),
            message,
            retry_after_ms: None,
            provider_code: None,
        });
    }

    let raw: ModelsList = resp.json().await.map_err(|e| ProviderError {
        kind: ErrorKind::Internal,
        message: format!("failed to parse Gemini /{API_VERSION}/models: {e}"),
        retry_after_ms: None,
        provider_code: None,
    })?;

    let models: Vec<ModelInfo> = raw
        .models
        .into_iter()
        .filter(|m| {
            m.supported_generation_methods
                .iter()
                .any(|s| s == "generateContent")
        })
        .map(|m| {
            let bare_id = m
                .name
                .strip_prefix("models/")
                .unwrap_or(&m.name)
                .to_string();
            let display_name = m.display_name.or_else(|| m.description.clone());
            ModelInfo {
                id: bare_id,
                display_name,
                context_window: None,
            }
        })
        .collect();

    let default_model_id = if models.iter().any(|m| m.id == DEFAULT_MODEL) {
        Some(DEFAULT_MODEL.to_string())
    } else {
        models.first().map(|m| m.id.clone())
    };

    Ok(ListModelsResponse {
        models,
        default_model_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GeminiProvider;
    use axum::{Json, Router, routing::get};
    use serde_json::json;

    async fn spawn_mock(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn provider_for(base: String) -> GeminiProvider {
        GeminiProvider::builder()
            .api_key("test-key")
            .base_url(base)
            .build()
            .expect("test provider must build")
    }

    #[tokio::test]
    async fn list_models_filters_to_generate_content_capable() {
        let app = Router::new().route(
            "/v1beta/models",
            get(|| async {
                Json(json!({
                    "models": [
                        {
                            "name": "models/gemini-2.5-flash",
                            "displayName": "Gemini 2.5 Flash",
                            "description": "Fast Gemini model",
                            "supportedGenerationMethods": ["generateContent", "countTokens"]
                        },
                        {
                            "name": "models/gemini-2.5-pro",
                            "displayName": "Gemini 2.5 Pro",
                            "supportedGenerationMethods": ["generateContent"]
                        },
                        {
                            "name": "models/embedding-001",
                            "displayName": "Embedding 001",
                            "supportedGenerationMethods": ["embedContent"]
                        },
                        {
                            "name": "models/text-bison",
                            "displayName": "Text Bison",
                            "supportedGenerationMethods": ["generateText"]
                        }
                    ]
                }))
            }),
        );
        let base = spawn_mock(app).await;
        let provider = provider_for(base);
        let resp = list_models(&provider).await.expect("should succeed");

        let ids: Vec<&str> = resp.models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"gemini-2.5-flash"), "{ids:?}");
        assert!(ids.contains(&"gemini-2.5-pro"), "{ids:?}");
        assert!(!ids.contains(&"embedding-001"), "{ids:?}");
        assert!(!ids.contains(&"text-bison"), "{ids:?}");
        // The bare id (no "models/" prefix) is what callers consume.
        assert!(!ids.iter().any(|id| id.starts_with("models/")), "{ids:?}");
    }

    #[tokio::test]
    async fn list_models_default_is_gemini_2_5_flash_when_present() {
        let app = Router::new().route(
            "/v1beta/models",
            get(|| async {
                Json(json!({
                    "models": [
                        {
                            "name": "models/gemini-2.5-pro",
                            "displayName": "Gemini 2.5 Pro",
                            "supportedGenerationMethods": ["generateContent"]
                        },
                        {
                            "name": "models/gemini-2.5-flash",
                            "displayName": "Gemini 2.5 Flash",
                            "supportedGenerationMethods": ["generateContent"]
                        }
                    ]
                }))
            }),
        );
        let base = spawn_mock(app).await;
        let provider = provider_for(base);
        let resp = list_models(&provider).await.expect("should succeed");
        assert_eq!(resp.default_model_id, Some(DEFAULT_MODEL.to_string()));
    }

    #[tokio::test]
    async fn list_models_default_falls_back_to_first_when_canonical_missing() {
        let app = Router::new().route(
            "/v1beta/models",
            get(|| async {
                Json(json!({
                    "models": [
                        {
                            "name": "models/gemini-2.5-pro",
                            "displayName": "Gemini 2.5 Pro",
                            "supportedGenerationMethods": ["generateContent"]
                        },
                        {
                            "name": "models/gemini-1.5-pro",
                            "displayName": "Gemini 1.5 Pro",
                            "supportedGenerationMethods": ["generateContent"]
                        }
                    ]
                }))
            }),
        );
        let base = spawn_mock(app).await;
        let provider = provider_for(base);
        let resp = list_models(&provider).await.expect("should succeed");
        assert_eq!(resp.default_model_id, Some("gemini-2.5-pro".to_string()));
    }

    #[tokio::test]
    async fn list_models_propagates_http_failure() {
        let app = Router::new().route(
            "/v1beta/models",
            get(|| async {
                (
                    axum::http::StatusCode::UNAUTHORIZED,
                    r#"{"error":{"message":"invalid api key","status":"UNAUTHENTICATED"}}"#,
                )
            }),
        );
        let base = spawn_mock(app).await;
        let provider = provider_for(base);
        let err = list_models(&provider)
            .await
            .expect_err("401 must surface as ProviderError");
        assert!(
            matches!(err.kind, ErrorKind::Authentication),
            "kind: {:?}",
            err
        );
        assert!(err.message.contains("HTTP 401"), "msg: {}", err.message);
        // The response body must show up in the error so a user staring at
        // the TUI note can tell `invalid api key` from `model_overloaded`.
        assert!(
            err.message.contains("invalid api key"),
            "msg: {}",
            err.message
        );
    }
}

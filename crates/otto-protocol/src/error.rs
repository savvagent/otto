//! Error shapes returned by provider servers when a `complete` call fails.
//!
//! Fatal errors flow back as MCP tool errors. SPP defines a small kind
//! taxonomy so hosts can react sensibly (retry, surface to user, abort).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Structured error a provider server returns inside the MCP tool error
/// payload.
#[derive(Debug, Clone, Error, Serialize, Deserialize, JsonSchema)]
#[error("{kind:?}: {message}")]
pub struct ProviderError {
    /// Coarse-grained category. Hosts should match on this.
    pub kind: ErrorKind,
    /// Human-readable detail. Forwarded as-is from the provider when
    /// available.
    pub message: String,
    /// Suggested retry delay in milliseconds, populated for
    /// [`ErrorKind::RateLimited`] and [`ErrorKind::Overloaded`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    /// Vendor-native error code, when known. Opaque to hosts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_code: Option<String>,
}

/// Coarse categorization of provider failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Request rejected by the provider as malformed (4xx).
    InvalidRequest,
    /// Authentication failed — typically a missing or bad API key.
    Authentication,
    /// Authenticated, but not allowed to use the requested model/feature.
    PermissionDenied,
    /// Model name unknown to the provider.
    ModelNotFound,
    /// The request exceeded the model's context window.
    ContextLengthExceeded,
    /// Per-minute or per-day rate limit hit. Honor `retry_after_ms`.
    RateLimited,
    /// Provider is over capacity (5xx-style). Honor `retry_after_ms`.
    Overloaded,
    /// Provider declined for safety/policy reasons.
    Refusal,
    /// Network failure between provider server and vendor API.
    Network,
    /// The provider does not implement the requested tool (e.g. `list_models`).
    /// Hosts treat this as "fall through to optimistic path" rather than a real
    /// error.
    NotImplemented,
    /// Anything else.
    Internal,
}

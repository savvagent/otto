//! Input shape for the `complete` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::content::ContentBlock;
use crate::tool::ToolDef;

/// Top-level input for the `complete` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompleteRequest {
    /// Provider-specific model identifier (e.g. `claude-sonnet-4-6`,
    /// `gpt-4o-2024-08-06`, `gemini-2.0-pro`).
    pub model: String,

    /// Conversation turns. Must be non-empty and start with a `user` role.
    /// System instructions belong in the top-level [`system`](Self::system)
    /// field, not as a synthetic turn.
    pub messages: Vec<Message>,

    /// System prompt. Translators forward this in the provider's preferred
    /// shape (`system` parameter on Anthropic, `system` role on OpenAI, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,

    /// Tool definitions the model may invoke. Provider servers translate
    /// these into the provider-native tool format.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDef>,

    /// Sampling temperature in `[0.0, 2.0]`. Provider-clamped if needed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Nucleus sampling parameter in `(0.0, 1.0]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    /// Hard cap on output tokens. Required because providers (notably
    /// Anthropic) require it, and we want one shape across all of them.
    pub max_tokens: u32,

    /// Stop sequences. Provider servers may truncate or reject if a provider
    /// has a lower limit (typically 4).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop_sequences: Vec<String>,

    /// Whether the host wants streaming. When `true`, the provider server
    /// emits MCP progress notifications carrying [`StreamEvent`](crate::StreamEvent)s
    /// before the final tool result.
    #[serde(default)]
    pub stream: bool,

    /// Whether to enable extended thinking, when the model supports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,

    /// Free-form metadata forwarded to the provider when supported. Hosts may
    /// stash conversation/session ids here for analytics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// A conversation turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Message {
    /// Who produced the turn.
    pub role: Role,

    /// One or more content blocks. Always emitted as an array on the wire,
    /// even when there is a single text block.
    pub content: Vec<ContentBlock>,
}

/// Conversation role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The user, or a tool-result-bearing turn (per Anthropic semantics,
    /// tool results are carried in a `user` turn).
    User,
    /// The model.
    Assistant,
}

/// Extended-thinking configuration, when supported by the provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ThinkingConfig {
    /// Soft budget in tokens. Provider servers clamp to provider limits.
    pub budget_tokens: u32,
}

impl CompleteRequest {
    /// Build a minimal text-only request. Convenient for tests and quick
    /// sanity checks.
    pub fn text(model: impl Into<String>, prompt: impl Into<String>, max_tokens: u32) -> Self {
        Self {
            model: model.into(),
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: prompt.into(),
                }],
            }],
            system: None,
            tools: Vec::new(),
            temperature: None,
            top_p: None,
            max_tokens,
            stop_sequences: Vec::new(),
            stream: false,
            thinking: None,
            metadata: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_request_round_trips() {
        let req = CompleteRequest::text("test-model", "hello", 64);
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "test-model");
        assert_eq!(v["max_tokens"], 64);
        assert!(
            v.get("temperature").is_none(),
            "None fields must be omitted"
        );
        assert!(v.get("tools").is_none(), "empty Vec fields must be omitted");
        let back: CompleteRequest = serde_json::from_value(v).unwrap();
        assert_eq!(back.messages.len(), 1);
    }

    #[test]
    fn rejects_unknown_fields() {
        let bad = serde_json::json!({
            "model": "x",
            "messages": [],
            "max_tokens": 1,
            "fake_field": true
        });
        let r: Result<CompleteRequest, _> = serde_json::from_value(bad);
        assert!(r.is_err(), "deny_unknown_fields must reject typos");
    }
}

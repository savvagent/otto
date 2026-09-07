//! Final response shape from the `complete` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::content::ContentBlock;

/// Final result emitted by the `complete` tool when the turn ends.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CompleteResponse {
    /// Provider-assigned message id (opaque). Hosts may surface this in
    /// debugging UIs but should not parse it.
    pub id: String,

    /// Echo of the model that produced the response. Useful when the host
    /// requested an alias and the provider resolved it.
    pub model: String,

    /// Output content blocks in order. May contain a mix of text, thinking,
    /// and tool_use blocks.
    pub content: Vec<ContentBlock>,

    /// Why generation stopped.
    pub stop_reason: StopReason,

    /// If [`stop_reason`](Self::stop_reason) is
    /// [`StopReason::StopSequence`], the matched sequence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,

    /// Cumulative usage for this turn.
    pub usage: Usage,
}

/// Reason generation halted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Model finished its turn cleanly.
    EndTurn,
    /// Model emitted one or more `tool_use` blocks; host must execute them
    /// and continue the loop.
    ToolUse,
    /// Hit the request's `max_tokens` cap.
    MaxTokens,
    /// One of the request's `stop_sequences` was emitted.
    StopSequence,
    /// Provider declined to generate (safety, policy, etc.).
    Refusal,
    /// Provider supplied a stop reason SPP does not yet model.
    Other,
}

/// Token accounting. Cache fields are populated when the provider supports
/// prompt caching (e.g., Anthropic, OpenAI).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Usage {
    /// Tokens in the prompt that were billed at full input rate.
    pub input_tokens: u32,
    /// Tokens generated.
    pub output_tokens: u32,
    /// Tokens written to the prompt cache (typically billed at a premium).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u32>,
    /// Tokens read from the prompt cache (typically billed at a discount).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
}

impl Usage {
    /// Sum of all input-side token counts (cached + uncached + cache-write).
    pub fn total_input(&self) -> u32 {
        self.input_tokens
            .saturating_add(self.cache_creation_input_tokens.unwrap_or(0))
            .saturating_add(self.cache_read_input_tokens.unwrap_or(0))
    }
}

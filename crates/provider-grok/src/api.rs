//! Typed Grok Chat Completions request and response shapes.
//!
//! Only the subset SPP needs is modeled. Field names follow xAI's
//! `snake_case` JSON convention (Grok's Chat Completions API is
//! OpenAI-wire-compatible); the streaming delta path also covers the
//! `choices[].delta` shape used in SSE responses.

#![allow(missing_docs)]

use serde::{Deserialize, Serialize};

// ---- Request types ----

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<RequestMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<String>,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
}

/// Controls whether usage is included in the final streaming chunk.
#[derive(Debug, Clone, Serialize)]
pub struct StreamOptions {
    pub include_usage: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum RequestMessage {
    System {
        content: String,
    },
    User {
        content: UserContent,
    },
    Assistant {
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<RequestToolCall>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

/// User message content — either a plain string or a multi-part array.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum UserContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: RequestFunction,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    /// JSON Schema object; SPP's schema is forwarded as-is.
    pub parameters: serde_json::Value,
}

// ---- Non-streaming response types ----

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<UsageStats>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub message: ResponseMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponseMessage {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ResponseToolCall>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponseToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ResponseFunction,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponseFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UsageStats {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
}

// ---- Streaming chunk types ----

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub model: String,
    #[serde(default)]
    pub choices: Vec<ChunkChoice>,
    /// Only present in the final chunk when `stream_options.include_usage` is
    /// true.
    #[serde(default)]
    pub usage: Option<UsageStats>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChunkChoice {
    pub delta: Delta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Delta {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<DeltaToolCall>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeltaToolCall {
    /// Index within the parallel tool-calls array. Grok uses this to
    /// identify which call a delta belongs to when multiple tools are called.
    pub index: u32,
    /// Present only on the first delta for this index.
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<DeltaFunction>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DeltaFunction {
    /// Present only on the first delta for this index.
    #[serde(default)]
    pub name: Option<String>,
    /// Partial JSON fragment; accumulate across deltas.
    #[serde(default)]
    pub arguments: Option<String>,
}

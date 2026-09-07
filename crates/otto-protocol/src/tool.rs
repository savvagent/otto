//! Tool-definition shape forwarded to providers.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A tool the model may call. Providers translate this into their native
/// tool-definition shape.
///
/// The `input_schema` is a JSON Schema (Draft 2020-12) describing the tool's
/// arguments. It is the same schema the underlying MCP tool publishes — SPP
/// passes it through unchanged so the model sees exactly what the tool
/// accepts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolDef {
    /// Tool name. Must match an MCP tool exposed to the host.
    pub name: String,
    /// Human-readable description shown to the model.
    pub description: String,
    /// JSON Schema for the tool's `input` object. Must be `type: "object"`
    /// at the top level.
    pub input_schema: serde_json::Value,
}

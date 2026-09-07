//! # Otto Provider Protocol (SPP)
//!
//! Wire format for MCP-based LLM provider servers. A *provider server* is a
//! Model Context Protocol server that exposes a single required tool,
//! [`COMPLETE_TOOL_NAME`], plus optional discovery tools. Otto (the host)
//! talks to provider servers over MCP's Streamable HTTP transport for
//! providers and stdio for tools.
//!
//! See `SPEC.md` in this crate for the human-readable specification.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod content;
pub mod error;
pub mod models;
pub mod provider_id;
pub mod request;
pub mod response;
pub mod stream;
pub mod tool;

pub use content::{ContentBlock, ImageSource, MediaType};
pub use error::{ErrorKind, ProviderError};
pub use models::{ListModelsRequest, ListModelsResponse, ModelInfo};
pub use provider_id::{ProviderId, ProviderIdError};
pub use request::{CompleteRequest, Message, Role};
pub use response::{CompleteResponse, StopReason, Usage};
pub use stream::{BlockDelta, StreamEvent, UsageDelta};
pub use tool::ToolDef;

/// Protocol semver. Bumped on breaking wire-format changes.
pub const SPP_VERSION: &str = "0.1.0";

/// Name of the required tool every provider MCP server must expose.
pub const COMPLETE_TOOL_NAME: &str = "complete";

/// Name of the optional `list_models` tool.
pub const LIST_MODELS_TOOL_NAME: &str = "list_models";

/// Name of the optional `count_tokens` tool.
pub const COUNT_TOKENS_TOOL_NAME: &str = "count_tokens";

/// MCP progress-notification `kind` discriminator for SPP stream events.
///
/// Provider servers attach SPP [`StreamEvent`]s to MCP `notifications/progress`
/// messages under `params.message` keyed by this constant so hosts can route
/// them without ambiguity.
pub const STREAM_EVENT_KIND: &str = "otto/stream-event";

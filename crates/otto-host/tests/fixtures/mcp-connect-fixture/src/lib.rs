//! Stub library target so Cargo accepts this fixture as a path dependency-like
//! package for tests that shell out to `cargo build --manifest-path ...`.

use std::{borrow::Cow, path::PathBuf, sync::Arc};

use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, Content, Implementation, JsonObject,
        ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerInfo,
        Tool,
    },
    service::RequestContext,
    transport::stdio,
};
use serde_json::json;

#[derive(Clone, Default)]
struct Fixture;

impl ServerHandler for Fixture {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::default())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        if std::env::var("MCP_FIXTURE_HANG_LIST").as_deref() == Ok("1") {
            std::future::pending::<()>().await;
        }

        let schema: JsonObject = json!({
            "type": "object",
            "properties": {
                "command": { "type": "string" }
            }
        })
        .as_object()
        .cloned()
        .expect("schema literal is an object");
        let tool = Tool::new(
            Cow::Owned(tool_name()),
            Cow::Borrowed("Return the configured environment variable. Test fixture."),
            Arc::new(schema),
        );
        Ok(ListToolsResult::with_all_items(vec![tool]))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let expected = tool_name();
        if request.name != expected {
            return Err(ErrorData::invalid_params(
                format!("unknown tool: {}", request.name),
                None,
            ));
        }
        let env_name = std::env::var("MCP_FIXTURE_RETURN_ENV").unwrap_or_else(|_| "MY_VAR".into());
        let value = std::env::var(&env_name).unwrap_or_default();
        Ok(CallToolResult::success(vec![Content::text(value)]))
    }
}

fn tool_name() -> String {
    std::env::var("MCP_FIXTURE_TOOL_NAME").unwrap_or_else(|_| "echo_env".into())
}

pub async fn run() -> anyhow::Result<()> {
    if let Ok(path) = std::env::var("MCP_FIXTURE_PID_FILE") {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, std::process::id().to_string())?;
    }

    let service = Fixture.serve(stdio()).await?;
    service.waiting().await?;
    if let Ok(path) = std::env::var("MCP_FIXTURE_EXIT_FILE") {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, "exited")?;
    }
    Ok(())
}

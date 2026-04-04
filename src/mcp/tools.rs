//! MCP tools — proxy tool for calling MCP server tools, and resource listing/reading.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::tools::{Tool, ToolContext, ToolResult};
use super::manager::McpConnectionManager;
use super::types::{self, McpToolDef};

/// Thread-safe shared reference to the MCP connection manager.
pub type SharedMcpManager = Arc<Mutex<McpConnectionManager>>;

// ── McpProxyTool ────────────────────────────────────────────────────────

/// A dynamic proxy tool that forwards calls to an MCP server tool.
///
/// One instance is created per tool discovered from connected MCP servers.
pub struct McpProxyTool {
    /// Fully-qualified tool name: `mcp__<server>__<tool>`.
    pub fq_name: String,
    /// Original tool name on the MCP server.
    pub original_name: String,
    /// Server name this tool belongs to.
    pub server_name: String,
    /// Tool description from the MCP server.
    pub tool_description: String,
    /// Tool input schema from the MCP server.
    pub tool_schema: Value,
    /// Shared connection manager for making calls.
    pub manager: SharedMcpManager,
}

#[async_trait]
impl Tool for McpProxyTool {
    fn name(&self) -> &str {
        &self.fq_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn input_schema(&self) -> Value {
        self.tool_schema.clone()
    }

    fn is_destructive(&self) -> bool {
        true // Assume MCP tools are destructive by default
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let mgr = self.manager.lock().await;
        let client = mgr.clients.get(&self.server_name).ok_or_else(|| {
            anyhow::anyhow!("MCP server '{}' not connected", self.server_name)
        })?;

        match client.call_tool(&self.original_name, input).await {
            Ok(result) => {
                let text = result
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        types::McpContent::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                if result.is_error {
                    Ok(ToolResult::err(json!({ "error": text })))
                } else {
                    Ok(ToolResult::ok(json!({ "result": text })))
                }
            }
            Err(e) => Ok(ToolResult::err(json!({
                "error": format!("MCP tool call failed: {}", e)
            }))),
        }
    }
}

// ── ListMcpResourcesTool ────────────────────────────────────────────────

/// List available resources from connected MCP servers.
pub struct ListMcpResourcesTool {
    pub manager: SharedMcpManager,
}

#[async_trait]
impl Tool for ListMcpResourcesTool {
    fn name(&self) -> &str {
        "ListMcpResourcesTool"
    }

    fn description(&self) -> &str {
        "List available resources from configured MCP servers."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "string",
                    "description": "Optional server name to filter resources"
                }
            }
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let filter_server = input["server"].as_str();
        let mgr = self.manager.lock().await;

        let mut all_resources = Vec::new();
        for (server_name, resource) in mgr.get_all_resources() {
            if let Some(filter) = filter_server {
                if server_name != filter {
                    continue;
                }
            }
            all_resources.push(json!({
                "server": server_name,
                "uri": resource.uri,
                "name": resource.name,
                "mimeType": resource.mime_type,
                "description": resource.description,
            }));
        }

        Ok(ToolResult::ok(json!(all_resources)))
    }
}

// ── ReadMcpResourceTool ─────────────────────────────────────────────────

/// Read a specific resource from an MCP server.
pub struct ReadMcpResourceTool {
    pub manager: SharedMcpManager,
}

#[async_trait]
impl Tool for ReadMcpResourceTool {
    fn name(&self) -> &str {
        "ReadMcpResourceTool"
    }

    fn description(&self) -> &str {
        "Read a specific resource from an MCP server by server name and resource URI."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "string",
                    "description": "The MCP server name"
                },
                "uri": {
                    "type": "string",
                    "description": "The resource URI to read"
                }
            },
            "required": ["server", "uri"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let server = input["server"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'server' parameter"))?;
        let uri = input["uri"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'uri' parameter"))?;

        let mgr = self.manager.lock().await;
        let client = mgr.clients.get(server).ok_or_else(|| {
            anyhow::anyhow!("MCP server '{}' not connected", server)
        })?;

        match client.read_resource(uri).await {
            Ok(contents) => {
                let result: Vec<Value> = contents
                    .iter()
                    .map(|c| match c {
                        types::McpContent::Text { text } => {
                            json!({ "type": "text", "text": text })
                        }
                        types::McpContent::Image { data, mime_type } => {
                            json!({ "type": "image", "data": data, "mimeType": mime_type })
                        }
                        types::McpContent::Unknown => {
                            json!({ "type": "unknown" })
                        }
                    })
                    .collect();
                Ok(ToolResult::ok(json!({ "contents": result })))
            }
            Err(e) => Ok(ToolResult::err(json!({
                "error": format!("Failed to read resource: {}", e)
            }))),
        }
    }
}

/// Build proxy tools from all connected MCP servers' tool definitions.
pub fn build_proxy_tools(
    manager: &SharedMcpManager,
    tools: &[(String, McpToolDef)],
) -> Vec<Box<dyn Tool + Send + Sync>> {
    tools
        .iter()
        .map(|(server_name, tool_def)| {
            let fq_name = types::build_tool_name(server_name, &tool_def.name);
            Box::new(McpProxyTool {
                fq_name,
                original_name: tool_def.name.clone(),
                server_name: server_name.clone(),
                tool_description: tool_def.description.clone(),
                tool_schema: tool_def.input_schema.clone(),
                manager: manager.clone(),
            }) as Box<dyn Tool + Send + Sync>
        })
        .collect()
}

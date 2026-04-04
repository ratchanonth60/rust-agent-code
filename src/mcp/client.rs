//! MCP client — handles the protocol handshake, tool discovery, and tool calls.

use anyhow::{anyhow, Result};
use serde_json::json;

use super::transport::StdioTransport;
use super::types::*;

/// A connected MCP client for a single server.
pub struct McpClient {
    /// Display name of the server.
    pub server_name: String,
    /// The underlying transport.
    transport: StdioTransport,
    /// Server capabilities discovered during initialization.
    pub capabilities: ServerCapabilities,
    /// Cached tool definitions.
    pub tools: Vec<McpToolDef>,
    /// Cached resource definitions.
    pub resources: Vec<McpResource>,
}

impl McpClient {
    /// Connect to an MCP server, perform the `initialize` handshake, and
    /// discover tools and resources.
    pub async fn connect(server_name: &str, config: &McpServerConfig) -> Result<Self> {
        let transport = match config {
            McpServerConfig::Stdio { command, args, env } => {
                StdioTransport::spawn(command, args, env).await?
            }
            McpServerConfig::Sse { url, .. } => {
                return Err(anyhow!("SSE transport not yet implemented for {}", url));
            }
            McpServerConfig::Http { url, .. } => {
                return Err(anyhow!(
                    "HTTP transport not yet implemented for {}",
                    url
                ));
            }
        };

        // 1. Send `initialize` request
        let init_response = transport
            .request(
                "initialize",
                Some(json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "rust-agent",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                })),
            )
            .await?;

        let capabilities = if let Some(result) = init_response.result {
            let init: InitializeResult = serde_json::from_value(result)?;
            init.capabilities
        } else {
            ServerCapabilities::default()
        };

        // 2. Send `initialized` notification
        transport.notify("notifications/initialized", None).await?;

        let mut client = Self {
            server_name: server_name.to_string(),
            transport,
            capabilities,
            tools: Vec::new(),
            resources: Vec::new(),
        };

        // 3. Discover tools
        if client.capabilities.tools.is_some() {
            client.refresh_tools().await?;
        }

        // 4. Discover resources
        if client.capabilities.resources.is_some() {
            client.refresh_resources().await?;
        }

        Ok(client)
    }

    /// Refresh the tool list from the server.
    pub async fn refresh_tools(&mut self) -> Result<()> {
        let response = self.transport.request("tools/list", None).await?;
        if let Some(result) = response.result {
            if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
                self.tools = tools
                    .iter()
                    .filter_map(|t| serde_json::from_value(t.clone()).ok())
                    .collect();
            }
        }
        Ok(())
    }

    /// Refresh the resource list from the server.
    pub async fn refresh_resources(&mut self) -> Result<()> {
        let response = self.transport.request("resources/list", None).await?;
        if let Some(result) = response.result {
            if let Some(resources) = result.get("resources").and_then(|r| r.as_array()) {
                self.resources = resources
                    .iter()
                    .filter_map(|r| serde_json::from_value(r.clone()).ok())
                    .collect();
            }
        }
        Ok(())
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolCallResult> {
        let response = self
            .transport
            .request(
                "tools/call",
                Some(json!({
                    "name": tool_name,
                    "arguments": arguments,
                })),
            )
            .await?;

        if let Some(error) = response.error {
            return Err(anyhow!(
                "MCP tool call error {}: {}",
                error.code,
                error.message
            ));
        }

        if let Some(result) = response.result {
            let call_result: McpToolCallResult = serde_json::from_value(result)?;
            Ok(call_result)
        } else {
            Err(anyhow!("MCP tool call returned no result"))
        }
    }

    /// Read a resource from the MCP server.
    pub async fn read_resource(&self, uri: &str) -> Result<Vec<McpContent>> {
        let response = self
            .transport
            .request("resources/read", Some(json!({ "uri": uri })))
            .await?;

        if let Some(error) = response.error {
            return Err(anyhow!(
                "MCP resource read error {}: {}",
                error.code,
                error.message
            ));
        }

        if let Some(result) = response.result {
            if let Some(contents) = result.get("contents").and_then(|c| c.as_array()) {
                let parsed: Vec<McpContent> = contents
                    .iter()
                    .filter_map(|c| serde_json::from_value(c.clone()).ok())
                    .collect();
                return Ok(parsed);
            }
        }

        Ok(Vec::new())
    }

    /// Gracefully disconnect from the server.
    pub async fn disconnect(self) -> Result<()> {
        self.transport.close().await
    }
}

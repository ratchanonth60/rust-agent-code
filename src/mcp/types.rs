//! MCP type definitions — server config, JSON-RPC messages, tool/resource descriptors.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// ── Server configuration ────────────────────────────────────────────────

/// Top-level MCP configuration file format.
///
/// Stored at `~/.rust-agent/mcp.json` or `.rust-agent/mcp.json` in the project.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpJsonConfig {
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

/// Configuration for a single MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpServerConfig {
    /// Stdio-based server (spawns a subprocess).
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    /// SSE (Server-Sent Events) remote server.
    Sse {
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
    },
    /// Streamable HTTP remote server.
    Http {
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
    },
}

// ── JSON-RPC 2.0 ────────────────────────────────────────────────────────

/// A JSON-RPC 2.0 request message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// A JSON-RPC 2.0 response message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// ── MCP protocol types ──────────────────────────────────────────────────

/// An MCP tool descriptor returned by `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "inputSchema")]
    pub input_schema: Value,
}

/// An MCP resource descriptor returned by `resources/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "mimeType")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Result of a `tools/call` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCallResult {
    #[serde(default)]
    pub content: Vec<McpContent>,
    #[serde(default, rename = "isError")]
    pub is_error: bool,
}

/// Content block within MCP tool/resource responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpContent {
    Text { text: String },
    #[serde(rename = "image")]
    Image { data: String, mime_type: String },
    #[serde(other)]
    Unknown,
}

/// Server capabilities returned during initialization.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerCapabilities {
    #[serde(default)]
    pub tools: Option<Value>,
    #[serde(default)]
    pub resources: Option<Value>,
    #[serde(default)]
    pub prompts: Option<Value>,
}

/// The `initialize` response result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    #[serde(default)]
    pub capabilities: ServerCapabilities,
    #[serde(default, rename = "serverInfo")]
    pub server_info: Option<Value>,
}

// ── Name normalization ──────────────────────────────────────────────────

/// Normalize a server or tool name for use in tool dispatch names.
///
/// Replaces non-alphanumeric characters (except `-` and `_`) with `_`,
/// matching the MCP SDK's `^[a-zA-Z0-9_-]{1,64}$` requirement.
pub fn normalize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect()
}

/// Build a fully-qualified tool name: `mcp__<server>__<tool>`.
pub fn build_tool_name(server_name: &str, tool_name: &str) -> String {
    format!(
        "mcp__{}__{}",
        normalize_name(server_name),
        normalize_name(tool_name)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_simple() {
        assert_eq!(normalize_name("my-tool"), "my-tool");
        assert_eq!(normalize_name("my tool"), "my_tool");
        assert_eq!(normalize_name("tool.v2"), "tool_v2");
    }

    #[test]
    fn build_tool_name_format() {
        assert_eq!(
            build_tool_name("my-server", "list_files"),
            "mcp__my-server__list_files"
        );
    }

    #[test]
    fn config_deserialize_stdio() {
        let json = r#"{"type": "stdio", "command": "npx", "args": ["-y", "@mcp/server"]}"#;
        let config: McpServerConfig = serde_json::from_str(json).unwrap();
        match config {
            McpServerConfig::Stdio { command, args, .. } => {
                assert_eq!(command, "npx");
                assert_eq!(args, vec!["-y", "@mcp/server"]);
            }
            _ => panic!("Expected Stdio config"),
        }
    }
}

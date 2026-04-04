//! MCP connection manager — connects to all configured servers and aggregates tools/resources.

use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use super::client::McpClient;
use super::types::{McpJsonConfig, McpResource, McpServerConfig, McpToolDef};

/// Manages connections to multiple MCP servers.
pub struct McpConnectionManager {
    /// Connected clients keyed by server name.
    pub clients: HashMap<String, McpClient>,
}

impl McpConnectionManager {
    /// Create an empty connection manager.
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }

    /// Load MCP config from the standard paths and connect to all servers.
    ///
    /// Config is searched at:
    /// 1. `<cwd>/.rust-agent/mcp.json`
    /// 2. `~/.rust-agent/mcp.json`
    pub async fn connect_from_config(cwd: &Path) -> Result<Self> {
        let mut manager = Self::new();

        let config_paths = vec![
            cwd.join(".rust-agent").join("mcp.json"),
            dirs::home_dir()
                .unwrap_or_default()
                .join(".rust-agent")
                .join("mcp.json"),
        ];

        for path in &config_paths {
            if path.exists() {
                let content = std::fs::read_to_string(path)?;
                let config: McpJsonConfig = serde_json::from_str(&content)?;
                for (name, server_config) in &config.mcp_servers {
                    if manager.clients.contains_key(name) {
                        continue; // local config takes precedence
                    }
                    match manager.connect_server(name, server_config).await {
                        Ok(()) => {
                            tracing::info!("Connected to MCP server: {}", name);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to connect to MCP server '{}': {}", name, e);
                        }
                    }
                }
            }
        }

        Ok(manager)
    }

    /// Connect to a single MCP server.
    pub async fn connect_server(&mut self, name: &str, config: &McpServerConfig) -> Result<()> {
        let client = McpClient::connect(name, config).await?;
        self.clients.insert(name.to_string(), client);
        Ok(())
    }

    /// Disconnect from a specific server.
    pub async fn disconnect_server(&mut self, name: &str) -> Result<()> {
        if let Some(client) = self.clients.remove(name) {
            client.disconnect().await?;
        }
        Ok(())
    }

    /// Get all tool definitions from all connected servers.
    pub fn get_all_tools(&self) -> Vec<(String, McpToolDef)> {
        let mut tools = Vec::new();
        for (server_name, client) in &self.clients {
            for tool in &client.tools {
                tools.push((server_name.clone(), tool.clone()));
            }
        }
        tools
    }

    /// Get all resource definitions from all connected servers.
    pub fn get_all_resources(&self) -> Vec<(String, McpResource)> {
        let mut resources = Vec::new();
        for (server_name, client) in &self.clients {
            for resource in &client.resources {
                resources.push((server_name.clone(), resource.clone()));
            }
        }
        resources
    }

    /// Disconnect from all servers.
    pub async fn disconnect_all(&mut self) -> Result<()> {
        let names: Vec<String> = self.clients.keys().cloned().collect();
        for name in names {
            self.disconnect_server(&name).await?;
        }
        Ok(())
    }
}

impl Default for McpConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

//! `/mcp` — manage MCP server connections.

use super::types::{Command, CommandContext, CommandResult, CommandType};

/// List, connect, or disconnect MCP servers.
pub struct McpCommand;

impl Command for McpCommand {
    fn name(&self) -> &str {
        "mcp"
    }

    fn description(&self) -> &str {
        "Manage MCP server connections"
    }

    fn argument_hint(&self) -> Option<&str> {
        Some("[list|connect|disconnect] [name]")
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, args: &str, ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let parts: Vec<&str> = args.split_whitespace().collect();
        let subcmd = parts.first().copied().unwrap_or("list");

        match subcmd {
            "list" | "" => {
                // Read MCP config files to show configured servers.
                let config_paths = vec![
                    ctx.cwd.join(".rust-agent").join("mcp.json"),
                    dirs::home_dir()
                        .unwrap_or_default()
                        .join(".rust-agent")
                        .join("mcp.json"),
                ];

                let mut servers: Vec<String> = Vec::new();
                for path in &config_paths {
                    if path.exists() {
                        if let Ok(content) = std::fs::read_to_string(path) {
                            if let Ok(config) = serde_json::from_str::<crate::mcp::types::McpJsonConfig>(&content) {
                                for name in config.mcp_servers.keys() {
                                    if !servers.contains(name) {
                                        servers.push(name.clone());
                                    }
                                }
                            }
                        }
                    }
                }

                if servers.is_empty() {
                    return Ok(CommandResult::Text(
                        "  No MCP servers configured.\n  \
                         Add servers to ~/.rust-agent/mcp.json"
                            .to_string(),
                    ));
                }

                let mut lines = Vec::new();
                lines.push("  MCP Servers (configured)".to_string());
                lines.push("  ───────────────────────".to_string());
                for name in &servers {
                    lines.push(format!("    {}", name));
                }
                lines.push(String::new());
                lines.push("  Servers connect automatically at engine startup.".to_string());
                Ok(CommandResult::Text(lines.join("\n")))
            }
            "connect" => {
                let name = parts.get(1).copied().unwrap_or("");
                if name.is_empty() {
                    return Ok(CommandResult::Text(
                        "  Usage: /mcp connect <server-name>\n  \
                         Note: MCP servers auto-connect at startup from ~/.rust-agent/mcp.json"
                            .to_string(),
                    ));
                }
                Ok(CommandResult::Text(format!(
                    "  MCP server '{}' will connect on next engine restart.\n  \
                     Add it to ~/.rust-agent/mcp.json if not already configured.",
                    name
                )))
            }
            "disconnect" => {
                let name = parts.get(1).copied().unwrap_or("");
                if name.is_empty() {
                    return Ok(CommandResult::Text(
                        "  Usage: /mcp disconnect <server-name>".to_string(),
                    ));
                }
                Ok(CommandResult::Text(format!(
                    "  Runtime disconnect for '{}' requires engine integration.\n  \
                     Remove from ~/.rust-agent/mcp.json to disable permanently.",
                    name
                )))
            }
            _ => Ok(CommandResult::Text(format!(
                "  Unknown subcommand: '{}'\n  Usage: /mcp [list|connect|disconnect] [name]",
                subcmd
            ))),
        }
    }
}

//! Team collaboration tools — create, delete, and message team channels.
//!
//! Tools use local file-backed storage at `~/.rust-agent/teams/`.
//! Each team is a JSON file containing an array of [`TeamMessage`]s.

pub mod manager;
pub mod types;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tools::{Tool, ToolContext, ToolResult};

// ── CreateTeamTool ──────────────────────────────────────────────────

/// Create a new team channel for collaboration.
pub struct CreateTeamTool;

#[async_trait]
impl Tool for CreateTeamTool {
    fn name(&self) -> &str {
        "CreateTeam"
    }

    fn description(&self) -> &str {
        "Create a new team channel. Teams are local message stores for collaboration."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Team channel name (alphanumeric, hyphens, underscores)"
                },
                "description": {
                    "type": "string",
                    "description": "Optional description for the team"
                }
            },
            "required": ["name"]
        })
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let name = input["name"].as_str().unwrap_or("").trim();
        if name.is_empty() {
            return Ok(ToolResult::err(json!({"error": "Team name is required"})));
        }

        // Validate name: alphanumeric, hyphens, underscores only
        if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return Ok(ToolResult::err(json!({
                "error": "Team name must contain only alphanumeric characters, hyphens, and underscores"
            })));
        }

        let description = input["description"].as_str();

        match manager::create_team(name, description) {
            Ok(info) => Ok(ToolResult::ok(json!({
                "created": true,
                "name": info.name,
                "description": info.description,
            }))),
            Err(e) => Ok(ToolResult::err(json!({"error": e.to_string()}))),
        }
    }

    fn is_destructive(&self) -> bool {
        true
    }
}

// ── DeleteTeamTool ──────────────────────────────────────────────────

/// Delete a team channel and all its messages.
pub struct DeleteTeamTool;

#[async_trait]
impl Tool for DeleteTeamTool {
    fn name(&self) -> &str {
        "DeleteTeam"
    }

    fn description(&self) -> &str {
        "Delete a team channel and all its messages permanently."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the team to delete"
                }
            },
            "required": ["name"]
        })
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let name = input["name"].as_str().unwrap_or("").trim();
        if name.is_empty() {
            return Ok(ToolResult::err(json!({"error": "Team name is required"})));
        }

        match manager::delete_team(name) {
            Ok(()) => Ok(ToolResult::ok(json!({
                "deleted": true,
                "name": name
            }))),
            Err(e) => Ok(ToolResult::err(json!({"error": e.to_string()}))),
        }
    }

    fn is_destructive(&self) -> bool {
        true
    }
}

// ── SendTeamMessageTool ─────────────────────────────────────────────

/// Send a message to a team channel.
pub struct SendTeamMessageTool;

#[async_trait]
impl Tool for SendTeamMessageTool {
    fn name(&self) -> &str {
        "SendTeamMessage"
    }

    fn description(&self) -> &str {
        "Send a message to a team channel. Use this for team collaboration and sharing context."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "team": {
                    "type": "string",
                    "description": "Name of the team to send the message to"
                },
                "author": {
                    "type": "string",
                    "description": "Author name (defaults to 'agent')"
                },
                "content": {
                    "type": "string",
                    "description": "Message content"
                }
            },
            "required": ["team", "content"]
        })
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let team = input["team"].as_str().unwrap_or("").trim();
        let author = input["author"].as_str().unwrap_or("agent").trim();
        let content = input["content"].as_str().unwrap_or("").trim();

        if team.is_empty() {
            return Ok(ToolResult::err(json!({"error": "Team name is required"})));
        }
        if content.is_empty() {
            return Ok(ToolResult::err(json!({"error": "Message content is required"})));
        }

        match manager::send_message(team, author, content) {
            Ok(msg) => Ok(ToolResult::ok(json!({
                "sent": true,
                "id": msg.id,
                "team": msg.team,
                "author": msg.author,
                "timestamp": msg.timestamp,
            }))),
            Err(e) => Ok(ToolResult::err(json!({"error": e.to_string()}))),
        }
    }
}

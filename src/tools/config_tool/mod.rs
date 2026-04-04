//! ConfigTool — read and write global agent configuration.
//!
//! Provides LLM access to the persistent `~/.rust-agent/config.json`
//! file so the agent can adjust its own settings during a session.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::config::GlobalConfig;
use crate::tools::{Tool, ToolContext, ToolResult};

/// Read or write the agent's global configuration.
pub struct ConfigTool;

#[async_trait]
impl Tool for ConfigTool {
    fn name(&self) -> &str {
        "ConfigTool"
    }

    fn description(&self) -> &str {
        "Read or write the agent's global configuration. \
         Without arguments, returns the current config. \
         With 'key' and 'value', updates a specific setting."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["read", "write"],
                    "description": "Action to perform (default: read)"
                },
                "key": {
                    "type": "string",
                    "description": "Config key to read/write (e.g. 'editor_mode', 'theme', 'default_model')"
                },
                "value": {
                    "type": "string",
                    "description": "Value to set (required for write action)"
                }
            }
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let action = input["action"].as_str().unwrap_or("read");

        match action {
            "read" => {
                let cfg = GlobalConfig::load();
                let key = input["key"].as_str();

                if let Some(key) = key {
                    let value = match key {
                        "editor_mode" => json!(format!("{:?}", cfg.editor_mode)),
                        "theme" => json!(cfg.theme),
                        "default_model" => json!(cfg.default_model),
                        "default_provider" => json!(cfg.default_provider),
                        "output_style" => json!(cfg.output_style),
                        _ => {
                            return Ok(ToolResult::err(json!({
                                "error": format!("Unknown config key: '{}'", key)
                            })));
                        }
                    };
                    Ok(ToolResult::ok(json!({ "key": key, "value": value })))
                } else {
                    Ok(ToolResult::ok(json!({
                        "config": {
                            "editor_mode": format!("{:?}", cfg.editor_mode),
                            "theme": cfg.theme,
                            "default_model": cfg.default_model,
                            "default_provider": cfg.default_provider,
                            "output_style": cfg.output_style,
                        }
                    })))
                }
            }
            "write" => {
                let key = input["key"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'key' for write action"))?;
                let value = input["value"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'value' for write action"))?;

                let mut cfg = GlobalConfig::load();

                match key {
                    "editor_mode" => {
                        cfg.editor_mode = match value.to_lowercase().as_str() {
                            "vim" => crate::config::EditorMode::Vim,
                            "normal" | "default" => crate::config::EditorMode::Normal,
                            _ => {
                                return Ok(ToolResult::err(json!({
                                    "error": format!("Invalid editor_mode: '{}'. Use 'normal' or 'vim'.", value)
                                })));
                            }
                        };
                    }
                    "theme" => {
                        cfg.theme = if value.is_empty() {
                            "default".to_string()
                        } else {
                            value.to_string()
                        };
                    }
                    "default_model" => {
                        cfg.default_model = if value.is_empty() {
                            None
                        } else {
                            Some(value.to_string())
                        };
                    }
                    "default_provider" => {
                        cfg.default_provider = if value.is_empty() {
                            None
                        } else {
                            Some(value.to_string())
                        };
                    }
                    "output_style" => {
                        cfg.output_style = if value == "none" || value.is_empty() {
                            None
                        } else {
                            Some(value.to_string())
                        };
                    }
                    _ => {
                        return Ok(ToolResult::err(json!({
                            "error": format!("Unknown config key: '{}'", key)
                        })));
                    }
                }

                cfg.save()?;

                Ok(ToolResult::ok(json!({
                    "status": "updated",
                    "key": key,
                    "value": value
                })))
            }
            _ => Ok(ToolResult::err(json!({
                "error": format!("Unknown action: '{}'. Use 'read' or 'write'.", action)
            }))),
        }
    }
}

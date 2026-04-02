//! [`AgentTool`] — a tool that spawns a sub-agent to handle complex sub-tasks.
//!
//! The sub-agent is a fresh [`QueryEngine`] instance with the same model and
//! configuration as the parent, but **without** `AgentTool` registered.  This
//! prevents infinite recursion while still allowing the parent agent to
//! delegate work to an isolated child context.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::engine::config::EngineConfig;
use crate::engine::query::{ModelProvider, QueryEngine};
use crate::tools::{Tool, ToolContext, ToolResult};

/// Spawns a sub-agent for complex delegated tasks.
///
/// The sub-agent runs in a fresh [`QueryEngine`] context (fresh message
/// history, no `AgentTool` registered) and returns its final text answer.
pub struct AgentTool {
    model: String,
    provider: ModelProvider,
    config: EngineConfig,
}

impl AgentTool {
    /// Create an `AgentTool` with the same model/provider/config as the parent engine.
    pub fn new(model: String, provider: ModelProvider, config: EngineConfig) -> Self {
        Self {
            model,
            provider,
            config,
        }
    }
}

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self) -> &str {
        "Launch a specialized sub-agent to handle a complex, multi-step task autonomously. \
         The sub-agent has access to all standard tools (Read, Write, Bash, Glob, Grep, etc.) \
         but runs in a fresh context and cannot spawn further sub-agents. \
         Use this when a task benefits from isolated execution or parallelism."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["prompt"],
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Full task description for the sub-agent."
                },
                "description": {
                    "type": "string",
                    "description": "Short (3–5 word) label shown in the UI while the sub-agent runs."
                }
            }
        })
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let prompt = match input["prompt"].as_str() {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => return Ok(ToolResult::err(json!({"error": "prompt is required"}))),
        };

        // Build a sub-agent without AgentTool to prevent infinite recursion.
        let mut sub_config = self.config.clone();
        sub_config.auto_mode = true; // sub-agents run non-interactively

        let sub_engine =
            QueryEngine::new(self.model.clone(), self.provider, None, None, sub_config)?;

        match sub_engine.query(&prompt, None).await {
            Ok(result) => Ok(ToolResult::ok(json!({"result": result}))),
            Err(e) => Ok(ToolResult::err(json!({"error": e.to_string()}))),
        }
    }
}

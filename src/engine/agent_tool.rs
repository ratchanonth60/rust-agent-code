//! [`AgentTool`] — a tool that spawns a sub-agent to handle complex sub-tasks.
//!
//! The sub-agent is a fresh [`QueryEngine`] instance with the same model and
//! configuration as the parent, but **without** `AgentTool` registered.  This
//! prevents infinite recursion while still allowing the parent agent to
//! delegate work to an isolated child context.
//!
//! Agent tasks are registered in the unified [`TaskRegistry`] for status
//! tracking and TUI pill display.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::engine::config::EngineConfig;
use crate::engine::query::{ModelProvider, QueryEngine};
use crate::tasks::SharedTaskRegistry;
use crate::tools::{Tool, ToolContext, ToolResult};

/// Spawns a sub-agent for complex delegated tasks.
///
/// The sub-agent runs in a fresh [`QueryEngine`] context (fresh message
/// history, no `AgentTool` registered) and returns its final text answer.
/// The task is tracked in the [`TaskRegistry`](crate::tasks::TaskRegistry)
/// so the TUI can display a running task count.
pub struct AgentTool {
    model: String,
    provider: ModelProvider,
    config: EngineConfig,
    registry: SharedTaskRegistry,
}

impl AgentTool {
    /// Create an `AgentTool` with the same model/provider/config as the parent engine.
    pub fn new(
        model: String,
        provider: ModelProvider,
        config: EngineConfig,
        registry: SharedTaskRegistry,
    ) -> Self {
        Self {
            model,
            provider,
            config,
            registry,
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

        let description = input["description"]
            .as_str()
            .unwrap_or("sub-agent")
            .to_string();

        let agent_id = uuid::Uuid::new_v4().to_string();

        // Register the agent task before execution.
        let task_id = crate::tasks::agent::register(
            &self.registry,
            &prompt,
            &agent_id,
            &description,
        )
        .unwrap_or_default();

        // Build a sub-agent without AgentTool to prevent infinite recursion.
        let mut sub_config = self.config.clone();
        sub_config.auto_mode = true; // sub-agents run non-interactively

        let sub_engine =
            QueryEngine::new(self.model.clone(), self.provider, None, None, sub_config, None)?;

        match sub_engine.query(&prompt, None).await {
            Ok(result) => {
                crate::tasks::agent::complete(&self.registry, &task_id, &result);
                Ok(ToolResult::ok(json!({"result": result})))
            }
            Err(e) => {
                let err_msg = e.to_string();
                crate::tasks::agent::fail(&self.registry, &task_id, &err_msg);
                Ok(ToolResult::err(json!({"error": err_msg})))
            }
        }
    }
}

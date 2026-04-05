//! Task management tools — background task creation, output retrieval, and stopping.
//!
//! Provides three LLM-accessible tools:
//! - [`BackgroundTaskTool`] — spawn a background shell command
//! - [`TaskOutputTool`] — read stdout/stderr from a running or completed task
//! - [`TaskStopTool`] — terminate a running background task
//!
//! All tools operate on the unified [`TaskRegistry`](crate::tasks::TaskRegistry)
//! via [`SharedTaskRegistry`](crate::tasks::SharedTaskRegistry).

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tasks::SharedTaskRegistry;
use crate::tools::{Tool, ToolContext, ToolResult};

// ── BackgroundTaskTool ──────────────────────────────────────────────────

/// Spawn a shell command as a background task.
pub struct BackgroundTaskTool {
    pub registry: SharedTaskRegistry,
}

#[async_trait]
impl Tool for BackgroundTaskTool {
    fn name(&self) -> &str {
        "BackgroundTask"
    }

    fn description(&self) -> &str {
        "Run a shell command in the background. Returns a task_id to retrieve output later."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to run in the background"
                },
                "description": {
                    "type": "string",
                    "description": "Short description of what this task does"
                }
            },
            "required": ["command"]
        })
    }

    fn is_destructive(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let command = input["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' parameter"))?;
        let description = input["description"].as_str().unwrap_or("");

        let task_id = crate::tasks::shell::spawn(&self.registry, command, description, None)?;

        Ok(ToolResult::ok(json!({
            "task_id": task_id,
            "status": "running",
            "message": format!("Background task started: {}", command)
        })))
    }
}

// ── TaskOutputTool ──────────────────────────────────────────────────────

/// Retrieve output from a background task.
pub struct TaskOutputTool {
    pub registry: SharedTaskRegistry,
}

#[async_trait]
impl Tool for TaskOutputTool {
    fn name(&self) -> &str {
        "TaskOutput"
    }

    fn description(&self) -> &str {
        "Get output from a running or completed background task."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The task ID returned by BackgroundTask"
                }
            },
            "required": ["task_id"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let task_id = input["task_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'task_id' parameter"))?;

        // Use collect_output to poll the child process (fixes stale-data bug).
        match crate::tasks::shell::collect_output(&self.registry, task_id) {
            Some((status, stdout, stderr)) => {
                let status_str = format!("{:?}", status).to_lowercase();
                Ok(ToolResult::ok(json!({
                    "task_id": task_id,
                    "status": status_str,
                    "stdout": stdout,
                    "stderr": stderr,
                })))
            }
            None => Ok(ToolResult::err(json!({
                "error": format!("Task '{}' not found", task_id)
            }))),
        }
    }
}

// ── TaskStopTool ────────────────────────────────────────────────────────

/// Stop a running background task.
pub struct TaskStopTool {
    pub registry: SharedTaskRegistry,
}

#[async_trait]
impl Tool for TaskStopTool {
    fn name(&self) -> &str {
        "TaskStop"
    }

    fn description(&self) -> &str {
        "Stop a running background task by its task_id."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The task ID to stop"
                }
            },
            "required": ["task_id"]
        })
    }

    fn is_destructive(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let task_id = input["task_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'task_id' parameter"))?;

        match crate::tasks::stop::stop_task(&self.registry, task_id) {
            Ok(()) => Ok(ToolResult::ok(json!({
                "task_id": task_id,
                "status": "killed"
            }))),
            Err(e) => Ok(ToolResult::err(json!({
                "error": e.to_string()
            }))),
        }
    }
}

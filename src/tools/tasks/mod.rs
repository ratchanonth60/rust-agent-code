//! Task management tools — background task creation, output retrieval, and stopping.
//!
//! Provides three LLM-accessible tools:
//! - [`BackgroundTaskTool`] — spawn a background shell command
//! - [`TaskOutputTool`] — read stdout/stderr from a running or completed task
//! - [`TaskStopTool`] — terminate a running background task
//!
//! Tasks are tracked in a [`TaskManager`] wrapped in `Arc<Mutex<..>>`.

pub mod manager;

use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

use crate::tools::{Tool, ToolContext, ToolResult};
use manager::{TaskManager, TaskStatus};

/// Thread-safe shared reference to the task manager.
pub type SharedTaskManager = Arc<Mutex<TaskManager>>;

/// Create a new shared task manager.
pub fn new_shared_task_manager() -> SharedTaskManager {
    Arc::new(Mutex::new(TaskManager::new()))
}

// ── BackgroundTaskTool ──────────────────────────────────────────────────

/// Spawn a shell command as a background task.
pub struct BackgroundTaskTool {
    pub manager: SharedTaskManager,
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

        let task_id = {
            let mut mgr = self
                .manager
                .lock()
                .map_err(|e| anyhow::anyhow!("Task manager lock error: {}", e))?;
            mgr.spawn(command, description)?
        };

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
    pub manager: SharedTaskManager,
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

        let mgr = self
            .manager
            .lock()
            .map_err(|e| anyhow::anyhow!("Task manager lock error: {}", e))?;

        match mgr.get_output(task_id) {
            Some((status, stdout, stderr)) => {
                let status_str = match status {
                    TaskStatus::Running => "running",
                    TaskStatus::Completed => "completed",
                    TaskStatus::Failed => "failed",
                    TaskStatus::Stopped => "stopped",
                };
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
    pub manager: SharedTaskManager,
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

        let mut mgr = self
            .manager
            .lock()
            .map_err(|e| anyhow::anyhow!("Task manager lock error: {}", e))?;

        if mgr.stop(task_id) {
            Ok(ToolResult::ok(json!({
                "task_id": task_id,
                "status": "stopped"
            })))
        } else {
            Ok(ToolResult::err(json!({
                "error": format!("Task '{}' not found or already stopped", task_id)
            })))
        }
    }
}

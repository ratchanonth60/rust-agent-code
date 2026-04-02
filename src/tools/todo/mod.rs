//! Task tracking tool with shared state.
//!
//! The LLM sends a full replacement array of [`TodoItem`]s via
//! [`TodoWriteTool`].  The shared list is accessible from the TUI
//! for rendering a progress sidebar.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

use crate::tools::{Tool, ToolContext, ToolResult};

/// A single todo item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: String, // "pending", "in_progress", "completed"
    #[serde(rename = "activeForm")]
    pub active_form: String,
}

/// Shared todo state, accessible from the engine and TUI.
pub type SharedTodoList = Arc<Mutex<Vec<TodoItem>>>;

/// Creates a new shared todo list.
pub fn new_shared_todo_list() -> SharedTodoList {
    Arc::new(Mutex::new(Vec::new()))
}

/// Replaces the entire todo list with the LLM-provided array.
pub struct TodoWriteTool {
    pub todos: SharedTodoList,
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "TodoWrite"
    }

    fn description(&self) -> &str {
        "Create and manage a structured task list for tracking progress. \
         Input is a full replacement array of todos, each with content (imperative), \
         status (pending/in_progress/completed), and activeForm (present continuous)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": { "type": "string", "description": "The task description in imperative form" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "completed"] },
                            "activeForm": { "type": "string", "description": "Present continuous form of the task" }
                        },
                        "required": ["content", "status", "activeForm"]
                    }
                }
            },
            "required": ["todos"]
        })
    }

    fn is_destructive(&self) -> bool { false }
    fn is_read_only(&self) -> bool { false }
    fn is_concurrency_safe(&self) -> bool { true }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let todos_val = input.get("todos")
            .ok_or_else(|| anyhow::anyhow!("Missing 'todos' field"))?;

        let new_todos: Vec<TodoItem> = serde_json::from_value(todos_val.clone())?;

        if let Ok(mut list) = self.todos.lock() {
            *list = new_todos.clone();
        }

        Ok(ToolResult::ok(json!({
            "status": "ok",
            "count": new_todos.len()
        })))
    }
}

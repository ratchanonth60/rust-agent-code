use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::Path;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Deserialized input for [`ReadFileTool`].
#[derive(Deserialize)]
pub struct ReadFileInput {
    /// Absolute or relative path to the file.
    pub path: String,
}

/// Reads the entire contents of a text file and returns it as JSON.
///
/// Marked as read-only — safe for concurrent execution.
pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str { "read_file" }

    fn description(&self) -> &str {
        "Read the contents of a file at a specified path."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The absolute or relative path to the file to read."
                }
            },
            "required": ["path"]
        })
    }

    /// Reads the file and returns `{"content": "..."}`.
    ///
    /// I/O failures are returned as `ToolResult::err` (not hard errors)
    /// so the LLM can react to missing files gracefully.
    async fn call(&self, input: Value, _context: &ToolContext) -> anyhow::Result<ToolResult> {
        let params: ReadFileInput = serde_json::from_value(input)?;

        let file_path = Path::new(&params.path);
        match fs::read_to_string(file_path) {
            Ok(content) => Ok(ToolResult::ok(serde_json::json!({
                "content": content
            }))),
            Err(e) => Ok(ToolResult::err(serde_json::json!({
                "error": format!("Failed to read file '{}': {}", params.path, e)
            }))),
        }
    }

    fn is_read_only(&self) -> bool { true }
}

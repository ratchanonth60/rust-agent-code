use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::Path;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Deserialized input for [`WriteFileTool`].
#[derive(Deserialize)]
pub struct WriteFileInput {
    /// Target file path.
    pub path: String,
    /// Text content to write.
    pub content: String,
    /// Must be `true` to overwrite an existing file.
    pub overwrite: Option<bool>,
}

/// Writes text content to a file, creating parent directories as needed.
///
/// Refuses to overwrite an existing file unless `overwrite` is explicitly
/// set to `true`, preventing accidental data loss.
pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str { "write_file" }

    fn description(&self) -> &str {
        "Write text content to a file at a specified path. Use overwrite=true to replace an existing file."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The absolute or relative path to the file to write to."
                },
                "content": {
                    "type": "string",
                    "description": "The content to write into the file."
                },
                "overwrite": {
                    "type": "boolean",
                    "description": "Set to true to overwrite the file if it already exists."
                }
            },
            "required": ["path", "content"]
        })
    }

    /// Writes `content` to `path`.
    ///
    /// # Errors
    ///
    /// Returns `ToolResult::err` (not a hard error) when:
    /// - The file already exists and `overwrite` is not `true`.
    /// - Parent directory creation fails.
    /// - The write itself fails (permissions, disk full, etc.).
    async fn call(&self, input: Value, _context: &ToolContext) -> anyhow::Result<ToolResult> {
        let params: WriteFileInput = serde_json::from_value(input)?;
        let file_path = Path::new(&params.path);

        if file_path.exists() && params.overwrite != Some(true) {
            return Ok(ToolResult::err(serde_json::json!({
                "error": format!("File '{}' already exists. Pass overwrite=true to overwrite it.", params.path)
            })));
        }

        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    return Ok(ToolResult::err(serde_json::json!({
                        "error": format!("Failed to create parent directory: {}", e)
                    })));
                }
            }
        }

        match fs::write(file_path, &params.content) {
            Ok(_) => Ok(ToolResult::ok(serde_json::json!({
                "success": true,
                "message": format!("Successfully wrote {} bytes to '{}'", params.content.len(), params.path)
            }))),
            Err(e) => Ok(ToolResult::err(serde_json::json!({
                "error": format!("Failed to write to file '{}': {}", params.path, e)
            }))),
        }
    }

    fn is_destructive(&self) -> bool { true }
}

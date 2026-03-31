use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Input structure for ReadFileTool
#[derive(Deserialize)]
pub struct ReadFileInput {
    pub path: String,
}

pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file at a specified path."
    }

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

    fn is_read_only(&self) -> bool {
        true
    }
}


/// Input structure for WriteFileTool
#[derive(Deserialize)]
pub struct WriteFileInput {
    pub path: String,
    pub content: String,
    pub overwrite: Option<bool>,
}

pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write text content to a file at a specified path. Use overwrite=true to replace an existing file."
    }

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

    fn is_destructive(&self) -> bool {
        true
    }
}

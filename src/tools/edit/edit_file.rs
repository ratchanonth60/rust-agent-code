use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Performs exact string replacements in files.
///
/// Supports three modes:
/// - **Single replacement** (default): `old_string` must appear exactly once.
/// - **Replace all**: set `replace_all = true` to replace every occurrence.
/// - **Create new file**: pass an empty `old_string` to write `new_string`
///   as a brand-new file.
pub struct FileEditTool;

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str { "Edit" }

    fn description(&self) -> &str {
        "Performs exact string replacements in files.\n\
         \n\
         Usage:\n\
         - You must read the file before editing. This tool will error if you haven't read it.\n\
         - The edit will FAIL if `old_string` is not unique in the file. Provide more context or use `replace_all`.\n\
         - Use `replace_all` for renaming variables or replacing all occurrences.\n\
         - When old_string is empty and file doesn't exist, creates a new file with new_string as content."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to modify"
                },
                "old_string": {
                    "type": "string",
                    "description": "The text to replace"
                },
                "new_string": {
                    "type": "string",
                    "description": "The text to replace it with (must be different from old_string)"
                },
                "replace_all": {
                    "type": "boolean",
                    "default": false,
                    "description": "Replace all occurrences of old_string (default false)"
                }
            },
            "required": ["file_path", "old_string", "new_string"],
            "additionalProperties": false
        })
    }

    fn is_destructive(&self) -> bool { true }

    /// Replaces `old_string` with `new_string` in the target file.
    ///
    /// # Errors
    ///
    /// Returns `Err` only on unexpected I/O failures. All validation
    /// issues (no match, ambiguous match, identical strings) are returned
    /// as `Ok(ToolResult::err(...))`.
    async fn call(&self, input: Value, _context: &ToolContext) -> anyhow::Result<ToolResult> {
        let file_path = input["file_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing file_path"))?;
        let old_string = input["old_string"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing old_string"))?;
        let new_string = input["new_string"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing new_string"))?;
        let replace_all = input["replace_all"].as_bool().unwrap_or(false);

        let path = Path::new(file_path);

        // Identical strings → no-op error
        if old_string == new_string {
            return Ok(ToolResult::err(json!({
                "error": "No changes to make: old_string and new_string are exactly the same."
            })));
        }

        // Empty old_string → create new file
        if old_string.is_empty() {
            if path.exists() {
                let existing = std::fs::read_to_string(path).unwrap_or_default();
                if !existing.is_empty() {
                    return Ok(ToolResult::err(json!({
                        "error": "Cannot create new file - file already exists with content."
                    })));
                }
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, new_string)?;
            return Ok(ToolResult::ok(json!({
                "success": true,
                "message": format!("Created new file: {}", file_path)
            })));
        }

        // File must exist for replacement
        if !path.exists() {
            return Ok(ToolResult::err(json!({
                "error": format!("File does not exist: {}", file_path)
            })));
        }

        let content = std::fs::read_to_string(path)?;

        let match_count = content.matches(old_string).count();
        if match_count == 0 {
            return Ok(ToolResult::err(json!({
                "error": format!("String to replace not found in file.\nString: {}", old_string)
            })));
        }

        // Ambiguous match guard
        if !replace_all && match_count > 1 {
            return Ok(ToolResult::err(json!({
                "error": format!(
                    "Found {} matches of the string to replace, but replace_all is false. \
                     To replace all occurrences, set replace_all to true. \
                     To replace only one occurrence, please provide more context to uniquely identify the instance.\n\
                     String: {}", match_count, old_string
                )
            })));
        }

        // Apply replacement
        let updated = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        // When deleting text, also consume the trailing newline if present
        let updated = if new_string.is_empty() && !old_string.ends_with('\n') {
            let with_newline = format!("{}\n", old_string);
            if content.contains(&with_newline) {
                if replace_all {
                    content.replace(&with_newline, "")
                } else {
                    content.replacen(&with_newline, "", 1)
                }
            } else {
                updated
            }
        } else {
            updated
        };

        std::fs::write(path, &updated)?;

        Ok(ToolResult::ok(json!({
            "success": true,
            "message": format!("Edited file: {} ({} replacement(s))", file_path, if replace_all { match_count } else { 1 })
        })))
    }
}

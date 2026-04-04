//! [`GlobTool`] — finds files by glob pattern, sorted newest-first.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Instant;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Finds files by glob pattern, returning paths sorted newest-first.
///
/// Uses the [`glob`] crate. Results are capped at [`MAX_RESULTS`] to
/// avoid flooding the LLM context window.
pub struct GlobTool;

/// Hard cap on the number of returned file paths.
const MAX_RESULTS: usize = 100;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str { "Glob" }

    fn description(&self) -> &str {
        "Fast file pattern matching tool that works with any codebase size.\n\
         Supports glob patterns like \"**/*.js\" or \"src/**/*.ts\".\n\
         Returns matching file paths sorted by modification time.\n\
         Use this tool when you need to find files by name patterns."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The glob pattern to match files against"
                },
                "path": {
                    "type": "string",
                    "description": "The directory to search in. If not specified, the current working directory will be used."
                }
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }

    fn is_read_only(&self) -> bool { true }
    fn is_concurrency_safe(&self) -> bool { true }

    /// Expands the glob, collects up to [`MAX_RESULTS`] file paths, and
    /// sorts them by `mtime` descending.
    async fn call(&self, input: Value, _context: &ToolContext) -> anyhow::Result<ToolResult> {
        let pattern = input["pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing pattern"))?;
        let search_path = input["path"]
            .as_str()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let start = Instant::now();

        let full_pattern = if search_path == std::path::Path::new(".") || search_path == std::env::current_dir().unwrap_or_default() {
            pattern.to_string()
        } else {
            format!("{}/{}", search_path.display(), pattern)
        };

        let mut files: Vec<PathBuf> = Vec::new();
        let mut truncated = false;

        match glob::glob(&full_pattern) {
            Ok(paths) => {
                for path in paths.flatten().filter(|p| p.is_file()) {
                    files.push(path);
                    if files.len() >= MAX_RESULTS {
                        truncated = true;
                        break;
                    }
                }
            }
            Err(e) => {
                return Ok(ToolResult::err(json!({
                    "error": format!("Invalid glob pattern: {}", e)
                })));
            }
        }

        // Newest first
        files.sort_by(|a, b| {
            let mtime_a = a.metadata().and_then(|m| m.modified()).ok();
            let mtime_b = b.metadata().and_then(|m| m.modified()).ok();
            mtime_b.cmp(&mtime_a)
        });

        let duration_ms = start.elapsed().as_millis() as u64;

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let filenames: Vec<String> = files
            .iter()
            .map(|f| {
                f.strip_prefix(&cwd)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| f.to_string_lossy().to_string())
            })
            .collect();

        if filenames.is_empty() {
            return Ok(ToolResult::ok(json!("No files found")));
        }

        Ok(ToolResult::ok(json!({
            "filenames": filenames,
            "numFiles": filenames.len(),
            "durationMs": duration_ms,
            "truncated": truncated
        })))
    }
}

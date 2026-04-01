use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Stdio;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Content search tool backed by [ripgrep](https://github.com/BurntSushi/ripgrep).
///
/// Supports regex patterns, file-type filters, context lines, and three
/// output modes: `files_with_matches` (default), `content`, and `count`.
pub struct GrepTool;

/// Default maximum output lines before truncation.
const DEFAULT_HEAD_LIMIT: usize = 250;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str { "Grep" }

    fn description(&self) -> &str {
        "A powerful search tool built on ripgrep.\n\
         Supports full regex syntax (e.g., \"log.*Error\", \"function\\s+\\w+\").\n\
         Filter files with glob parameter (e.g., \"*.js\") or type parameter (e.g., \"js\", \"py\").\n\
         Output modes: \"content\" shows matching lines, \"files_with_matches\" shows only file paths (default), \"count\" shows match counts."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The regular expression pattern to search for in file contents"
                },
                "path": {
                    "type": "string",
                    "description": "File or directory to search in. Defaults to current working directory."
                },
                "glob": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g. \"*.js\", \"*.{ts,tsx}\")"
                },
                "output_mode": {
                    "type": "string",
                    "enum": ["content", "files_with_matches", "count"],
                    "description": "Output mode. Defaults to \"files_with_matches\"."
                },
                "-i": { "type": "boolean", "description": "Case insensitive search" },
                "-n": { "type": "boolean", "description": "Show line numbers in output. Defaults to true for content mode." },
                "-A": { "type": "number", "description": "Number of lines to show after each match" },
                "-B": { "type": "number", "description": "Number of lines to show before each match" },
                "-C": { "type": "number", "description": "Number of lines of context around each match" },
                "context": { "type": "number", "description": "Number of lines of context (alias for -C)" },
                "type": { "type": "string", "description": "File type to search (e.g. js, py, rust, go)" },
                "head_limit": { "type": "number", "description": "Limit output to first N lines/entries. 0 for unlimited. Default 250." },
                "offset": { "type": "number", "description": "Skip first N lines/entries before applying head_limit. Default 0." },
                "multiline": { "type": "boolean", "description": "Enable multiline mode. Default false." }
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }

    fn is_read_only(&self) -> bool { true }
    fn is_concurrency_safe(&self) -> bool { true }

    /// Spawns `rg` with the assembled arguments and formats the output.
    ///
    /// # Errors
    ///
    /// Returns `ToolResult::err` when `rg` is not installed or exits
    /// with code 2 (regex / argument error). Exit code 1 (no matches)
    /// is reported as a successful empty result.
    async fn call(&self, input: Value, _context: &ToolContext) -> anyhow::Result<ToolResult> {
        let pattern = input["pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing pattern"))?;
        let search_path = input["path"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .to_string_lossy()
                    .to_string()
            });
        let output_mode = input["output_mode"].as_str().unwrap_or("files_with_matches");
        let case_insensitive = input["-i"].as_bool().unwrap_or(false);
        let show_line_numbers = input["-n"].as_bool().unwrap_or(true);
        let multiline = input["multiline"].as_bool().unwrap_or(false);
        let head_limit = input["head_limit"].as_u64().map(|v| v as usize).unwrap_or(DEFAULT_HEAD_LIMIT);
        let offset = input["offset"].as_u64().unwrap_or(0) as usize;
        let context_lines = input["context"].as_u64().or_else(|| input["-C"].as_u64());
        let after_lines = input["-A"].as_u64();
        let before_lines = input["-B"].as_u64();
        let file_type = input["type"].as_str();
        let glob_pattern = input["glob"].as_str();

        let mut args = build_rg_args(
            output_mode, show_line_numbers, context_lines, before_lines,
            after_lines, case_insensitive, multiline, file_type, glob_pattern, pattern,
        );
        args.push(search_path.clone());

        let output = tokio::process::Command::new("rg")
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        let output = match output {
            Ok(o) => o,
            Err(e) => {
                return Ok(ToolResult::err(json!({
                    "error": format!("Failed to execute ripgrep (rg): {}. Make sure ripgrep is installed.", e)
                })));
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        // rg exit codes: 0 = matches, 1 = no matches, 2 = error
        if output.status.code() == Some(2) {
            return Ok(ToolResult::err(json!({ "error": format!("ripgrep error: {}", stderr) })));
        }

        if stdout.trim().is_empty() {
            return Ok(ToolResult::ok(json!({ "message": "No matches found", "numFiles": 0 })));
        }

        let lines: Vec<&str> = stdout.lines().collect();
        let total_lines = lines.len();
        let lines_after_offset: Vec<&str> = lines.into_iter().skip(offset).collect();
        let (limited_lines, truncated) = apply_head_limit(lines_after_offset, head_limit);

        let result_text = limited_lines.join("\n");
        let result_text = relativize_paths(&result_text);

        format_output(output_mode, &result_text, limited_lines.len(), total_lines, truncated)
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Assembles the `rg` CLI argument vector from parsed tool input.
fn build_rg_args(
    output_mode: &str,
    show_line_numbers: bool,
    context_lines: Option<u64>,
    before_lines: Option<u64>,
    after_lines: Option<u64>,
    case_insensitive: bool,
    multiline: bool,
    file_type: Option<&str>,
    glob_pattern: Option<&str>,
    pattern: &str,
) -> Vec<String> {
    let mut args: Vec<String> = vec!["--hidden".to_string()];

    // Exclude VCS directories
    for dir in &[".git", ".svn", ".hg", ".bzr", ".jj"] {
        args.push(format!("--glob=!{}", dir));
    }

    args.push("--max-columns".to_string());
    args.push("500".to_string());

    match output_mode {
        "files_with_matches" => args.push("-l".to_string()),
        "count" => args.push("-c".to_string()),
        "content" => {
            if show_line_numbers { args.push("-n".to_string()); }
            if let Some(ctx) = context_lines {
                args.push(format!("-C{}", ctx));
            } else {
                if let Some(b) = before_lines { args.push(format!("-B{}", b)); }
                if let Some(a) = after_lines { args.push(format!("-A{}", a)); }
            }
        }
        _ => args.push("-l".to_string()),
    }

    if case_insensitive { args.push("-i".to_string()); }
    if multiline {
        args.push("-U".to_string());
        args.push("--multiline-dotall".to_string());
    }
    if let Some(ft) = file_type {
        args.push("--type".to_string());
        args.push(ft.to_string());
    }
    if let Some(g) = glob_pattern {
        args.push("--glob".to_string());
        args.push(g.to_string());
    }

    // Escape patterns that look like flags
    if pattern.starts_with('-') { args.push("-e".to_string()); }
    args.push(pattern.to_string());

    args
}

/// Applies offset + head_limit pagination to a line list.
///
/// Returns `(visible_lines, was_truncated)`.
fn apply_head_limit(lines: Vec<&str>, head_limit: usize) -> (Vec<&str>, bool) {
    if head_limit == 0 {
        (lines, false)
    } else {
        let truncated = lines.len() > head_limit;
        let limited: Vec<&str> = lines.into_iter().take(head_limit).collect();
        (limited, truncated)
    }
}

/// Strips the cwd prefix from file paths (handles both `/` and `\`).
fn relativize_paths(text: &str) -> String {
    let cwd = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .to_string_lossy()
        .to_string();
    let result = text.replace(&format!("{}/", cwd), "");
    let cwd_backslash = cwd.replace('/', "\\");
    result.replace(&format!("{}\\", cwd_backslash), "")
}

/// Builds the final [`ToolResult`] JSON for the given `output_mode`.
fn format_output(
    output_mode: &str,
    result_text: &str,
    num_limited_lines: usize,
    total_lines: usize,
    truncated: bool,
) -> anyhow::Result<ToolResult> {
    match output_mode {
        "files_with_matches" => {
            let filenames: Vec<&str> = result_text.lines().collect();
            Ok(ToolResult::ok(json!({
                "filenames": filenames,
                "numFiles": filenames.len(),
                "truncated": truncated
            })))
        }
        "count" => {
            let (mut total_matches, mut file_count) = (0u64, 0u64);
            for line in result_text.lines() {
                if let Some(count_str) = line.rsplit(':').next() {
                    if let Ok(count) = count_str.trim().parse::<u64>() {
                        total_matches += count;
                        file_count += 1;
                    }
                }
            }
            Ok(ToolResult::ok(json!({
                "content": result_text,
                "numFiles": file_count,
                "numMatches": total_matches,
                "truncated": truncated
            })))
        }
        _ => {
            Ok(ToolResult::ok(json!({
                "content": result_text,
                "numLines": num_limited_lines,
                "totalLines": total_lines,
                "truncated": truncated
            })))
        }
    }
}

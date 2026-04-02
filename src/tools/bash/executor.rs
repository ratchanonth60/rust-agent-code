//! [`BashTool`] implementation — spawns `bash -c` with timeout.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Deserialized input for [`BashTool`].
#[derive(Deserialize)]
pub struct BashInput {
    /// The shell command string to execute.
    pub command: String,
    /// Optional timeout in ms; defaults to 300 000 (5 min).
    pub timeout_ms: Option<u64>,
}

/// Executes arbitrary shell commands via `bash -c` with a configurable timeout.
///
/// Returns JSON containing `stdout`, `stderr`, and `exit_code`.
/// On timeout the child process is killed and an error result is returned.
pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str { "bash" }

    fn description(&self) -> &str {
        "Execute a bash command on the local machine. Use this to run shell scripts, compilers, search tools, etc. Provide the exact string of the command to execute."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute."
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Optional timeout in milliseconds (default 300000)."
                }
            },
            "required": ["command"]
        })
    }

    /// Spawns `bash -c <command>`, waits with timeout, and collects output.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the child process cannot be spawned (e.g. `bash`
    /// not found). Non-zero exit codes are returned as
    /// `Ok(ToolResult::err(...))` — they are not hard errors.
    async fn call(&self, input: Value, _context: &ToolContext) -> anyhow::Result<ToolResult> {
        let params: BashInput = serde_json::from_value(input)?;

        let mut child = Command::new("bash")
            .arg("-c")
            .arg(&params.command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn bash: {}", e))?;

        let timeout_duration = Duration::from_millis(params.timeout_ms.unwrap_or(300_000));

        let result = match timeout(timeout_duration, child.wait()).await {
            Ok(status_res) => status_res,
            Err(_) => {
                let _ = child.kill().await;
                return Ok(ToolResult::err(serde_json::json!({
                    "error": "Command timed out."
                })));
            }
        };

        match result {
            Ok(status) => {
                let mut stdout_buf = String::new();
                let mut stderr_buf = String::new();

                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_string(&mut stdout_buf).await;
                }
                if let Some(mut err) = child.stderr.take() {
                    let _ = err.read_to_string(&mut stderr_buf).await;
                }

                if status.success() {
                    Ok(ToolResult::ok(serde_json::json!({
                        "stdout": stdout_buf.trim(),
                        "stderr": stderr_buf.trim(),
                        "exit_code": 0
                    })))
                } else {
                    Ok(ToolResult::err(serde_json::json!({
                        "stdout": stdout_buf.trim(),
                        "stderr": stderr_buf.trim(),
                        "exit_code": status.code().unwrap_or(1),
                        "error": format!("Command exited with status: {}", status)
                    })))
                }
            }
            Err(e) => Ok(ToolResult::err(serde_json::json!({
                "error": format!("Failed waiting for bash command: {}", e)
            }))),
        }
    }

    fn is_destructive(&self) -> bool { true }
}

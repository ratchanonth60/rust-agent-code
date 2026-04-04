//! MCP stdio transport — spawns a subprocess and communicates via JSON-RPC over stdin/stdout.

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use super::types::{JsonRpcRequest, JsonRpcResponse};

/// A stdio-based MCP transport that manages a child process.
pub struct StdioTransport {
    child: Mutex<Child>,
    reader: Mutex<BufReader<tokio::process::ChildStdout>>,
    writer: Mutex<tokio::process::ChildStdin>,
    next_id: Mutex<u64>,
}

impl StdioTransport {
    /// Spawn a new MCP server process with the given command and args.
    pub async fn spawn(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        for (key, val) in env {
            cmd.env(key, val);
        }

        let mut child = cmd.spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to open child stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to open child stdout"))?;

        Ok(Self {
            child: Mutex::new(child),
            reader: Mutex::new(BufReader::new(stdout)),
            writer: Mutex::new(stdin),
            next_id: Mutex::new(1),
        })
    }

    /// Send a JSON-RPC request and wait for the matching response.
    pub async fn request(&self, method: &str, params: Option<serde_json::Value>) -> Result<JsonRpcResponse> {
        let id = {
            let mut next_id = self.next_id.lock().await;
            let id = *next_id;
            *next_id += 1;
            id
        };

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        };

        let mut payload = serde_json::to_string(&request)?;
        payload.push('\n');

        {
            let mut writer = self.writer.lock().await;
            writer.write_all(payload.as_bytes()).await?;
            writer.flush().await?;
        }

        // Read lines until we find a response matching our request ID
        let mut reader = self.reader.lock().await;
        let mut line = String::new();
        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                return Err(anyhow!("MCP server closed connection"));
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Try to parse as JSON-RPC response
            if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                if response.id == Some(id) {
                    return Ok(response);
                }
                // Not our response — skip (could be a notification)
            }
            // Not valid JSON or a notification — skip
        }
    }

    /// Send a JSON-RPC notification (no response expected).
    pub async fn notify(&self, method: &str, params: Option<serde_json::Value>) -> Result<()> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params.unwrap_or(serde_json::Value::Null),
        });

        let mut payload = serde_json::to_string(&notification)?;
        payload.push('\n');

        let mut writer = self.writer.lock().await;
        writer.write_all(payload.as_bytes()).await?;
        writer.flush().await?;
        Ok(())
    }

    /// Kill the child process.
    pub async fn close(&self) -> Result<()> {
        let mut child = self.child.lock().await;
        child.kill().await.ok();
        Ok(())
    }
}

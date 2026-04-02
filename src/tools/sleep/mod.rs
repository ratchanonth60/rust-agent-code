use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tools::{Tool, ToolContext, ToolResult};

pub struct SleepTool;

#[async_trait]
impl Tool for SleepTool {
    fn name(&self) -> &str {
        "Sleep"
    }

    fn description(&self) -> &str {
        "Pause execution for a specified number of seconds. Use sparingly."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "seconds": {
                    "type": "integer",
                    "description": "Number of seconds to sleep (1-300)",
                    "minimum": 1,
                    "maximum": 300
                }
            },
            "required": ["seconds"]
        })
    }

    fn is_destructive(&self) -> bool { false }
    fn is_read_only(&self) -> bool { true }
    fn is_concurrency_safe(&self) -> bool { true }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let seconds = input.get("seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .min(300);

        tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;

        Ok(ToolResult::ok(json!({
            "slept_seconds": seconds
        })))
    }
}

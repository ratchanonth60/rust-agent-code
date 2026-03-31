pub mod bash;
pub mod fs;

use async_trait::async_trait;
use serde_json::Value;

use crate::models::Message;

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub debug: bool,
    pub tools_available: Vec<String>,
    pub max_budget_usd: Option<f64>,
    pub auto_mode: bool,
}

pub struct ToolResult {
    pub output: Value,
    pub is_error: bool,
    pub new_messages: Vec<Message>,
}

impl ToolResult {
    pub fn ok(output: Value) -> Self {
        Self { output, is_error: false, new_messages: vec![] }
    }
    
    pub fn err(output: Value) -> Self {
        Self { output, is_error: true, new_messages: vec![] }
    }
}

/// Core interface for any Tool implemented in the Agent.
#[async_trait]
pub trait Tool: Send + Sync {
    /// The unique name of the tool
    fn name(&self) -> &str;

    /// The structured description used by the LLM
    fn description(&self) -> &str;

    /// The JSON Schema defining the input parameters (used for OpenAI formatting)
    fn input_schema(&self) -> serde_json::Value;

    /// Optional aliases
    fn aliases(&self) -> Vec<&str> { vec![] }

    /// Executes the core logic for the tool.
    /// `input` is dynamically typed from JSON schema, meaning each tool parses it inside.
    async fn call(&self, input: Value, context: &ToolContext) -> anyhow::Result<ToolResult>;

    /// Indicates if this tool mutates system state (e.g. creating/modifying files/processes)
    fn is_destructive(&self) -> bool { false }

    /// Indicates if this tool only reads data without side effects
    fn is_read_only(&self) -> bool { false }

    fn is_concurrency_safe(&self) -> bool { false }
}

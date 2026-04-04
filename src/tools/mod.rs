//! Tool system: trait definition, context, result types, and sub-module registry.
//!
//! Every agent tool implements the [`Tool`] trait.  The engine stores tools as
//! `Vec<Box<dyn Tool + Send + Sync>>` and dispatches calls by matching the
//! LLM's `function.name` against [`Tool::name`].

pub mod ask_user;
pub mod bash;
pub mod config_tool;
pub mod edit;
pub mod fs;
pub mod glob_tool;
pub mod grep_tool;
pub mod notebook;
pub mod plan_mode;
pub mod registry;
pub mod skill_tool;
pub mod sleep;
pub mod tasks;
pub mod todo;
pub mod web_fetch;
pub mod web_search;
pub mod worktree;

use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;

use crate::models::Message;
use crate::permissions::PermissionMode;

// ── Context and Result types ─────────────────────────────────────────────

/// Runtime context passed to every [`Tool::call`] invocation.
///
/// Contains flags and metadata that tools can inspect to adjust their
/// behavior (e.g. skipping confirmation prompts in `auto_mode`).
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// Enable verbose debug output inside tool execution.
    pub debug: bool,
    /// Names of all tools registered in the current engine session.
    pub tools_available: Vec<String>,
    /// Optional hard budget cap; tools may refuse to run if exceeded.
    pub max_budget_usd: Option<f64>,
    /// When `true`, tools skip interactive confirmation prompts.
    pub auto_mode: bool,
    /// Working directory for file-relative operations.
    pub cwd: PathBuf,
    /// Current permission mode — tools may adapt behavior accordingly.
    pub permission_mode: PermissionMode,
    /// Active session ID (if session persistence is enabled).
    pub session_id: Option<String>,
    /// `true` when running inside a sub-agent spawned by [`AgentTool`].
    pub is_agent: bool,
}

/// The outcome of a single [`Tool::call`] execution.
///
/// Wraps the JSON output value, an error flag, and any extra
/// [`Message`]s the tool wants injected into the conversation.
pub struct ToolResult {
    /// The JSON payload returned to the LLM.
    pub output: Value,
    /// `true` when the tool encountered an error.
    pub is_error: bool,
    /// Additional messages to append to the conversation history.
    pub new_messages: Vec<Message>,
}

impl ToolResult {
    /// Creates a successful result.
    ///
    /// # Examples
    ///
    /// ```
    /// use serde_json::json;
    /// let result = ToolResult::ok(json!({"status": "done"}));
    /// assert!(!result.is_error);
    /// ```
    pub fn ok(output: Value) -> Self {
        Self { output, is_error: false, new_messages: vec![] }
    }

    /// Creates an error result.
    ///
    /// # Examples
    ///
    /// ```
    /// use serde_json::json;
    /// let result = ToolResult::err(json!({"error": "not found"}));
    /// assert!(result.is_error);
    /// ```
    pub fn err(output: Value) -> Self {
        Self { output, is_error: true, new_messages: vec![] }
    }
}

/// Core trait that every agent tool must implement.
///
/// The engine stores tools as `Vec<Box<dyn Tool + Send + Sync>>` and
/// dispatches calls by matching [`Tool::name`] against the LLM's
/// `function.name` field.
///
/// # Implementing a new tool
///
/// ```ignore
/// pub struct MyTool;
///
/// #[async_trait]
/// impl Tool for MyTool {
///     fn name(&self) -> &str { "my_tool" }
///     fn description(&self) -> &str { "Does something useful." }
///     fn input_schema(&self) -> Value { json!({"type":"object","properties":{}}) }
///
///     async fn call(&self, input: Value, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
///         Ok(ToolResult::ok(json!({"ok": true})))
///     }
/// }
/// ```
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique name used in LLM function-call dispatch.
    fn name(&self) -> &str;

    /// Human / LLM-readable description injected into the tool schema.
    fn description(&self) -> &str;

    /// JSON Schema that describes the expected `input` parameter shape.
    fn input_schema(&self) -> serde_json::Value;

    /// Alternative names that can also trigger this tool.
    fn aliases(&self) -> Vec<&str> { vec![] }

    /// Executes the tool logic.
    ///
    /// # Errors
    ///
    /// Returns `Err` on unrecoverable failures (missing params, I/O errors).
    /// Recoverable problems should return `Ok(ToolResult::err(...))` instead.
    async fn call(&self, input: Value, context: &ToolContext) -> anyhow::Result<ToolResult>;

    /// Returns `true` if this tool mutates system state (files, processes, etc.).
    fn is_destructive(&self) -> bool { false }

    /// Returns `true` if this tool only reads data without side effects.
    fn is_read_only(&self) -> bool { false }

    /// Returns `true` if this tool can safely run concurrently with others.
    fn is_concurrency_safe(&self) -> bool { false }
}

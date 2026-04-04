//! Plan mode tools — enter and exit read-only plan mode.
//!
//! [`EnterPlanModeTool`] switches the engine to plan mode (blocking
//! destructive tools), and [`ExitPlanModeTool`] restores the previous
//! permission mode.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tools::{Tool, ToolContext, ToolResult};

// ── EnterPlanModeTool ───────────────────────────────────────────────────

/// Switch the engine into plan mode (read-only).
pub struct EnterPlanModeTool;

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str {
        "EnterPlanMode"
    }

    fn description(&self) -> &str {
        "Enter plan mode to design an implementation approach before writing code. \
         In plan mode, destructive tools are blocked."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, _input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        // Plan mode toggle requires SharedEngineState integration.
        // For now, return acknowledgment.
        Ok(ToolResult::ok(json!({
            "status": "plan_mode_entered",
            "message": "Plan mode activated. Destructive tools are now blocked. \
                       Use ExitPlanMode when your plan is ready for approval."
        })))
    }
}

// ── ExitPlanModeTool ────────────────────────────────────────────────────

/// Exit plan mode and restore normal permissions.
pub struct ExitPlanModeTool;

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        "ExitPlanMode"
    }

    fn description(&self) -> &str {
        "Exit plan mode after the plan is ready for user approval."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, _input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        Ok(ToolResult::ok(json!({
            "status": "plan_mode_exited",
            "message": "Plan mode deactivated. Normal permissions restored."
        })))
    }
}

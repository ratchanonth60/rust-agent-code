//! `/plan` — toggle plan mode (read-only mode that blocks destructive tools).

use super::types::{Command, CommandContext, CommandResult, CommandType};

/// Toggle plan mode on or off.
pub struct PlanCommand;

impl Command for PlanCommand {
    fn name(&self) -> &str {
        "plan"
    }

    fn description(&self) -> &str {
        "Toggle plan mode (read-only)"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        // Plan mode toggling requires SharedEngineState integration.
        // For now, return informational text.
        Ok(CommandResult::Text(
            "  Plan mode toggle requires engine state integration.\n  \
             Use --permission-mode plan at startup for read-only mode."
                .to_string(),
        ))
    }
}

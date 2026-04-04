//! `/cost` — show session token usage and cost.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct CostCommand;

impl Command for CostCommand {
    fn name(&self) -> &str {
        "cost"
    }

    fn description(&self) -> &str {
        "Show token usage and cost"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        if let Some(ref tracker) = ctx.cost_tracker {
            if let Ok(t) = tracker.lock() {
                return Ok(CommandResult::Text(format!("  {}", t.format_total_cost())));
            }
        }
        Ok(CommandResult::Text(
            "  Cost tracking not available.".to_string(),
        ))
    }
}

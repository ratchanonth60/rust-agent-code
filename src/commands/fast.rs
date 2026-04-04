//! `/fast` — toggle fast mode.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct FastCommand;

impl Command for FastCommand {
    fn name(&self) -> &str {
        "fast"
    }

    fn description(&self) -> &str {
        "Toggle fast mode"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        Ok(CommandResult::Text(
            "  Fast mode toggle is a placeholder.\n  \
             When enabled, the agent will prefer speed over thoroughness.\n  \
             This will be wired to the engine in a future release."
                .to_string(),
        ))
    }
}

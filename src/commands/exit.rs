//! `/exit` and `/quit` — exit the application.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct ExitCommand;

impl Command for ExitCommand {
    fn name(&self) -> &str {
        "exit"
    }

    fn description(&self) -> &str {
        "Exit the agent"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["quit"]
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        Ok(CommandResult::Exit)
    }
}

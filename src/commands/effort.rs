//! `/effort` — set reasoning effort level.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct EffortCommand;

impl Command for EffortCommand {
    fn name(&self) -> &str {
        "effort"
    }

    fn description(&self) -> &str {
        "Set reasoning effort level"
    }

    fn argument_hint(&self) -> Option<&str> {
        Some("[low|medium|high]")
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let level = args.trim().to_lowercase();

        match level.as_str() {
            "low" | "medium" | "high" => Ok(CommandResult::Text(format!(
                "  Reasoning effort set to '{}'. (Placeholder \u{2014} will be wired to the engine in a future release.)",
                level
            ))),
            "" => Ok(CommandResult::Text(
                "  Current effort level: medium (default)\n  \
                 Usage: /effort <low|medium|high>"
                    .to_string(),
            )),
            _ => Ok(CommandResult::Text(format!(
                "  Invalid effort level: '{}'\n  Valid values: low, medium, high",
                level
            ))),
        }
    }
}

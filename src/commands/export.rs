//! `/export` — export conversation (placeholder).

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct ExportCommand;

impl Command for ExportCommand {
    fn name(&self) -> &str {
        "export"
    }

    fn description(&self) -> &str {
        "Export conversation"
    }

    fn argument_hint(&self) -> Option<&str> {
        Some("[format]")
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let format = args.trim();

        let supported = ["json", "markdown", "md", "txt"];
        if format.is_empty() {
            Ok(CommandResult::Text(format!(
                "  Export conversation.\n  \
                 Usage: /export <format>\n  \
                 Supported formats: {}\n  \
                 (This feature is a placeholder and will be fully implemented in a future release.)",
                supported.join(", ")
            )))
        } else if supported.contains(&format) {
            Ok(CommandResult::Text(format!(
                "  Export to '{}' format is not yet implemented.\n  \
                 This is a placeholder for future conversation export functionality.",
                format
            )))
        } else {
            Ok(CommandResult::Text(format!(
                "  Unknown export format: '{}'\n  Supported formats: {}",
                format,
                supported.join(", ")
            )))
        }
    }
}

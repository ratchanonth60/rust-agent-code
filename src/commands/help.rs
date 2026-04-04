//! `/help` — display available commands.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct HelpCommand;

impl Command for HelpCommand {
    fn name(&self) -> &str {
        "help"
    }

    fn description(&self) -> &str {
        "Show available commands"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        // We build help from the registry, but since we don't have access to it here,
        // we return a static help text. The registry-aware help is built in mod.rs.
        let _ = ctx;
        Ok(CommandResult::Text(String::new())) // placeholder; overridden by registry-aware build
    }
}

/// Build help text from a command registry.
pub fn build_help_text(commands: &[&dyn Command]) -> String {
    let mut lines = Vec::new();
    lines.push("  Available commands:".to_string());
    for cmd in commands {
        let hint = cmd
            .argument_hint()
            .map(|h| format!(" {}", h))
            .unwrap_or_default();
        lines.push(format!(
            "    /{}{:<16} {}",
            cmd.name(),
            hint,
            cmd.description()
        ));
    }
    lines.join("\n")
}

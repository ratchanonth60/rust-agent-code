//! `/permissions` — show current permission mode.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct PermissionsCommand;

impl Command for PermissionsCommand {
    fn name(&self) -> &str {
        "permissions"
    }

    fn description(&self) -> &str {
        "Show current permission mode"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let mut lines = Vec::new();
        lines.push("  Permission Modes".to_string());
        lines.push("  ----------------".to_string());
        lines.push("  Available modes:".to_string());
        lines.push("    default          Ask for everything except read-only tools in the cwd".to_string());
        lines.push("    accept-edits     Auto-allow file writes within the working directory".to_string());
        lines.push("    bypass           Auto-allow nearly all operations".to_string());
        lines.push("    plan             Read-only mode: deny all destructive tools".to_string());
        lines.push("    dont-ask         Non-interactive: convert all ask decisions to deny".to_string());
        lines.push(String::new());
        lines.push("  Set via CLI: --permission-mode <mode>".to_string());

        Ok(CommandResult::Text(lines.join("\n")))
    }
}

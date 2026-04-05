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
        let lines = vec![
            "  Permission Modes".to_string(),
            "  ----------------".to_string(),
            "  Available modes:".to_string(),
            "    default          Ask for everything except read-only tools in the cwd".to_string(),
            "    accept-edits     Auto-allow file writes within the working directory".to_string(),
            "    bypass           Auto-allow nearly all operations".to_string(),
            "    plan             Read-only mode: deny all destructive tools".to_string(),
            "    dont-ask         Non-interactive: convert all ask decisions to deny".to_string(),
            String::new(),
            "  Set via CLI: --permission-mode <mode>".to_string(),
        ];

        Ok(CommandResult::Text(lines.join("\n")))
    }
}

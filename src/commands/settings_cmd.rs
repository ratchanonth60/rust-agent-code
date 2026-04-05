//! `/settings` — open the interactive settings dialog.
//!
//! In TUI mode, this command is intercepted by `App::handle_slash_command()`
//! *before* reaching the registry — it opens the [`SettingsDialog`] overlay
//! directly (see `src/ui/app/commands_handler.rs`).
//!
//! This registry entry exists so that:
//! - `/settings` appears in `/help` and autocomplete
//! - Non-TUI modes (bare, one-shot) get a helpful fallback message
//!
//! [`SettingsDialog`]: crate::ui::dialogs::settings_dialog::SettingsDialog

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct SettingsCommand;

impl Command for SettingsCommand {
    fn name(&self) -> &str {
        "settings"
    }

    fn description(&self) -> &str {
        "Open interactive settings dialog"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        // The actual dialog opening is handled by the TUI's handle_slash_command
        // before it reaches the registry. This is a fallback for non-TUI modes.
        Ok(CommandResult::Text(
            "  Settings dialog is only available in TUI mode.\n  \
             Use /config [key] [value] to change settings in this mode."
                .to_string(),
        ))
    }
}

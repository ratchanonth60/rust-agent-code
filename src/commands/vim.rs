//! `/vim` — toggle Vim editing mode for the TUI prompt.

use super::types::{Command, CommandContext, CommandResult, CommandType};
use crate::config::{EditorMode, GlobalConfig};

/// Toggle between Normal and Vim input modes.
pub struct VimCommand;

impl Command for VimCommand {
    fn name(&self) -> &str {
        "vim"
    }

    fn description(&self) -> &str {
        "Toggle vim editing mode"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let mut cfg = GlobalConfig::load();
        cfg.editor_mode = match cfg.editor_mode {
            EditorMode::Normal => EditorMode::Vim,
            EditorMode::Vim => EditorMode::Normal,
        };
        cfg.save()?;
        Ok(CommandResult::Text(format!(
            "  Editor mode: {}",
            cfg.editor_mode
        )))
    }
}

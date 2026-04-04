//! `/model` — display or change the active LLM model.

use super::types::{Command, CommandContext, CommandResult, CommandType};

/// Show or switch the active model.
pub struct ModelCommand;

impl Command for ModelCommand {
    fn name(&self) -> &str {
        "model"
    }

    fn description(&self) -> &str {
        "Show or change the active model"
    }

    fn argument_hint(&self) -> Option<&str> {
        Some("[name]")
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let name = args.trim();
        if name.is_empty() {
            // Show current model — requires SharedEngineState integration.
            Ok(CommandResult::Text(
                "  Usage: /model <name>\n  \
                 Available models depend on the active provider.\n  \
                 Examples: gemini-2.5-flash, gpt-4o-mini, claude-sonnet-4-6"
                    .to_string(),
            ))
        } else {
            // Change model — requires SharedEngineState integration.
            // For now, persist in GlobalConfig.
            let mut cfg = crate::config::GlobalConfig::load();
            cfg.default_model = Some(name.to_string());
            cfg.save()?;
            Ok(CommandResult::Text(format!(
                "  Model set to: {} (saved to config, takes effect next session)",
                name
            )))
        }
    }
}

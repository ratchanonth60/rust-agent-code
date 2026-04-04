//! `/compact` — compact or summarize the conversation (placeholder).

use super::types::{Command, CommandContext, CommandResult, CommandType, PromptCommand};

pub struct CompactCommand;

impl Command for CompactCommand {
    fn name(&self) -> &str {
        "compact"
    }

    fn description(&self) -> &str {
        "Compact conversation context"
    }

    fn argument_hint(&self) -> Option<&str> {
        Some("[instructions]")
    }

    fn command_type(&self) -> CommandType {
        CommandType::Prompt
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let args = args.trim();

        let instructions = if args.is_empty() {
            String::new()
        } else {
            format!("\n\nAdditional instructions for summarization: {}", args)
        };

        let prompt = format!(
            "Please provide a concise summary of our conversation so far. \
             Capture the key decisions, code changes, and any outstanding tasks. \
             This summary will be used as context for continuing the conversation.{}",
            instructions
        );

        Ok(CommandResult::Prompt(PromptCommand {
            content: prompt,
            allowed_tools: None,
        }))
    }
}

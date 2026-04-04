//! `/review` — generate a prompt for the LLM to review recent git changes.

use super::types::{Command, CommandContext, CommandResult, CommandType, PromptCommand};

pub struct ReviewCommand;

impl Command for ReviewCommand {
    fn name(&self) -> &str {
        "review"
    }

    fn description(&self) -> &str {
        "Review recent git changes"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["review-pr"]
    }

    fn command_type(&self) -> CommandType {
        CommandType::Prompt
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let args = args.trim();

        let scope = if args.is_empty() {
            "the current uncommitted changes (both staged and unstaged)".to_string()
        } else {
            format!("the changes specified by: {}", args)
        };

        let prompt = format!(
            "Please review {} in this repository.\n\n\
             Steps:\n\
             1. Run `git diff` and `git diff --staged` to see all changes.\n\
             2. Run `git log --oneline -10` for recent commit context.\n\
             3. Review each changed file for:\n\
                - Correctness and potential bugs\n\
                - Code style and consistency\n\
                - Security concerns\n\
                - Performance issues\n\
                - Missing error handling\n\
                - Test coverage gaps\n\
             4. Provide a structured review with specific, actionable feedback.\n\
             5. Rate the overall change quality (looks good / needs changes / needs discussion).",
            scope
        );

        Ok(CommandResult::Prompt(PromptCommand {
            content: prompt,
            allowed_tools: Some(vec![
                "Bash".to_string(),
                "ReadFile".to_string(),
                "GlobTool".to_string(),
                "GrepTool".to_string(),
            ]),
        }))
    }
}

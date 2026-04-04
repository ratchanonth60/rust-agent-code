//! `/commit` — generate a prompt for the LLM to commit changes following git safety protocol.

use super::types::{Command, CommandContext, CommandResult, CommandType, PromptCommand};

pub struct CommitCommand;

impl Command for CommitCommand {
    fn name(&self) -> &str {
        "commit"
    }

    fn description(&self) -> &str {
        "Commit changes with AI-generated message"
    }

    fn argument_hint(&self) -> Option<&str> {
        Some("[message]")
    }

    fn command_type(&self) -> CommandType {
        CommandType::Prompt
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let args = args.trim();

        let prompt = if args.is_empty() {
            "Look at the current git diff (staged and unstaged) and the recent git log. \
             Then create a new git commit following the git safety protocol:\n\
             1. Run `git status` to see all changes.\n\
             2. Run `git diff` and `git diff --staged` to review changes.\n\
             3. Run `git log --oneline -5` to see the commit message style.\n\
             4. Stage the relevant files (prefer specific files over `git add -A`).\n\
             5. Write a concise commit message that focuses on the 'why' not the 'what'.\n\
             6. Create the commit.\n\
             7. Do NOT push unless explicitly asked.\n\
             IMPORTANT: Never amend existing commits unless asked. Always create NEW commits."
                .to_string()
        } else {
            format!(
                "Create a git commit with this guidance: \"{}\"\n\
                 Follow the git safety protocol:\n\
                 1. Run `git status` and `git diff --staged` to verify what will be committed.\n\
                 2. Stage relevant files (prefer specific files over `git add -A`).\n\
                 3. Create the commit with an appropriate message based on the guidance above.\n\
                 4. Do NOT push unless explicitly asked.\n\
                 IMPORTANT: Never amend existing commits unless asked. Always create NEW commits.",
                args
            )
        };

        Ok(CommandResult::Prompt(PromptCommand {
            content: prompt,
            allowed_tools: Some(vec!["Bash".to_string()]),
        }))
    }
}

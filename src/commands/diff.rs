//! `/diff` — run `git diff` and display the output.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct DiffCommand;

impl Command for DiffCommand {
    fn name(&self) -> &str {
        "diff"
    }

    fn description(&self) -> &str {
        "Show git diff output"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, args: &str, ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let mut cmd = std::process::Command::new("git");
        cmd.arg("diff").current_dir(&ctx.cwd);

        // Allow extra args like `--staged` or a specific path.
        let extra: Vec<&str> = args.split_whitespace().collect();
        if !extra.is_empty() {
            cmd.args(&extra);
        }

        let output = cmd.output();
        match output {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout).to_string();
                if text.trim().is_empty() {
                    Ok(CommandResult::Text(
                        "  No differences found.".to_string(),
                    ))
                } else {
                    Ok(CommandResult::Text(text))
                }
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                Ok(CommandResult::Text(format!("  git diff failed: {}", stderr)))
            }
            Err(e) => Ok(CommandResult::Text(format!(
                "  Failed to run git: {}",
                e
            ))),
        }
    }
}

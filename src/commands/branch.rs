//! `/branch` — show current branch or list branches.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct BranchCommand;

impl Command for BranchCommand {
    fn name(&self) -> &str {
        "branch"
    }

    fn description(&self) -> &str {
        "Show current branch or list branches"
    }

    fn argument_hint(&self) -> Option<&str> {
        Some("[name]")
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, args: &str, ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let args = args.trim();

        if args.is_empty() {
            // Show all branches (highlights current one).
            let output = std::process::Command::new("git")
                .args(["branch", "-a"])
                .current_dir(&ctx.cwd)
                .output();

            match output {
                Ok(out) if out.status.success() => {
                    let text = String::from_utf8_lossy(&out.stdout).to_string();
                    if text.trim().is_empty() {
                        Ok(CommandResult::Text(
                            "  No branches found (not a git repository?).".to_string(),
                        ))
                    } else {
                        Ok(CommandResult::Text(text))
                    }
                }
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    Ok(CommandResult::Text(format!("  git branch failed: {}", stderr)))
                }
                Err(e) => Ok(CommandResult::Text(format!(
                    "  Failed to run git: {}",
                    e
                ))),
            }
        } else {
            // Show info about a specific branch.
            let output = std::process::Command::new("git")
                .args(["log", "--oneline", "-10", args])
                .current_dir(&ctx.cwd)
                .output();

            match output {
                Ok(out) if out.status.success() => {
                    let text = String::from_utf8_lossy(&out.stdout).to_string();
                    Ok(CommandResult::Text(format!(
                        "  Recent commits on '{}':\n{}",
                        args, text
                    )))
                }
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    Ok(CommandResult::Text(format!(
                        "  Could not show branch '{}': {}",
                        args, stderr
                    )))
                }
                Err(e) => Ok(CommandResult::Text(format!(
                    "  Failed to run git: {}",
                    e
                ))),
            }
        }
    }
}

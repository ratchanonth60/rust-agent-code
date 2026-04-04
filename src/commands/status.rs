//! `/status` — show current session status overview.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct StatusCommand;

impl Command for StatusCommand {
    fn name(&self) -> &str {
        "status"
    }

    fn description(&self) -> &str {
        "Show session status"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let mut lines = Vec::new();
        lines.push("  Session Status".to_string());
        lines.push("  -------------".to_string());
        lines.push(format!("  Working directory: {}", ctx.cwd.display()));

        // Git branch
        let branch = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&ctx.cwd)
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                } else {
                    None
                }
            });

        if let Some(branch) = branch {
            lines.push(format!("  Git branch:        {}", branch));
        } else {
            lines.push("  Git branch:        (not a git repo)".to_string());
        }

        // Cost summary
        if let Some(ref tracker) = ctx.cost_tracker {
            if let Ok(t) = tracker.lock() {
                let model_count = t.model_usage.len();
                lines.push(format!("  Models used:       {}", model_count));
                lines.push(format!("  Total cost:        ${:.4}", t.total_cost_usd));
            }
        }

        Ok(CommandResult::Text(lines.join("\n")))
    }
}

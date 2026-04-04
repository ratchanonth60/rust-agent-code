//! `/stats` — show session statistics: token usage, cost, and timing.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct StatsCommand;

impl Command for StatsCommand {
    fn name(&self) -> &str {
        "stats"
    }

    fn description(&self) -> &str {
        "Show session statistics"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let mut lines = Vec::new();
        lines.push("  Session Statistics".to_string());
        lines.push("  -----------------".to_string());

        // Working directory
        lines.push(format!("  Working directory: {}", ctx.cwd.display()));

        // Cost tracker info
        if let Some(ref tracker) = ctx.cost_tracker {
            if let Ok(t) = tracker.lock() {
                let total_input: u64 = t.model_usage.values().map(|u| u.input_tokens).sum();
                let total_output: u64 = t.model_usage.values().map(|u| u.output_tokens).sum();
                let model_count = t.model_usage.len();

                lines.push(format!("  Models used:       {}", model_count));
                lines.push(format!("  Input tokens:      {}", total_input));
                lines.push(format!("  Output tokens:     {}", total_output));
                lines.push(format!("  API duration:      {}ms", t.total_api_duration_ms));
                lines.push(format!("  Tool duration:     {}ms", t.total_tool_duration_ms));
                lines.push(format!("  Lines added:       {}", t.total_lines_added));
                lines.push(format!("  Lines removed:     {}", t.total_lines_removed));
                lines.push(format!("  Total cost:        ${:.4}", t.total_cost_usd));
            }
        } else {
            lines.push("  Cost tracking:     not available".to_string());
        }

        Ok(CommandResult::Text(lines.join("\n")))
    }
}

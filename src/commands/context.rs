//! `/context` — show what context is being injected into the system prompt.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct ContextCommand;

impl Command for ContextCommand {
    fn name(&self) -> &str {
        "context"
    }

    fn description(&self) -> &str {
        "Show injected context sources"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let mut lines = Vec::new();
        lines.push("  Context Sources".to_string());
        lines.push("  ===============".to_string());

        // CLAUDE.md files
        lines.push(String::new());
        lines.push("  CLAUDE.md Files".to_string());
        lines.push("  ---------------".to_string());

        let claudemd_paths = [
            dirs::home_dir()
                .map(|h| h.join(".claude").join("CLAUDE.md")),
            Some(ctx.cwd.join("CLAUDE.md")),
            Some(ctx.cwd.join(".claude").join("CLAUDE.md")),
        ];

        for path in claudemd_paths.iter().flatten() {
                if path.exists() {
                    let size = std::fs::metadata(path)
                        .map(|m| m.len())
                        .unwrap_or(0);
                    lines.push(format!("    [loaded] {} ({} bytes)", path.display(), size));
                } else {
                    lines.push(format!("    [absent] {}", path.display()));
                }
        }

        // Git context
        lines.push(String::new());
        lines.push("  Git Context".to_string());
        lines.push("  -----------".to_string());

        let in_git = std::process::Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .current_dir(&ctx.cwd)
            .output()
            .ok()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if in_git {
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
                })
                .unwrap_or_else(|| "(unknown)".to_string());
            lines.push(format!("    [active] Branch: {}", branch));
            lines.push("    [active] Status, recent commits injected".to_string());
        } else {
            lines.push("    [inactive] Not a git repository".to_string());
        }

        // Memory
        lines.push(String::new());
        lines.push("  Memory".to_string());
        lines.push("  ------".to_string());
        let mem_dir = crate::mem::get_auto_mem_path();
        let mem_index = mem_dir.join("MEMORY.md");
        if mem_index.exists() {
            let size = std::fs::metadata(&mem_index)
                .map(|m| m.len())
                .unwrap_or(0);
            lines.push(format!("    [loaded] MEMORY.md ({} bytes)", size));
        } else {
            lines.push(format!("    [absent] {}", mem_index.display()));
        }

        // Output styles
        lines.push(String::new());
        lines.push("  Output Styles".to_string());
        lines.push("  -------------".to_string());
        let styles = crate::output_styles::load_output_styles();
        if styles.is_empty() {
            lines.push("    (no output styles loaded)".to_string());
        } else {
            for style in &styles {
                lines.push(format!("    [loaded] {}", style.name));
            }
        }

        Ok(CommandResult::Text(lines.join("\n")))
    }
}

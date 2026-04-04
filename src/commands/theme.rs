//! `/theme` — list or switch output themes.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct ThemeCommand;

impl Command for ThemeCommand {
    fn name(&self) -> &str {
        "theme"
    }

    fn description(&self) -> &str {
        "List or switch output themes"
    }

    fn argument_hint(&self) -> Option<&str> {
        Some("[name]")
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let args = args.trim();

        if args.is_empty() {
            // List available output styles.
            let styles = crate::output_styles::load_output_styles();
            let cfg = crate::config::GlobalConfig::load();

            let mut lines = Vec::new();
            lines.push("  Output Themes / Styles".to_string());
            lines.push("  ---------------------".to_string());
            lines.push(format!("  Current theme: {}", cfg.theme));
            lines.push(format!(
                "  Current output style: {}",
                cfg.output_style.as_deref().unwrap_or("(none)")
            ));

            if styles.is_empty() {
                lines.push(String::new());
                lines.push("  No output styles found.".to_string());
                lines.push("  Add .md files to ~/.rust-agent/output-styles/ to create styles.".to_string());
            } else {
                lines.push(String::new());
                lines.push("  Available output styles:".to_string());
                for style in &styles {
                    lines.push(format!("    - {}", style.name));
                }
            }

            Ok(CommandResult::Text(lines.join("\n")))
        } else {
            // Set theme.
            let mut cfg = crate::config::GlobalConfig::load();
            cfg.theme = args.to_string();
            if let Err(e) = cfg.save() {
                return Ok(CommandResult::Text(format!(
                    "  Failed to save config: {}",
                    e
                )));
            }
            Ok(CommandResult::Text(format!(
                "  Theme set to '{}'.",
                args
            )))
        }
    }
}

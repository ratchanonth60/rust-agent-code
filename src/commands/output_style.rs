//! `/output-style` — change output style (alias of `/theme`).

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct OutputStyleCommand;

impl Command for OutputStyleCommand {
    fn name(&self) -> &str {
        "output-style"
    }

    fn description(&self) -> &str {
        "Change output style"
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
            lines.push("  Output Styles".to_string());
            lines.push("  -------------".to_string());
            lines.push(format!(
                "  Active: {}",
                cfg.output_style.as_deref().unwrap_or("(none)")
            ));

            if styles.is_empty() {
                lines.push(String::new());
                lines.push("  No output styles found.".to_string());
                lines.push("  Add .md files to ~/.rust-agent/output-styles/ to create styles.".to_string());
            } else {
                lines.push(String::new());
                lines.push("  Available styles:".to_string());
                for style in &styles {
                    let marker = if cfg.output_style.as_deref() == Some(&style.name) {
                        " (active)"
                    } else {
                        ""
                    };
                    lines.push(format!("    - {}{}", style.name, marker));
                }
            }

            Ok(CommandResult::Text(lines.join("\n")))
        } else if args == "none" || args == "reset" {
            let mut cfg = crate::config::GlobalConfig::load();
            cfg.output_style = None;
            if let Err(e) = cfg.save() {
                return Ok(CommandResult::Text(format!(
                    "  Failed to save config: {}",
                    e
                )));
            }
            Ok(CommandResult::Text(
                "  Output style cleared.".to_string(),
            ))
        } else {
            let mut cfg = crate::config::GlobalConfig::load();
            cfg.output_style = Some(args.to_string());
            if let Err(e) = cfg.save() {
                return Ok(CommandResult::Text(format!(
                    "  Failed to save config: {}",
                    e
                )));
            }
            Ok(CommandResult::Text(format!(
                "  Output style set to '{}'.",
                args
            )))
        }
    }
}

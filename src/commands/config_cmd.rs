//! `/config` — show or set configuration in `~/.rust-agent/config.json`.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct ConfigCommand;

impl Command for ConfigCommand {
    fn name(&self) -> &str {
        "config"
    }

    fn description(&self) -> &str {
        "Show or set configuration"
    }

    fn argument_hint(&self) -> Option<&str> {
        Some("[key] [value]")
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let parts: Vec<&str> = args.split_whitespace().collect();

        match parts.len() {
            // No arguments — show full config.
            0 => {
                let cfg = crate::config::GlobalConfig::load();
                let path = crate::config::config_path();
                let mut lines = Vec::new();
                lines.push(format!("  Config file: {}", path.display()));
                lines.push("  ─────────────────────".to_string());
                lines.push(format!("  editor_mode:      {}", cfg.editor_mode));
                lines.push(format!("  theme:            {}", cfg.theme));
                lines.push(format!(
                    "  default_model:    {}",
                    cfg.default_model.as_deref().unwrap_or("(none)")
                ));
                lines.push(format!(
                    "  default_provider: {}",
                    cfg.default_provider.as_deref().unwrap_or("(none)")
                ));
                lines.push(format!(
                    "  output_style:     {}",
                    cfg.output_style.as_deref().unwrap_or("(none)")
                ));
                Ok(CommandResult::Text(lines.join("\n")))
            }

            // One argument — show specific key.
            1 => {
                let key = parts[0];
                let cfg = crate::config::GlobalConfig::load();
                let value = match key {
                    "editor_mode" => cfg.editor_mode.to_string(),
                    "theme" => cfg.theme.clone(),
                    "default_model" => cfg
                        .default_model
                        .as_deref()
                        .unwrap_or("(none)")
                        .to_string(),
                    "default_provider" => cfg
                        .default_provider
                        .as_deref()
                        .unwrap_or("(none)")
                        .to_string(),
                    "output_style" => cfg
                        .output_style
                        .as_deref()
                        .unwrap_or("(none)")
                        .to_string(),
                    _ => {
                        return Ok(CommandResult::Text(format!(
                            "  Unknown config key: '{}'\n  \
                             Available keys: editor_mode, theme, default_model, default_provider, output_style",
                            key
                        )));
                    }
                };
                Ok(CommandResult::Text(format!("  {} = {}", key, value)))
            }

            // Two or more arguments — set key=value.
            _ => {
                let key = parts[0];
                let value = parts[1..].join(" ");
                let mut cfg = crate::config::GlobalConfig::load();

                match key {
                    "editor_mode" => {
                        match value.as_str() {
                            "normal" => cfg.editor_mode = crate::config::EditorMode::Normal,
                            "vim" => cfg.editor_mode = crate::config::EditorMode::Vim,
                            _ => {
                                return Ok(CommandResult::Text(format!(
                                    "  Invalid editor_mode: '{}'. Use 'normal' or 'vim'.",
                                    value
                                )));
                            }
                        }
                    }
                    "theme" => cfg.theme = value.clone(),
                    "default_model" => {
                        cfg.default_model = if value == "none" {
                            None
                        } else {
                            Some(value.clone())
                        };
                    }
                    "default_provider" => {
                        cfg.default_provider = if value == "none" {
                            None
                        } else {
                            Some(value.clone())
                        };
                    }
                    "output_style" => {
                        cfg.output_style = if value == "none" {
                            None
                        } else {
                            Some(value.clone())
                        };
                    }
                    _ => {
                        return Ok(CommandResult::Text(format!(
                            "  Unknown config key: '{}'\n  \
                             Available keys: editor_mode, theme, default_model, default_provider, output_style",
                            key
                        )));
                    }
                }

                if let Err(e) = cfg.save() {
                    return Ok(CommandResult::Text(format!(
                        "  Failed to save config: {}",
                        e
                    )));
                }

                Ok(CommandResult::Text(format!(
                    "  Set {} = {}",
                    key, value
                )))
            }
        }
    }
}

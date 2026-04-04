//! `/keybindings` — show current keybinding summary.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct KeybindingsCommand;

impl Command for KeybindingsCommand {
    fn name(&self) -> &str {
        "keybindings"
    }

    fn description(&self) -> &str {
        "Show keybinding summary"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["keys"]
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let bindings = crate::keybindings::default_bindings::default_bindings();

        let mut lines = Vec::new();
        lines.push("  Keybinding Summary".to_string());
        lines.push("  ==================".to_string());

        for block in &bindings {
            lines.push(String::new());
            lines.push(format!("  [{}]", block.context));
            let mut sorted_bindings: Vec<_> = block.bindings.iter().collect();
            sorted_bindings.sort_by_key(|(key, _)| (*key).clone());
            for (key, action) in &sorted_bindings {
                let action_str = action.as_deref().unwrap_or("(unbound)");
                lines.push(format!("    {:<24} {}", key, action_str));
            }
        }

        lines.push(String::new());
        lines.push("  Override keybindings in ~/.rust-agent/keybindings.json".to_string());

        Ok(CommandResult::Text(lines.join("\n")))
    }
}

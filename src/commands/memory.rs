//! `/memory` — list memory files from `~/.rust-agent/memory/`.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct MemoryCommand;

impl Command for MemoryCommand {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "List agent memory files"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let memory_dir = crate::mem::get_auto_mem_path();

        if !memory_dir.exists() {
            return Ok(CommandResult::Text(format!(
                "  Memory directory does not exist yet: {}",
                memory_dir.display()
            )));
        }

        let mut lines = Vec::new();
        lines.push(format!("  Memory files ({})", memory_dir.display()));
        lines.push("  ─────────────────────".to_string());

        let mut found = false;
        match std::fs::read_dir(&memory_dir) {
            Ok(entries) => {
                let mut files: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                files.sort_by_key(|e| e.file_name());

                for entry in files {
                    let path = entry.path();
                    if path.is_file() {
                        let name = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("?");
                        let size = std::fs::metadata(&path)
                            .map(|m| m.len())
                            .unwrap_or(0);
                        lines.push(format!("    {} ({} bytes)", name, size));
                        found = true;
                    }
                }
            }
            Err(e) => {
                return Ok(CommandResult::Text(format!(
                    "  Failed to read memory directory: {}",
                    e
                )));
            }
        }

        if !found {
            lines.push("    (no memory files yet)".to_string());
        }

        Ok(CommandResult::Text(lines.join("\n")))
    }
}

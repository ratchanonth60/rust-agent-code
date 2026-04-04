//! Command registry — stores and looks up slash commands.

use super::types::Command;

/// Holds all registered slash commands and provides lookup by name/alias.
#[derive(Default)]
pub struct CommandRegistry {
    commands: Vec<Box<dyn Command>>,
}

impl CommandRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    /// Register a command.
    pub fn register(&mut self, cmd: Box<dyn Command>) {
        self.commands.push(cmd);
    }

    /// Find a command by name or alias (without the leading `/`).
    pub fn find(&self, name: &str) -> Option<&dyn Command> {
        self.commands.iter().find_map(|cmd| {
            if cmd.name() == name || cmd.aliases().contains(&name) {
                Some(cmd.as_ref())
            } else {
                None
            }
        })
    }

    /// List all registered, enabled commands.
    pub fn list(&self) -> Vec<&dyn Command> {
        self.commands
            .iter()
            .filter(|c| c.is_enabled())
            .map(|c| c.as_ref())
            .collect()
    }
}

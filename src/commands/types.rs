//! Command trait and supporting types for the slash command system.

use std::sync::{Arc, Mutex};

/// The type of command determines how its result is handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandType {
    /// Executes locally and returns text to display in the TUI.
    Local,
    /// Generates a prompt that is sent to the LLM for processing.
    Prompt,
}

/// The result of executing a command.
pub enum CommandResult {
    /// Display text directly in the TUI.
    Text(String),
    /// Send a prompt to the LLM (for prompt-type commands).
    Prompt(PromptCommand),
    /// Command was handled, no output needed.
    Silent,
    /// Request the application to exit.
    Exit,
    /// Clear the conversation.
    Clear,
}

/// A prompt-type command result with the prompt content and optional tool restrictions.
pub struct PromptCommand {
    /// The prompt content to send to the LLM.
    pub content: String,
    /// If set, only these tools are available during this query.
    pub allowed_tools: Option<Vec<String>>,
}

/// Context available to commands during execution.
pub struct CommandContext {
    /// The cost tracker for displaying session costs.
    pub cost_tracker: Option<Arc<Mutex<crate::engine::cost_tracker::CostTracker>>>,
    /// Current working directory.
    pub cwd: std::path::PathBuf,
}

/// Trait that all slash commands must implement.
pub trait Command: Send + Sync {
    /// The slash command name (without the leading `/`).
    fn name(&self) -> &str;

    /// A short description shown in `/help`.
    fn description(&self) -> &str;

    /// Alternative names for this command.
    fn aliases(&self) -> Vec<&str> {
        vec![]
    }

    /// The type of command.
    fn command_type(&self) -> CommandType;

    /// Whether this command is currently enabled.
    fn is_enabled(&self) -> bool {
        true
    }

    /// Optional hint for arguments shown in `/help`.
    fn argument_hint(&self) -> Option<&str> {
        None
    }

    /// Execute the command with the given arguments.
    fn execute(&self, args: &str, ctx: &CommandContext) -> anyhow::Result<CommandResult>;
}

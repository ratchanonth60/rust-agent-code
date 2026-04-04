//! Slash command system — trait-based, extensible command registry.
//!
//! Commands are registered via [`build_default_registry`] and looked up by
//! name in the TUI's [`crate::ui::app::App::handle_slash_command`].

pub mod types;
pub mod registry;

// ── Local commands ──────────────────────────────────────────────────────
pub mod branch;
pub mod clear;
pub mod config_cmd;
pub mod context;
pub mod cost;
pub mod diff;
pub mod doctor;
pub mod effort;
pub mod exit;
pub mod export;
pub mod fast;
pub mod help;
pub mod keybindings_cmd;
pub mod memory;
pub mod model;
pub mod output_style;
pub mod permissions_cmd;
pub mod plan;
pub mod stats;
pub mod status;
pub mod vim;

// ── Prompt commands ─────────────────────────────────────────────────────
pub mod commit;
pub mod compact;
pub mod review;
pub mod resume;
pub mod theme;

pub use registry::CommandRegistry;
pub use types::{Command, CommandContext, CommandResult, CommandType, PromptCommand};

/// Build a registry with all built-in commands pre-registered.
pub fn build_default_registry() -> CommandRegistry {
    let mut reg = CommandRegistry::new();

    // Core
    reg.register(Box::new(help::HelpCommand));
    reg.register(Box::new(clear::ClearCommand));
    reg.register(Box::new(cost::CostCommand));
    reg.register(Box::new(exit::ExitCommand));

    // Configuration & mode
    reg.register(Box::new(config_cmd::ConfigCommand));
    reg.register(Box::new(model::ModelCommand));
    reg.register(Box::new(theme::ThemeCommand));
    reg.register(Box::new(output_style::OutputStyleCommand));
    reg.register(Box::new(vim::VimCommand));
    reg.register(Box::new(effort::EffortCommand));
    reg.register(Box::new(fast::FastCommand));
    reg.register(Box::new(plan::PlanCommand));
    reg.register(Box::new(permissions_cmd::PermissionsCommand));

    // Information
    reg.register(Box::new(stats::StatsCommand));
    reg.register(Box::new(status::StatusCommand));
    reg.register(Box::new(context::ContextCommand));
    reg.register(Box::new(keybindings_cmd::KeybindingsCommand));
    reg.register(Box::new(doctor::DoctorCommand));
    reg.register(Box::new(memory::MemoryCommand));

    // Git
    reg.register(Box::new(diff::DiffCommand));
    reg.register(Box::new(branch::BranchCommand));
    reg.register(Box::new(commit::CommitCommand));
    reg.register(Box::new(review::ReviewCommand));

    // Session
    reg.register(Box::new(compact::CompactCommand));
    reg.register(Box::new(export::ExportCommand));
    reg.register(Box::new(resume::ResumeCommand));

    reg
}

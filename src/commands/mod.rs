//! Slash command system — trait-based, extensible command registry.
//!
//! Commands are registered via [`build_default_registry`] and looked up by
//! name in the TUI's [`crate::ui::app::App::handle_slash_command`].

pub mod types;
pub mod registry;

// ── Auth ────────────────────────────────────────────────────────────────
pub mod auth_status;
pub mod login;
pub mod logout;

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
pub mod mcp;
pub mod memory;
pub mod model;
pub mod output_style;
pub mod permissions_cmd;
pub mod plan;
pub mod settings_cmd;
pub mod stats;
pub mod status;
pub mod vim;

// ── Prompt commands ─────────────────────────────────────────────────────
pub mod commit;
pub mod compact;
pub mod review;
pub mod resume;
pub mod skill;
pub mod theme;

pub use registry::CommandRegistry;
pub use types::{Command, CommandContext, CommandResult, CommandType, PromptCommand};

/// Build a registry with all built-in commands pre-registered.
pub fn build_default_registry() -> CommandRegistry {
    let mut reg = CommandRegistry::new();

    // Auth
    reg.register(Box::new(auth_status::AuthStatusCommand));
    reg.register(Box::new(login::LoginCommand));
    reg.register(Box::new(logout::LogoutCommand));

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
    reg.register(Box::new(settings_cmd::SettingsCommand));

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

    // MCP
    reg.register(Box::new(mcp::McpCommand));

    // Skills
    reg.register(Box::new(skill::SkillCommand));

    reg
}

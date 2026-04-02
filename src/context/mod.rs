//! Context injection — CLAUDE.md files, git status, and system info.
//!
//! [`build_context_prompt`] assembles the full block that is prepended
//! to the system prompt before every LLM call.

pub mod claudemd;
pub mod git;
pub mod system_info;

/// Builds the full context block that gets injected into the system prompt.
pub fn build_context_prompt(cwd: &std::path::Path) -> String {
    let mut parts = Vec::new();

    // CLAUDE.md files
    let claudemd = claudemd::load_claudemd_files(cwd);
    if !claudemd.is_empty() {
        parts.push(format!("# Project Instructions (CLAUDE.md)\n\n{}", claudemd));
    }

    // Git context
    let git_ctx = git::get_git_context(cwd);
    if !git_ctx.is_empty() {
        parts.push(format!("# Git Context\n\n{}", git_ctx));
    }

    // System info
    let sys_info = system_info::get_system_info(cwd);
    parts.push(format!("# Environment\n\n{}", sys_info));

    parts.join("\n\n---\n\n")
}

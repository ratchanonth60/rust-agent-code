//! `/doctor` — check system health: git, API keys, tools.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct DoctorCommand;

impl Command for DoctorCommand {
    fn name(&self) -> &str {
        "doctor"
    }

    fn description(&self) -> &str {
        "Check system health and dependencies"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, _args: &str, ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let mut lines = vec![
            "  System Health Check".to_string(),
            "  ===================".to_string(),
            check_tool("git", &["--version"], &ctx.cwd),
            check_tool("rg", &["--version"], &ctx.cwd),
            check_tool("bash", &["--version"], &ctx.cwd),
        ];

        // Git repository status
        let git_repo = std::process::Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .current_dir(&ctx.cwd)
            .output()
            .ok()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if git_repo {
            lines.push("  [ok]   Git repository detected".to_string());
        } else {
            lines.push("  [warn] Not inside a git repository".to_string());
        }

        // API keys
        lines.push(String::new());
        lines.push("  API Keys".to_string());
        lines.push("  --------".to_string());
        check_env_var("GEMINI_API_KEY", &mut lines);
        check_env_var("OPENAI_API_KEY", &mut lines);
        check_env_var("ANTHROPIC_API_KEY", &mut lines);

        // Config file
        lines.push(String::new());
        lines.push("  Configuration".to_string());
        lines.push("  -------------".to_string());
        let config_path = crate::config::config_path();
        if config_path.exists() {
            lines.push(format!("  [ok]   Config file: {}", config_path.display()));
        } else {
            lines.push(format!(
                "  [info] No config file (defaults will be used): {}",
                config_path.display()
            ));
        }

        // Memory directory
        let mem_dir = crate::mem::get_auto_mem_path();
        if mem_dir.exists() {
            lines.push(format!("  [ok]   Memory directory: {}", mem_dir.display()));
        } else {
            lines.push(format!(
                "  [info] No memory directory yet: {}",
                mem_dir.display()
            ));
        }

        Ok(CommandResult::Text(lines.join("\n")))
    }
}

/// Check if a CLI tool is available and return a status line.
fn check_tool(name: &str, args: &[&str], cwd: &std::path::Path) -> String {
    match std::process::Command::new(name)
        .args(args)
        .current_dir(cwd)
        .output()
    {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            let first_line = version.lines().next().unwrap_or("").trim();
            format!("  [ok]   {}: {}", name, first_line)
        }
        Ok(_) => format!("  [fail] {}: installed but returned error", name),
        Err(_) => format!("  [fail] {}: not found in PATH", name),
    }
}

/// Check if an environment variable is set and add a status line.
fn check_env_var(name: &str, lines: &mut Vec<String>) {
    match std::env::var(name) {
        Ok(val) if !val.is_empty() => {
            let masked = format!("{}...{}", &val[..4.min(val.len())], &val[val.len().saturating_sub(4)..]);
            lines.push(format!("  [ok]   {}: set ({})", name, masked));
        }
        _ => {
            lines.push(format!("  [miss] {}: not set", name));
        }
    }
}

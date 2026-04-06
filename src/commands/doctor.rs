//! `/doctor` — check system health: git, API keys, tools.

use std::fmt::Write;

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
        let mut output = String::with_capacity(1024);

        writeln!(output, "  System Health Check")?;
        writeln!(output, "  ===================")?;

        // 1. Tool checks
        for tool in ["git", "rg", "bash"] {
            writeln!(output, "{}", check_tool(tool, &["--version"], &ctx.cwd))?;
        }

        // 2. Git repository status
        let is_git = std::process::Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .current_dir(&ctx.cwd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        writeln!(
            output,
            "{}",
            if is_git {
                "  [ok]   Git repository detected"
            } else {
                "  [warn] Not inside a git repository"
            }
        )?;

        // 3. API keys
        writeln!(output, "\n  API Keys\n  --------")?;
        for key in ["GEMINI_API_KEY", "OPENAI_API_KEY", "ANTHROPIC_API_KEY"] {
            writeln!(output, "{}", check_env_var(key))?;
        }

        // 4. OAuth credentials
        writeln!(output, "\n  OAuth Credentials\n  -----------------")?;
        self.check_oauth(&mut output);

        // 5. Configuration & memory
        writeln!(output, "\n  Configuration\n  -------------")?;
        let config_path = crate::config::config_path();
        if config_path.exists() {
            writeln!(output, "  [ok]   Config file: {}", config_path.display())?;
        } else {
            writeln!(
                output,
                "  [info] No config file (defaults will be used): {}",
                config_path.display()
            )?;
        }

        let mem_dir = crate::mem::get_auto_mem_path();
        if mem_dir.exists() {
            writeln!(output, "  [ok]   Memory directory: {}", mem_dir.display())?;
        } else {
            writeln!(
                output,
                "  [info] No memory directory yet: {}",
                mem_dir.display()
            )?;
        }

        Ok(CommandResult::Text(output))
    }
}

impl DoctorCommand {
    fn check_oauth(&self, f: &mut String) {
        use crate::auth::credentials::CredentialStore;
        match CredentialStore::load() {
            Ok(store) => {
                let status = match store.get_token("gemini") {
                    Some(cred) if cred.is_expired() => {
                        "  [warn] Gemini: OAuth token expired (will auto-refresh)"
                    }
                    Some(cred) if cred.needs_refresh() => {
                        "  [ok]   Gemini: OAuth token valid (refresh soon)"
                    }
                    Some(_) => "  [ok]   Gemini: OAuth token valid",
                    None => "  [info] Gemini: No OAuth token (use /login gemini)",
                };
                let _ = writeln!(f, "{}", status);
            }
            Err(_) => {
                let _ = writeln!(f, "  [info] No credentials file found");
            }
        }
    }
}

fn check_tool(name: &str, args: &[&str], cwd: &std::path::Path) -> String {
    std::process::Command::new(name)
        .args(args)
        .current_dir(cwd)
        .output()
        .map(|output| {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout);
                format!(
                    "  [ok]   {}: {}",
                    name,
                    version.lines().next().unwrap_or("").trim()
                )
            } else {
                format!("  [fail] {}: installed but returned error", name)
            }
        })
        .unwrap_or_else(|_| format!("  [fail] {}: not found in PATH", name))
}

fn check_env_var(name: &str) -> String {
    match std::env::var(name) {
        Ok(val) if !val.is_empty() => {
            let masked = if val.len() <= 8 {
                "****".to_string()
            } else {
                format!("{}...{}", &val[..4], &val[val.len() - 4..])
            };
            format!("  [ok]   {}: set ({})", name, masked)
        }
        _ => format!("  [miss] {}: not set", name),
    }
}

//! `/auth-status` — show authentication status for all providers.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct AuthStatusCommand;

impl Command for AuthStatusCommand {
    fn name(&self) -> &str {
        "auth-status"
    }

    fn description(&self) -> &str {
        "Show authentication status for all providers"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["auth"]
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, _args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let mut lines = vec![
            "  Authentication Status".to_string(),
            "  ====================".to_string(),
        ];

        // OAuth credentials
        let store = crate::auth::credentials::CredentialStore::load().unwrap_or_default();

        if let Some(cred) = store.get_token("gemini") {
            let status = if cred.is_expired() {
                "expired"
            } else if cred.needs_refresh() {
                "refresh"
            } else {
                "active"
            };
            let masked = mask_token(&cred.access_token);
            lines.push(format!("  [{status}] Gemini OAuth: {masked}"));
        } else {
            lines.push("  [miss] Gemini OAuth: not configured (use /login gemini)".to_string());
        }

        // Env var fallbacks
        lines.push(String::new());
        lines.push("  Environment Variables".to_string());
        lines.push("  --------------------".to_string());
        check_env("GEMINI_API_KEY", &mut lines);
        check_env("ANTHROPIC_API_KEY", &mut lines);
        check_env("OPENAI_API_KEY", &mut lines);

        Ok(CommandResult::Text(lines.join("\n")))
    }
}

fn mask_token(token: &str) -> String {
    if token.len() < 12 {
        return "***".to_string();
    }
    format!("{}...{}", &token[..6], &token[token.len() - 4..])
}

fn check_env(name: &str, lines: &mut Vec<String>) {
    match std::env::var(name) {
        Ok(val) if !val.is_empty() => {
            let masked = mask_token(&val);
            lines.push(format!("  [ok]   {name}: {masked}"));
        }
        _ => {
            lines.push(format!("  [miss] {name}: not set"));
        }
    }
}

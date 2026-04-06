//! `/logout` — remove stored OAuth2 credentials for a provider.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct LogoutCommand;

impl Command for LogoutCommand {
    fn name(&self) -> &str {
        "logout"
    }

    fn description(&self) -> &str {
        "Remove stored OAuth2 credentials"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["deauth"]
    }

    fn argument_hint(&self) -> Option<&str> {
        Some("[provider]")
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let provider = args.trim();
        let provider = if provider.is_empty() { "gemini" } else { provider };

        match provider {
            "gemini" | "google" => {
                let mut store = crate::auth::credentials::CredentialStore::load()?;
                if let Some(cred) = store.get_token("gemini") {
                    let token = cred.access_token.clone();
                    let config = crate::auth::client_config::load_gemini_config();
                    // Best-effort revocation.
                    let _ = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current()
                            .block_on(crate::auth::oauth::revoke_token(&config, &token))
                    });
                    store.remove_token("gemini");
                    store.save()?;
                    Ok(CommandResult::Text(
                        "  Logged out from Gemini. OAuth credentials removed.".to_string(),
                    ))
                } else {
                    Ok(CommandResult::Text(
                        "  No Gemini OAuth credentials found.".to_string(),
                    ))
                }
            }
            "claude" | "anthropic" => {
                let mut store = crate::auth::credentials::CredentialStore::load()?;
                if store.get_token("claude").is_some() {
                    // No revocation endpoint for Anthropic — just remove local token.
                    store.remove_token("claude");
                    store.save()?;
                    Ok(CommandResult::Text(
                        "  Logged out from Claude. OAuth credentials removed.".to_string(),
                    ))
                } else {
                    Ok(CommandResult::Text(
                        "  No Claude OAuth credentials found.".to_string(),
                    ))
                }
            }
            _ => Ok(CommandResult::Text(format!(
                "  Unknown provider: '{provider}'\n  Supported: gemini, claude"
            ))),
        }
    }
}

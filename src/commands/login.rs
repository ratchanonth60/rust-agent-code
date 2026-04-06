//! `/login` — authenticate with a provider via OAuth2.

use super::types::{Command, CommandContext, CommandResult, CommandType};

pub struct LoginCommand;

impl Command for LoginCommand {
    fn name(&self) -> &str {
        "login"
    }

    fn description(&self) -> &str {
        "Authenticate with a provider via OAuth2"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["auth"]
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
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current()
                        .block_on(crate::auth::oauth::run_oauth_flow("gemini"))
                });

                match result {
                    Ok(()) => Ok(CommandResult::Text(
                        "  Logged in to Gemini.\n  \
                         OAuth token saved to ~/.rust-agent/credentials.json"
                            .to_string(),
                    )),
                    Err(e) => Ok(CommandResult::Text(format!("  Login failed: {e}"))),
                }
            }
            "claude" | "anthropic" => {
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current()
                        .block_on(crate::auth::oauth::run_oauth_flow("claude"))
                });

                match result {
                    Ok(()) => Ok(CommandResult::Text(
                        "  Logged in to Claude.\n  \
                         OAuth token saved to ~/.rust-agent/credentials.json"
                            .to_string(),
                    )),
                    Err(e) => Ok(CommandResult::Text(format!("  Login failed: {e}"))),
                }
            }
            _ => Ok(CommandResult::Text(format!(
                "  Unknown provider: '{provider}'\n  Supported: gemini, claude"
            ))),
        }
    }
}

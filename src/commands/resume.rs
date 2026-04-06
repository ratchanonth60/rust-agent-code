//! `/resume` — resume a previous session.

use super::types::{Command, CommandContext, CommandResult, CommandType};
use std::fmt::Write;
pub struct ResumeCommand;

impl Command for ResumeCommand {
    fn name(&self) -> &str {
        "resume"
    }

    fn description(&self) -> &str {
        "Resume a previous session"
    }

    fn argument_hint(&self) -> Option<&str> {
        Some("[id]")
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn execute(&self, args: &str, _ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let args = args.trim();

        // 1. Fetch sessions once and handle error early (Early Return)
        let session = match crate::engine::session::Session::list_sessions() {
            Ok(s) => s,
            Err(e) => {
                return Ok(CommandResult::Text(format!(
                    "  Failed to list sessions: {}",
                    e
                )))
            }
        };
        // 2. Branch: List available sessions
        if args.is_empty() {
            if session.is_empty() {
                return Ok(CommandResult::Text(
                    "  No saved sessions found.".to_string(),
                ));
            }
            let mut output = String::with_capacity(512); // Pre-allocate
            writeln!(output, "  Available sessions:\n  -------------------")?;

            for s in session.iter().take(20) {
                let summary = s.summary.as_deref().unwrap_or("(no summary)");
                let ts = format_timestamp(s.created_at);
                let id_preview = &s.id[..8.min(s.id.len())];
                writeln!(
                    output,
                    " {} {} {} msgs {} {}",
                    id_preview, ts, s.message_count, s.model, summary,
                )?;
            }
            write!(output, "\n Usage: /resume <session-id>")?;
            return Ok(CommandResult::Text(output));
        }
        // 3. Branch: Resume a specific session
        let matched: Vec<_> = session
            .into_iter()
            .filter(|s| s.id.starts_with(args))
            .collect();

        match matched.len() {
            0 => Ok(CommandResult::Text(format!(
                " No session found matching '{}'.",
                args,
            ))),
            1 => {
                let session_id = &matched[0].id;
                match crate::engine::session::Session::load(session_id) {
                    Ok(session) => Ok(CommandResult::ResumeSession {
                        session_id: session.id,
                        messages: session.messages,
                        model: session.model,
                        provider: session.provider,
                    }),
                    Err(e) => Ok(CommandResult::Text(format!(
                        " Failed to load session '{}': {}",
                        session_id, e
                    ))),
                }
            }
            _ => {
                let ids: Vec<&str> = matched.iter().map(|s| s.id.as_str()).collect();
                Ok(CommandResult::Text(format!(
                    " Multiple session match '{}': {:?}\n Please be more specific.",
                    args, ids
                )))
            }
        }
    }
}

/// Format a unix timestamp into a human-readable string.
fn format_timestamp(ts: u64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    let dt = UNIX_EPOCH + Duration::from_secs(ts);
    // 4. Use match with range patterns for cleaner logic
    if let Ok(elapsed) = dt.elapsed() {
        let secs = elapsed.as_secs();
        match secs {
            0..=59 => "just now".to_string(),
            60..=3599 => format!("{}m ago", secs / 60),
            3600..=86399 => format!("{}h ago", secs / 3600),
            _ => format!("{}d ago", secs / 86400),
        }
    } else {
        "future".to_string()
    }
}

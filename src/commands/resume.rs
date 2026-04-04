//! `/resume` — resume a previous session.

use super::types::{Command, CommandContext, CommandResult, CommandType};

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

        if args.is_empty() {
            // List available sessions.
            match crate::engine::session::Session::list_sessions() {
                Ok(sessions) if sessions.is_empty() => {
                    Ok(CommandResult::Text(
                        "  No saved sessions found.".to_string(),
                    ))
                }
                Ok(sessions) => {
                    let mut lines = Vec::new();
                    lines.push("  Available sessions:".to_string());
                    lines.push("  -------------------".to_string());
                    for s in sessions.iter().take(20) {
                        let summary = s
                            .summary
                            .as_deref()
                            .unwrap_or("(no summary)");
                        let ts = format_timestamp(s.created_at);
                        lines.push(format!(
                            "  {}  {}  {} msgs  {}  {}",
                            &s.id[..8.min(s.id.len())],
                            ts,
                            s.message_count,
                            s.model,
                            summary,
                        ));
                    }
                    lines.push(String::new());
                    lines.push("  Usage: /resume <session-id>".to_string());
                    Ok(CommandResult::Text(lines.join("\n")))
                }
                Err(e) => Ok(CommandResult::Text(format!(
                    "  Failed to list sessions: {}",
                    e
                ))),
            }
        } else {
            // Try to find a session matching the given ID prefix.
            match crate::engine::session::Session::list_sessions() {
                Ok(sessions) => {
                    let matched: Vec<_> = sessions
                        .iter()
                        .filter(|s| s.id.starts_with(args))
                        .collect();
                    match matched.len() {
                        0 => Ok(CommandResult::Text(format!(
                            "  No session found matching '{}'.",
                            args
                        ))),
                        1 => {
                            let s = &matched[0];
                            match crate::engine::session::Session::load(&s.id) {
                                Ok(session) => {
                                    let summary = session.summary.as_deref().unwrap_or("(no summary)");
                                    let mut lines = Vec::new();
                                    lines.push(format!("  Resuming session: {}", session.id));
                                    lines.push(format!("  Model: {}  |  Provider: {}", session.model, session.provider));
                                    lines.push(format!("  Messages: {}  |  CWD: {}", session.messages.len(), session.cwd));
                                    lines.push(format!("  Summary: {}", summary));
                                    lines.push(String::new());

                                    // Show last few conversation turns for context
                                    let recent: Vec<_> = session.messages.iter().rev().take(6).collect();
                                    if !recent.is_empty() {
                                        lines.push("  Recent conversation:".to_string());
                                        lines.push("  ───────────────────".to_string());
                                        for msg in recent.iter().rev() {
                                            let role = msg["role"].as_str().unwrap_or("?");
                                            let content = msg.get("content")
                                                .and_then(|c| {
                                                    if let Some(s) = c.as_str() {
                                                        Some(s.to_string())
                                                    } else if let Some(arr) = c.as_array() {
                                                        // Claude format: array of content blocks
                                                        let texts: Vec<String> = arr.iter()
                                                            .filter_map(|b| b["text"].as_str().map(String::from))
                                                            .collect();
                                                        if texts.is_empty() { None } else { Some(texts.join(" ")) }
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .unwrap_or_else(|| "(tool use)".to_string());
                                            let truncated = if content.len() > 120 {
                                                format!("{}...", &content[..120])
                                            } else {
                                                content
                                            };
                                            lines.push(format!("    [{}] {}", role, truncated));
                                        }
                                    }

                                    Ok(CommandResult::Text(lines.join("\n")))
                                }
                                Err(e) => Ok(CommandResult::Text(format!(
                                    "  Failed to load session '{}': {}",
                                    s.id, e
                                ))),
                            }
                        }
                        _ => {
                            let ids: Vec<&str> = matched.iter().map(|s| s.id.as_str()).collect();
                            Ok(CommandResult::Text(format!(
                                "  Multiple sessions match '{}': {:?}\n  Please be more specific.",
                                args, ids
                            )))
                        }
                    }
                }
                Err(e) => Ok(CommandResult::Text(format!(
                    "  Failed to list sessions: {}",
                    e
                ))),
            }
        }
    }
}

/// Format a unix timestamp into a human-readable string.
fn format_timestamp(ts: u64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    let dt = UNIX_EPOCH + Duration::from_secs(ts);
    // Simple formatting without chrono — just show seconds since epoch in a readable way.
    match dt.elapsed() {
        Ok(elapsed) => {
            let secs = elapsed.as_secs();
            if secs < 60 {
                "just now".to_string()
            } else if secs < 3600 {
                format!("{}m ago", secs / 60)
            } else if secs < 86400 {
                format!("{}h ago", secs / 3600)
            } else {
                format!("{}d ago", secs / 86400)
            }
        }
        Err(_) => "future".to_string(),
    }
}

//! Slash command dispatch — parsing and executing `/commands` from user input.
//!
//! When the user types a line beginning with `/`, it is routed here instead of
//! being sent to the LLM engine. This module handles:
//!
//! - **Special-case commands**: `/help` (needs registry access), dialog openers
//!   (`/settings`, `/model`, `/theme`, `/resume`).
//! - **Registry lookup**: Finds the matching [`Command`] by name or alias.
//! - **Result handling**: Maps [`CommandResult`] variants to UI actions (display
//!   text, clear conversation, exit, etc.).
//!
//! # Dialog shortcut commands
//!
//! Some commands bypass the registry and directly open a dialog overlay:
//!
//! | Input         | Effect                          |
//! |---------------|---------------------------------|
//! | `/settings`   | Opens the full settings editor  |
//! | `/model`      | Opens the model picker dialog   |
//! | `/theme`      | Opens the theme picker dialog   |
//! | `/resume`     | Opens the session picker dialog |
//!
//! If `/model` or `/theme` are invoked with arguments (e.g., `/model gpt-4o`),
//! they fall through to the registry command for direct value setting.
//! `/resume <id>` falls through to the registry for direct session loading.

use crate::commands::{CommandContext, CommandResult};
use crate::ui::dialogs::ActiveDialog;

use super::{App, MessageEntry};

impl App {
    /// Parse and execute a slash command, then append resulting UI messages.
    ///
    /// # Parameters
    ///
    /// - `cmd`: Full user input beginning with `/` (e.g., `"/help"`, `"/model gpt-4o"`).
    ///
    /// # Command resolution order
    ///
    /// 1. `/help` — special-cased because it needs `command_registry.list()`
    /// 2. Dialog commands — `/settings`, `/model` (no args), `/theme` (no args), `/resume` (no args)
    /// 3. Registry lookup — finds command by name or alias
    /// 4. Unknown command — displays error message
    pub(super) fn handle_slash_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let command_name = parts
            .first()
            .copied()
            .unwrap_or("")
            .trim_start_matches('/');
        let args = if parts.len() > 1 {
            parts[1..].join(" ")
        } else {
            String::new()
        };

        // Special case: /help needs access to the registry for listing commands.
        if command_name == "help" {
            let commands = self.command_registry.list();
            let help_text = crate::commands::help::build_help_text(&commands);
            self.messages.push(MessageEntry::System(help_text));
            self.auto_scroll();
            return;
        }

        // Dialog commands — open overlay dialogs instead of running a command.
        // /model, /theme, /resume only open dialogs when called without arguments;
        // with arguments they fall through to the registry for direct setting.
        match command_name {
            "settings" => {
                self.open_dialog(ActiveDialog::Settings);
                return;
            }
            "model" if args.is_empty() => {
                self.open_dialog(ActiveDialog::ModelPicker);
                return;
            }
            "theme" if args.is_empty() => {
                self.open_dialog(ActiveDialog::ThemePicker);
                return;
            }
            "resume" if args.is_empty() => {
                self.open_dialog(ActiveDialog::SessionPicker);
                return;
            }
            _ => {}
        }

        // Standard registry lookup — find command by name/alias and execute.
        if let Some(command) = self.command_registry.find(command_name) {
            let ctx = CommandContext {
                cost_tracker: self.cost_tracker.clone(),
                cwd: std::env::current_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from(".")),
            };

            match command.execute(&args, &ctx) {
                Ok(result) => match result {
                    CommandResult::Text(text) => {
                        self.messages.push(MessageEntry::System(text));
                    }
                    CommandResult::Clear => {
                        self.messages.clear();
                        self.scroll_offset = 0;
                        self.messages.push(MessageEntry::System(
                            "  Conversation cleared.".to_string(),
                        ));
                    }
                    CommandResult::Exit => {
                        self.exit = true;
                    }
                    CommandResult::Silent => {}
                    CommandResult::Prompt(_prompt_cmd) => {
                        // TODO: send prompt_cmd.content to engine with allowed_tools filter
                        self.messages.push(MessageEntry::System(
                            "  Prompt commands not yet wired to engine.".to_string(),
                        ));
                    }
                    CommandResult::ResumeSession {
                        session_id,
                        messages,
                        model,
                        provider,
                    } => {
                        self.apply_resume_session(session_id, messages, model, provider);
                    }
                },
                Err(e) => {
                    self.messages
                        .push(MessageEntry::Error(format!("  Command error: {}", e)));
                }
            }
        } else {
            self.messages.push(MessageEntry::System(format!(
                "  Unknown command: /{}",
                command_name
            )));
        }
        self.auto_scroll();
    }

    /// Apply a session resume — clear TUI, show recent messages, send to engine.
    ///
    /// Used by both `CommandResult::ResumeSession` (from `/resume <id>`) and
    /// the `SessionPicker` dialog.
    pub(super) fn apply_resume_session(
        &mut self,
        session_id: String,
        messages: Vec<serde_json::Value>,
        model: String,
        provider: String,
    ) {
        let msg_count = messages.len();
        self.messages.clear();
        self.scroll_offset = 0;

        // Display recent conversation context in TUI
        let recent: Vec<_> = messages.iter().rev().take(6).collect();
        for msg in recent.iter().rev() {
            let role = msg["role"].as_str().unwrap_or("?");
            let content = msg
                .get("content")
                .and_then(|c| {
                    if let Some(s) = c.as_str() {
                        Some(s.to_string())
                    } else if let Some(arr) = c.as_array() {
                        let texts: Vec<String> = arr
                            .iter()
                            .filter_map(|b| b["text"].as_str().map(String::from))
                            .collect();
                        if texts.is_empty() { None } else { Some(texts.join(" ")) }
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "(tool use)".to_string());
            let truncated = if content.len() > 200 {
                format!("{}...", &content[..200])
            } else {
                content
            };
            match role {
                "user" => self.messages.push(MessageEntry::User(truncated)),
                "assistant" => self.messages.push(MessageEntry::Assistant(truncated)),
                _ => self.messages.push(MessageEntry::System(
                    format!("  [{}] {}", role, truncated),
                )),
            }
        }

        self.messages.push(MessageEntry::System(format!(
            "  Session resumed: {} ({} messages, {} / {})",
            &session_id[..8.min(session_id.len())],
            msg_count,
            model,
            provider,
        )));

        // Send resume event to engine via special prefix
        let resume_payload = serde_json::json!({
            "__resume": true,
            "session_id": session_id,
            "messages": messages,
        });
        let _ = self.tx_to_engine.try_send(
            format!("__resume:{}", resume_payload)
        );
    }
}

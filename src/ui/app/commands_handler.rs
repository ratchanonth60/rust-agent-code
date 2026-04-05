//! Slash command dispatch — parsing and executing `/commands` from user input.
//!
//! When the user types a line beginning with `/`, it is routed here instead of
//! being sent to the LLM engine. This module handles:
//!
//! - **Special-case commands**: `/help` (needs registry access), dialog openers
//!   (`/settings`, `/model`, `/theme`).
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
//!
//! If `/model` or `/theme` are invoked with arguments (e.g., `/model gpt-4o`),
//! they fall through to the registry command for direct value setting.

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
    /// 2. Dialog commands — `/settings`, `/model` (no args), `/theme` (no args)
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
        // /model and /theme only open dialogs when called without arguments;
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
}

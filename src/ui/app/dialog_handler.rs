//! Dialog overlay management — opening, routing keys, and processing results.
//!
//! This module handles the lifecycle of dialog overlays in the TUI:
//!
//! - **Opening**: Instantiates the appropriate dialog widget (settings, model picker,
//!   theme picker) and sets it as the active overlay.
//! - **Key routing**: When a dialog is active, all key events are forwarded to the
//!   dialog's `handle_key()` method instead of the normal input handler.
//! - **Result processing**: When a dialog closes with a selection, the result is
//!   applied (e.g., persisting a model change to `~/.rust-agent/config.json`).
//!
//! # Dialog lifecycle
//!
//! ```text
//! /settings command  →  open_dialog(Settings)  →  SettingsDialog::new()
//!                                                        ↓
//!                                               dialog_widget = Some(...)
//!                                               active_dialog = Settings
//!                                                        ↓
//!                                               Key events routed to dialog
//!                                                        ↓
//!                                               Esc → save_settings() + Select("saved")
//!                                                        ↓
//!                                               handle_dialog_result("saved")
//!                                               active_dialog = None
//! ```

use crate::ui::dialogs::{ActiveDialog, Dialog};

use super::{App, MessageEntry};

impl App {
    /// Open a dialog overlay, replacing any currently active dialog.
    ///
    /// Instantiates the correct dialog widget based on the [`ActiveDialog`] variant
    /// and stores it in `self.dialog_widget`. While a dialog is active, the main
    /// event loop routes all key events to the dialog instead of normal input.
    ///
    /// Passing `ActiveDialog::None` is a no-op.
    pub fn open_dialog(&mut self, dialog: ActiveDialog) {
        let widget: Box<dyn Dialog> = match &dialog {
            ActiveDialog::None => return,
            ActiveDialog::ModelPicker => {
                Box::new(crate::ui::dialogs::model_picker::ModelPickerDialog::new())
            }
            ActiveDialog::ThemePicker => {
                Box::new(crate::ui::dialogs::theme_picker::ThemePickerDialog::new())
            }
            ActiveDialog::Settings => {
                Box::new(crate::ui::dialogs::settings_dialog::SettingsDialog::new())
            }
        };
        self.active_dialog = dialog;
        self.dialog_widget = Some(widget);
    }

    /// Close the active dialog and clear the widget reference.
    ///
    /// Called after a dialog emits `DialogAction::Select` or `DialogAction::Cancel`.
    pub fn close_dialog(&mut self) {
        self.active_dialog = ActiveDialog::None;
        self.dialog_widget = None;
    }

    /// Process the selected value after a dialog closes.
    ///
    /// Each dialog type produces a different kind of result:
    ///
    /// - **ModelPicker**: The selected model name → saved to `default_model` in config.
    /// - **ThemePicker**: The selected theme name → saved to `theme` in config.
    /// - **Settings**: Always `"saved"` → confirmation message displayed.
    ///
    /// The result is persisted to `~/.rust-agent/config.json` and a system message
    /// is pushed to the conversation timeline.
    pub(super) fn handle_dialog_result(&mut self, value: &str) {
        match &self.active_dialog {
            ActiveDialog::ModelPicker => {
                let mut cfg = crate::config::GlobalConfig::load();
                cfg.default_model = Some(value.to_string());
                let _ = cfg.save();
                self.messages.push(MessageEntry::System(format!(
                    "  Model set to: {}",
                    value
                )));
            }
            ActiveDialog::ThemePicker => {
                let mut cfg = crate::config::GlobalConfig::load();
                cfg.theme = value.to_string();
                let _ = cfg.save();
                self.messages.push(MessageEntry::System(format!(
                    "  Theme set to: {}",
                    value
                )));
            }
            ActiveDialog::Settings => {
                if value == "saved" {
                    self.messages
                        .push(MessageEntry::System("  Settings saved.".to_string()));
                }
            }
            ActiveDialog::None => {}
        }
        self.auto_scroll();
    }
}

//! Dialog system — overlay widgets for interactive selection.
//!
//! Dialogs are rendered as centered overlays on top of the conversation.
//! The TUI checks [`ActiveDialog`] each frame and routes key events
//! to the active dialog instead of the normal input handler.
//!
//! # Available dialogs
//!
//! | Dialog          | Trigger      | Module              |
//! |-----------------|--------------|---------------------|
//! | Model picker    | `/model`     | [`model_picker`]    |
//! | Theme picker    | `/theme`     | [`theme_picker`]    |
//! | Settings editor | `/settings`  | [`settings_dialog`] |
//!
//! # Adding a new dialog
//!
//! 1. Create a new module (e.g., `my_dialog.rs`) implementing [`Dialog`].
//! 2. Add a variant to [`ActiveDialog`].
//! 3. Register the variant in `App::open_dialog()` (in `app/dialog_handler.rs`).
//! 4. Handle the result in `App::handle_dialog_result()`.

/// API key setup dialog — first-run prompt for entering API key or OAuth login.
pub mod api_key_setup;
/// Model selection dialog — pick an LLM model grouped by provider.
pub mod model_picker;
/// Session picker dialog — select a session to resume.
pub mod session_picker;
/// Full settings editor — edit all config values with grouped categories.
pub mod settings_dialog;
/// Theme selection dialog — pick a color theme or output style.
pub mod theme_picker;

use ratatui::{
    layout::Rect,
    Frame,
};

use crate::engine::ModelProvider;

/// Which dialog is currently active (if any).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ActiveDialog {
    /// No dialog open.
    #[default]
    None,
    /// Model selection dialog.
    ModelPicker,
    /// Theme selection dialog.
    ThemePicker,
    /// Full settings dialog.
    Settings,
    /// Session picker dialog (resume).
    SessionPicker,
    /// API key setup dialog (first-run).
    ApiKeySetup(ModelProvider),
}

/// Common trait for dialog widgets.
pub trait Dialog {
    /// Handle a key event. Returns `true` if the dialog consumed the event.
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> DialogAction;

    /// Render the dialog as an overlay.
    fn render(&self, f: &mut Frame, area: Rect);

    /// Get the dialog title.
    fn title(&self) -> &str;
}

/// Action returned by a dialog after handling a key event.
#[derive(Debug, Clone)]
pub enum DialogAction {
    /// Keep the dialog open, event was consumed.
    Continue,
    /// Close the dialog with the selected value.
    Select(String),
    /// Close the dialog without selecting.
    Cancel,
}

/// Compute a centered rectangle for a dialog overlay.
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

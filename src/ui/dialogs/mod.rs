//! Dialog system — overlay widgets for interactive selection.
//!
//! Dialogs are rendered as centered overlays on top of the conversation.
//! The TUI checks [`ActiveDialog`] each frame and routes key events
//! to the active dialog instead of the normal input handler.

pub mod model_picker;
pub mod theme_picker;

use ratatui::{
    layout::Rect,
    Frame,
};

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

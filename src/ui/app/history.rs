//! Input history — up/down arrow recall of previous user prompts.
//!
//! Maintains a chronological list of submitted chat inputs and supports
//! browsing through them with up/down navigation. The current draft
//! is preserved when entering history mode and restored when exiting.
//!
//! # Navigation model
//!
//! ```text
//! [oldest] ← ← ← [newest]  [draft]
//!     ↑ Up              Down ↓
//! ```
//!
//! - **Up arrow** moves toward older entries, capturing the draft on first press.
//! - **Down arrow** moves toward newer entries; past the newest returns to draft.
//! - Submitting a prompt or pressing Esc resets navigation and clears the draft.

use super::App;

impl App {
    /// Insert a submitted prompt into history while avoiding adjacent duplicates.
    ///
    /// Duplicate suppression prevents the same prompt from filling the history
    /// when the user resubmits the same query multiple times in a row.
    pub(super) fn push_history_entry(&mut self, submitted: String) {
        let is_duplicate = self
            .input_history
            .last()
            .map(|s| s == &submitted)
            .unwrap_or(false);
        if !is_duplicate {
            self.input_history.push(submitted);
        }
    }

    /// Exit history-navigation mode and clear the saved draft buffer.
    ///
    /// Called when the user types a new character, submits input, or presses Esc.
    pub(super) fn reset_history_navigation(&mut self) {
        self.history_cursor = None;
        self.history_draft = None;
    }

    /// Move to an older history entry and load it into the input buffer.
    ///
    /// On the first navigation step, the current input text is captured as
    /// `history_draft` so it can be restored when the user navigates back
    /// past the newest entry. Subsequent up presses move the cursor toward
    /// index 0 (the oldest entry).
    pub(super) fn navigate_history_previous(&mut self) {
        if self.input_history.is_empty() {
            return;
        }

        let next_cursor = match self.history_cursor {
            Some(idx) => idx.saturating_sub(1),
            None => {
                // First press — save draft and jump to newest entry
                self.history_draft = Some(self.input.clone());
                self.input_history.len() - 1
            }
        };

        self.history_cursor = Some(next_cursor);
        self.input = self.input_history[next_cursor].clone();
    }

    /// Move toward newer history entries and restore draft at the end.
    ///
    /// If there is no newer entry (cursor is at the end of history),
    /// navigation mode is exited and the pre-navigation draft text is
    /// restored into `self.input`.
    pub(super) fn navigate_history_next(&mut self) {
        let Some(idx) = self.history_cursor else {
            return;
        };

        if idx + 1 < self.input_history.len() {
            let next_idx = idx + 1;
            self.history_cursor = Some(next_idx);
            self.input = self.input_history[next_idx].clone();
        } else {
            // Past the newest entry — exit history and restore draft
            self.history_cursor = None;
            self.input = self.history_draft.take().unwrap_or_default();
        }
    }
}

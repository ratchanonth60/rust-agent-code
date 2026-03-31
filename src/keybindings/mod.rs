use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// All valid keybinding action identifiers from the TS schema.
/// We implement the foundational ones here.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum KeybindingAction {
    AppInterrupt, // ctrl+c
    AppExit,      // ctrl+d
    AppRedraw,    // ctrl+l
    ChatSubmit,   // enter
    ChatCancel,   // escape
    HistoryNext,  // down
    HistoryPrev,  // up
}

/// A basic Resolver that statically maps Crossterm KeyEvents to KeybindingActions.
/// This fulfills Phase 8 initialization. Future updates can parse this from `keybindings.json`.
pub fn resolve_key_event(event: KeyEvent) -> Option<KeybindingAction> {
    match (event.code, event.modifiers) {
        // App-level events
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(KeybindingAction::AppInterrupt),
        (KeyCode::Char('d'), KeyModifiers::CONTROL) => Some(KeybindingAction::AppExit),
        (KeyCode::Char('l'), KeyModifiers::CONTROL) => Some(KeybindingAction::AppRedraw),
        
        // Chat / History events
        (KeyCode::Enter, _) => Some(KeybindingAction::ChatSubmit),
        (KeyCode::Esc, _) => Some(KeybindingAction::ChatCancel),
        (KeyCode::Up, _) => Some(KeybindingAction::HistoryPrev),
        (KeyCode::Down, _) => Some(KeybindingAction::HistoryNext),
        
        _ => None,
    }
}

//! Keybinding system — maps key events to actions across UI contexts.
//!
//! Ported from the Claude Code TypeScript keybindings module. Supports:
//! - 17 UI contexts ([`types::KeybindingContext`])
//! - 70+ actions ([`types::KeybindingAction`])
//! - Multi-keystroke chord sequences (e.g. `ctrl+x ctrl+k`)
//! - User overrides from `~/.rust-agent/keybindings.json`
//! - Last-match-wins resolution with reserved shortcut validation
//!
//! # Architecture
//!
//! ```text
//! KeyEvent (crossterm)
//!   → matcher::matches_keystroke()     // compare event against ParsedKeystroke
//!   → resolver::resolve_key()          // single-key lookup
//!   → resolver::resolve_key_with_chord_state()  // chord-aware lookup
//!   → KeybindingAction                 // dispatched to the UI layer
//! ```

pub mod default_bindings;
pub mod loader;
pub mod matcher;
pub mod parser;
pub mod reserved;
pub mod resolver;
pub mod types;

// Re-export the most commonly used items at the module level.
pub use loader::{load_keybindings, KeybindingsLoadResult, KeybindingWarning};
pub use resolver::{resolve_key, resolve_key_with_chord_state, get_binding_display_text};
pub use types::{
    ChordResolveResult, Chord, KeybindingAction, KeybindingContext,
    ParsedBinding, ParsedKeystroke,
};

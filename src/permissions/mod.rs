//! Tool permission system.
//!
//! Implements a five-mode permission model (Default, AcceptEdits,
//! BypassPermissions, Plan, DontAsk) with dangerous-path guards and
//! per-session rules that can be added via "always allow" prompts.

pub mod types;
pub mod checker;
pub mod path_safety;

pub use types::*;
pub use checker::*;

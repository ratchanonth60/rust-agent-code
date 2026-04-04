//! Engine shared state — mutable state that can be modified at runtime.
//!
//! [`SharedEngineState`] wraps the engine's mutable fields in an `Arc<RwLock<..>>`
//! so that slash commands (e.g. `/model`, `/plan`) can alter the model, provider,
//! or permission mode without restarting the engine.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use crate::engine::config::EngineConfig;
use crate::engine::cost_tracker::CostTracker;
use crate::engine::query::ModelProvider;
use crate::permissions::PermissionRule;
use crate::tools::todo::SharedTodoList;

/// Mutable runtime state shared between the engine and TUI.
pub struct EngineState {
    /// Active model name (e.g. "claude-sonnet-4-6", "gemini-2.5-flash").
    pub model: String,
    /// Active LLM provider.
    pub provider: ModelProvider,
    /// Engine configuration (auto_mode, permission_mode, etc.).
    pub config: EngineConfig,
    /// Cumulative token usage and cost.
    pub cost_tracker: Arc<Mutex<CostTracker>>,
    /// Session permission rules (e.g. "always allow" decisions).
    pub permission_rules: Arc<Mutex<Vec<PermissionRule>>>,
    /// Working directory for path safety checks.
    pub cwd: PathBuf,
    /// Shared todo list state.
    pub todo_list: SharedTodoList,
    /// Unique session identifier.
    pub session_id: String,
}

/// Thread-safe shared reference to engine state.
pub type SharedEngineState = Arc<RwLock<EngineState>>;

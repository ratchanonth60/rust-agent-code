use crate::permissions::PermissionMode;

/// Engine configuration derived from CLI arguments and environment.
///
/// Passed into [`QueryEngine::new`] so that runtime flags (auto-mode,
/// budget caps, etc.) are available throughout the query loop.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// When `true`, tools skip interactive confirmation prompts.
    pub auto_mode: bool,
    /// When `true`, run in bare/simple mode (no TUI, minimal prompts).
    pub bare_mode: bool,
    /// Enable verbose debug output.
    pub debug: bool,
    /// Optional hard budget cap in USD; the engine will stop when exceeded.
    pub max_budget_usd: Option<f64>,
    /// Maximum output tokens per LLM call.
    pub max_tokens: u32,
    /// Permission mode controlling tool authorization.
    pub permission_mode: PermissionMode,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            auto_mode: false,
            bare_mode: false,
            debug: false,
            max_budget_usd: None,
            max_tokens: 8192,
            permission_mode: PermissionMode::Default,
        }
    }
}

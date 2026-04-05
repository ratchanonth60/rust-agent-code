//! Concrete task state types for each supported [`TaskType`] variant.
//!
//! Each struct wraps a [`TaskStateBase`] (from `crate::models`) with
//! type-specific fields. The [`TaskState`] enum is the tagged union
//! used by [`TaskRegistry`](super::TaskRegistry) for polymorphic storage.
//!
//! # Extending with a new task type
//!
//! 1. Define a struct (e.g. `DreamTaskState`) containing `pub base: TaskStateBase`.
//! 2. Add a variant to [`TaskState`].
//! 3. Extend the `base()` / `base_mut()` match arms.

use std::process::Child;

use crate::models::{TaskStateBase, TaskStatus, TaskType};

// ── LocalBash ──────────────────────────────────────────────────────

/// State for a background shell task spawned via `bash -c`.
pub struct LocalBashTaskState {
    /// Shared base fields (id, status, timestamps, …).
    pub base: TaskStateBase,
    /// The shell command string that was executed.
    pub command: String,
    /// Captured stdout (populated after the process exits).
    pub stdout: String,
    /// Captured stderr (populated after the process exits).
    pub stderr: String,
    /// Handle to the child process (`None` once collected).
    pub child: Option<Child>,
    /// Whether the user explicitly backgrounded this task.
    pub backgrounded: bool,
    /// Agent ID that spawned this task (for `kill_for_agent`).
    pub agent_id: Option<String>,
}

// ── LocalAgent ─────────────────────────────────────────────────────

/// State for a sub-agent task launched by [`AgentTool`].
pub struct LocalAgentTaskState {
    /// Shared base fields (id, status, timestamps, …).
    pub base: TaskStateBase,
    /// The prompt sent to the sub-agent.
    pub prompt: String,
    /// Unique identifier for this agent instance.
    pub agent_id: String,
    /// Human-readable progress message (updated during execution).
    pub progress: Option<String>,
    /// Whether this task is running in the background.
    pub backgrounded: bool,
    /// Final result text (set on completion).
    pub result: Option<String>,
    /// Error message (set on failure).
    pub error: Option<String>,
}

// ── TaskState enum ─────────────────────────────────────────────────

/// Union of all concrete task states.
///
/// Only `LocalBash` and `LocalAgent` are active in Phase 1.
/// Additional variants (RemoteAgent, Dream, …) will be added as
/// the corresponding systems are ported.
pub enum TaskState {
    LocalBash(LocalBashTaskState),
    LocalAgent(LocalAgentTaskState),
}

impl TaskState {
    /// Access the common [`TaskStateBase`] fields.
    pub fn base(&self) -> &TaskStateBase {
        match self {
            Self::LocalBash(s) => &s.base,
            Self::LocalAgent(s) => &s.base,
        }
    }

    /// Mutable access to the common [`TaskStateBase`] fields.
    pub fn base_mut(&mut self) -> &mut TaskStateBase {
        match self {
            Self::LocalBash(s) => &mut s.base,
            Self::LocalAgent(s) => &mut s.base,
        }
    }

    /// Shorthand for `base().id`.
    pub fn id(&self) -> &str {
        &self.base().id
    }

    /// Shorthand for `base().status`.
    pub fn status(&self) -> &TaskStatus {
        &self.base().status
    }

    /// Shorthand for `base().task_type`.
    pub fn task_type(&self) -> &TaskType {
        &self.base().task_type
    }

    /// Whether this task is currently backgrounded.
    pub fn is_backgrounded(&self) -> bool {
        match self {
            Self::LocalBash(s) => s.backgrounded,
            Self::LocalAgent(s) => s.backgrounded,
        }
    }

    /// Downcast to a shell task state (returns `None` for other types).
    pub fn as_shell(&self) -> Option<&LocalBashTaskState> {
        match self {
            Self::LocalBash(s) => Some(s),
            _ => None,
        }
    }

    /// Mutable downcast to a shell task state.
    pub fn as_shell_mut(&mut self) -> Option<&mut LocalBashTaskState> {
        match self {
            Self::LocalBash(s) => Some(s),
            _ => None,
        }
    }

    /// Downcast to an agent task state (returns `None` for other types).
    pub fn as_agent(&self) -> Option<&LocalAgentTaskState> {
        match self {
            Self::LocalAgent(s) => Some(s),
            _ => None,
        }
    }

    /// Mutable downcast to an agent task state.
    pub fn as_agent_mut(&mut self) -> Option<&mut LocalAgentTaskState> {
        match self {
            Self::LocalAgent(s) => Some(s),
            _ => None,
        }
    }
}

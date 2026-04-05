//! Unified task registry — tracks all background tasks (shell and agent).
//!
//! This module is the **single source of truth** for task state during a
//! session.  Tools and the engine register, query, and kill tasks through
//! [`TaskRegistry`], accessed via a [`SharedTaskRegistry`] handle.
//!
//! # Module layout
//!
//! | Module       | Responsibility                              |
//! |--------------|---------------------------------------------|
//! | `types`      | Concrete state types + `TaskState` enum     |
//! | `shell`      | Shell task lifecycle (spawn, collect, kill)  |
//! | `agent`      | Agent task lifecycle (register, complete)    |
//! | `stop`       | Generic stop dispatch across task types      |
//! | `pill_label` | Compact pill string for the TUI status bar   |

pub mod agent;
pub mod pill_label;
pub mod shell;
pub mod stop;
pub mod types;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::models::{TaskStatus, TaskType};
use types::TaskState;

/// Thread-safe shared reference to the task registry.
pub type SharedTaskRegistry = Arc<Mutex<TaskRegistry>>;

/// Create a new empty shared task registry.
pub fn new_shared_registry() -> SharedTaskRegistry {
    Arc::new(Mutex::new(TaskRegistry::new()))
}

/// Central registry that owns all task state for the session.
///
/// Each task is identified by a short prefixed ID (e.g. `b001` for
/// shell, `a001` for agent).  The registry provides CRUD operations
/// and summary queries used by tools and the TUI.
pub struct TaskRegistry {
    tasks: HashMap<String, TaskState>,
    /// Per-prefix counters for sequential ID generation.
    counters: HashMap<char, u32>,
}

/// Returns the single-letter prefix for a given task type.
///
/// - `b` for LocalBash
/// - `a` for LocalAgent
/// - `t` for all others (future stubs)
fn prefix_for(task_type: &TaskType) -> char {
    match task_type {
        TaskType::LocalBash => 'b',
        TaskType::LocalAgent => 'a',
        _ => 't',
    }
}

impl TaskRegistry {
    /// Create an empty task registry.
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            counters: HashMap::new(),
        }
    }

    /// Generate the next sequential ID for a task type.
    ///
    /// Returns IDs like `b001`, `b002`, `a001`, etc.
    pub fn generate_id(&mut self, task_type: &TaskType) -> String {
        let prefix = prefix_for(task_type);
        let counter = self.counters.entry(prefix).or_insert(0);
        *counter += 1;
        format!("{}{:03}", prefix, counter)
    }

    /// Insert a task into the registry. Returns the task ID.
    pub fn register(&mut self, state: TaskState) -> String {
        let id = state.id().to_string();
        self.tasks.insert(id.clone(), state);
        id
    }

    /// Look up a task by ID.
    pub fn get(&self, id: &str) -> Option<&TaskState> {
        self.tasks.get(id)
    }

    /// Mutable look up a task by ID.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut TaskState> {
        self.tasks.get_mut(id)
    }

    /// Remove a task from the registry.
    pub fn remove(&mut self, id: &str) -> Option<TaskState> {
        self.tasks.remove(id)
    }

    /// List all tasks sorted by ID.
    pub fn list(&self) -> Vec<&TaskState> {
        let mut tasks: Vec<_> = self.tasks.values().collect();
        tasks.sort_by_key(|t| t.id());
        tasks
    }

    /// List only tasks currently in [`Running`](TaskStatus::Running) status.
    pub fn running_tasks(&self) -> Vec<&TaskState> {
        self.tasks
            .values()
            .filter(|t| *t.status() == TaskStatus::Running)
            .collect()
    }

    /// Count of currently running tasks (for the status bar pill).
    pub fn running_count(&self) -> usize {
        self.tasks
            .values()
            .filter(|t| *t.status() == TaskStatus::Running)
            .count()
    }
}

impl Default for TaskRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{TaskStateBase, TaskStatus, TaskType};
    use types::{LocalAgentTaskState, LocalBashTaskState, TaskState};

    fn make_base(id: &str, task_type: TaskType) -> TaskStateBase {
        TaskStateBase {
            id: id.to_string(),
            task_type,
            status: TaskStatus::Running,
            description: "test".to_string(),
            tool_use_id: None,
            start_time: 0,
            end_time: None,
            total_paused_ms: None,
            output_file: String::new(),
            output_offset: 0,
            notified: false,
        }
    }

    #[test]
    fn test_generate_id_prefixes() {
        let mut reg = TaskRegistry::new();
        assert_eq!(reg.generate_id(&TaskType::LocalBash), "b001");
        assert_eq!(reg.generate_id(&TaskType::LocalBash), "b002");
        assert_eq!(reg.generate_id(&TaskType::LocalAgent), "a001");
        assert_eq!(reg.generate_id(&TaskType::LocalAgent), "a002");
        assert_eq!(reg.generate_id(&TaskType::Dream), "t001");
    }

    #[test]
    fn test_register_and_get() {
        let mut reg = TaskRegistry::new();
        let state = TaskState::LocalBash(LocalBashTaskState {
            base: make_base("b001", TaskType::LocalBash),
            command: "echo hi".to_string(),
            stdout: String::new(),
            stderr: String::new(),
            child: None,
            backgrounded: false,
            agent_id: None,
        });
        let id = reg.register(state);
        assert_eq!(id, "b001");
        assert!(reg.get("b001").is_some());
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn test_running_count() {
        let mut reg = TaskRegistry::new();
        // One running shell task
        reg.register(TaskState::LocalBash(LocalBashTaskState {
            base: make_base("b001", TaskType::LocalBash),
            command: "sleep 60".to_string(),
            stdout: String::new(),
            stderr: String::new(),
            child: None,
            backgrounded: false,
            agent_id: None,
        }));
        // One completed agent task
        let mut completed_base = make_base("a001", TaskType::LocalAgent);
        completed_base.status = TaskStatus::Completed;
        reg.register(TaskState::LocalAgent(LocalAgentTaskState {
            base: completed_base,
            prompt: "test".to_string(),
            agent_id: "agent-1".to_string(),
            progress: None,
            backgrounded: false,
            result: Some("done".to_string()),
            error: None,
        }));

        assert_eq!(reg.running_count(), 1);
        assert_eq!(reg.list().len(), 2);
    }
}

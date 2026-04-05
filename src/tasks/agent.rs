//! Agent task lifecycle — register, complete, fail, and kill sub-agent tasks.
//!
//! Agent tasks are registered when [`AgentTool`] fires and updated once the
//! sub-agent produces a result (or errors out).  Unlike shell tasks, agent
//! tasks have no child process — they are tracked purely for status reporting
//! and TUI integration.
//!
//! # Key functions
//!
//! | Function   | Purpose                                      |
//! |------------|----------------------------------------------|
//! | `register` | Create a Running agent task entry             |
//! | `complete` | Mark as Completed with a result string        |
//! | `fail`     | Mark as Failed with an error message          |
//! | `kill`     | Mark as Killed (e.g. user-initiated cancel)   |

use anyhow::{anyhow, Result};

use crate::models::{TaskStateBase, TaskStatus, TaskType};
use super::SharedTaskRegistry;
use super::types::{LocalAgentTaskState, TaskState};

/// Register a new agent task in the registry.
///
/// The task starts in [`TaskStatus::Running`] and should be transitioned
/// via [`complete`], [`fail`], or [`kill`] once the sub-agent finishes.
///
/// # Returns
///
/// The assigned task ID (e.g. `a001`).
pub fn register(
    registry: &SharedTaskRegistry,
    prompt: &str,
    agent_id: &str,
    description: &str,
) -> Result<String> {
    let mut reg = registry
        .lock()
        .map_err(|e| anyhow!("Registry lock error: {}", e))?;

    let task_id = reg.generate_id(&TaskType::LocalAgent);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let state = TaskState::LocalAgent(LocalAgentTaskState {
        base: TaskStateBase {
            id: task_id.clone(),
            task_type: TaskType::LocalAgent,
            status: TaskStatus::Running,
            description: description.to_string(),
            tool_use_id: None,
            start_time: now,
            end_time: None,
            total_paused_ms: None,
            output_file: String::new(),
            output_offset: 0,
            notified: false,
        },
        prompt: prompt.to_string(),
        agent_id: agent_id.to_string(),
        progress: None,
        backgrounded: false,
        result: None,
        error: None,
    });

    reg.register(state);
    Ok(task_id)
}

/// Mark an agent task as successfully completed.
///
/// Sets the status to [`TaskStatus::Completed`] and stores the result text.
pub fn complete(registry: &SharedTaskRegistry, task_id: &str, result: &str) {
    let mut reg = match registry.lock() {
        Ok(r) => r,
        Err(_) => return,
    };

    if let Some(task) = reg.get_mut(task_id) {
        if let Some(agent) = task.as_agent_mut() {
            if agent.base.status == TaskStatus::Running {
                agent.base.status = TaskStatus::Completed;
                agent.result = Some(result.to_string());

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                agent.base.end_time = Some(now);
            }
        }
    }
}

/// Mark an agent task as failed with an error message.
///
/// Sets the status to [`TaskStatus::Failed`] and stores the error text.
pub fn fail(registry: &SharedTaskRegistry, task_id: &str, error: &str) {
    let mut reg = match registry.lock() {
        Ok(r) => r,
        Err(_) => return,
    };

    if let Some(task) = reg.get_mut(task_id) {
        if let Some(agent) = task.as_agent_mut() {
            if agent.base.status == TaskStatus::Running {
                agent.base.status = TaskStatus::Failed;
                agent.error = Some(error.to_string());

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                agent.base.end_time = Some(now);
            }
        }
    }
}

/// Kill a running agent task (user-initiated cancellation).
///
/// Sets the status to [`TaskStatus::Killed`] and also kills any shell
/// tasks that were spawned by this agent via [`shell::kill_for_agent`].
///
/// # Returns
///
/// `true` if the task existed and was running.
pub fn kill(registry: &SharedTaskRegistry, task_id: &str) -> bool {
    // First, find the agent_id so we can kill child shell tasks.
    let agent_id = {
        let reg = match registry.lock() {
            Ok(r) => r,
            Err(_) => return false,
        };
        match reg.get(task_id) {
            Some(task) => task.as_agent().map(|a| a.agent_id.clone()),
            None => return false,
        }
    };

    // Kill child shell tasks spawned by this agent.
    if let Some(ref aid) = agent_id {
        super::shell::kill_for_agent(registry, aid);
    }

    // Now mark the agent task itself as killed.
    let mut reg = match registry.lock() {
        Ok(r) => r,
        Err(_) => return false,
    };

    let task = match reg.get_mut(task_id) {
        Some(t) => t,
        None => return false,
    };

    let agent = match task.as_agent_mut() {
        Some(a) => a,
        None => return false,
    };

    if agent.base.status != TaskStatus::Running {
        return false;
    }

    agent.base.status = TaskStatus::Killed;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    agent.base.end_time = Some(now);

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::new_shared_registry;

    #[test]
    fn test_register_and_complete() {
        let registry = new_shared_registry();
        let id = register(&registry, "test prompt", "agent-1", "test agent").unwrap();
        assert!(id.starts_with('a'));

        {
            let reg = registry.lock().unwrap();
            assert_eq!(*reg.get(&id).unwrap().status(), TaskStatus::Running);
        }

        complete(&registry, &id, "done!");

        let reg = registry.lock().unwrap();
        let task = reg.get(&id).unwrap();
        assert_eq!(*task.status(), TaskStatus::Completed);
        assert_eq!(task.as_agent().unwrap().result.as_deref(), Some("done!"));
    }

    #[test]
    fn test_register_and_fail() {
        let registry = new_shared_registry();
        let id = register(&registry, "test prompt", "agent-2", "failing agent").unwrap();

        fail(&registry, &id, "something went wrong");

        let reg = registry.lock().unwrap();
        let task = reg.get(&id).unwrap();
        assert_eq!(*task.status(), TaskStatus::Failed);
        assert_eq!(
            task.as_agent().unwrap().error.as_deref(),
            Some("something went wrong")
        );
    }

    #[test]
    fn test_kill_agent() {
        let registry = new_shared_registry();
        let id = register(&registry, "test prompt", "agent-3", "killable agent").unwrap();

        assert!(kill(&registry, &id));

        let reg = registry.lock().unwrap();
        assert_eq!(*reg.get(&id).unwrap().status(), TaskStatus::Killed);
    }

    #[test]
    fn test_kill_already_completed() {
        let registry = new_shared_registry();
        let id = register(&registry, "test prompt", "agent-4", "done agent").unwrap();
        complete(&registry, &id, "finished");

        // Should not be killable once completed
        assert!(!kill(&registry, &id));
    }
}

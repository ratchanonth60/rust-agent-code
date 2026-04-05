//! Generic stop dispatch — terminates any running task regardless of type.
//!
//! [`stop_task`] inspects the [`TaskType`] of the target and delegates to
//! the appropriate lifecycle module ([`shell::kill`] or [`agent::kill`]).

use anyhow::{anyhow, Result};

use crate::models::TaskType;
use super::SharedTaskRegistry;

/// Stop a running task by its ID.
///
/// Dispatches to the correct kill function based on the task's
/// [`TaskType`]:
///
/// - [`TaskType::LocalBash`] → [`shell::kill`](super::shell::kill)
/// - [`TaskType::LocalAgent`] → [`agent::kill`](super::agent::kill)
///
/// # Errors
///
/// Returns an error if:
/// - The task ID is not found in the registry
/// - The task is already in a terminal state
/// - The underlying kill operation failed
pub fn stop_task(registry: &SharedTaskRegistry, task_id: &str) -> Result<()> {
    // Look up the task to get its type and status.
    let (task_type, status) = {
        let reg = registry
            .lock()
            .map_err(|e| anyhow!("Registry lock error: {}", e))?;
        let task = reg
            .get(task_id)
            .ok_or_else(|| anyhow!("Task '{}' not found", task_id))?;
        (task.task_type().clone(), task.status().clone())
    };

    if status.is_terminal() {
        return Err(anyhow!(
            "Task '{}' is already in terminal state: {:?}",
            task_id,
            status
        ));
    }

    let killed = match task_type {
        TaskType::LocalBash => super::shell::kill(registry, task_id),
        TaskType::LocalAgent => super::agent::kill(registry, task_id),
        _ => {
            return Err(anyhow!(
                "Stop not implemented for task type {:?}",
                task_type
            ))
        }
    };

    if killed {
        Ok(())
    } else {
        Err(anyhow!(
            "Failed to stop task '{}' (may have already finished)",
            task_id
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TaskStatus;
    use crate::tasks::{agent, new_shared_registry, shell};

    #[test]
    fn test_stop_shell_task() {
        let registry = new_shared_registry();
        let id = shell::spawn(&registry, "sleep 60", "stoppable", None).unwrap();

        assert!(stop_task(&registry, &id).is_ok());

        let reg = registry.lock().unwrap();
        assert_eq!(*reg.get(&id).unwrap().status(), TaskStatus::Killed);
    }

    #[test]
    fn test_stop_agent_task() {
        let registry = new_shared_registry();
        let id = agent::register(&registry, "prompt", "agent-1", "stoppable agent").unwrap();

        assert!(stop_task(&registry, &id).is_ok());

        let reg = registry.lock().unwrap();
        assert_eq!(*reg.get(&id).unwrap().status(), TaskStatus::Killed);
    }

    #[test]
    fn test_stop_already_terminal() {
        let registry = new_shared_registry();
        let id = agent::register(&registry, "prompt", "agent-2", "done agent").unwrap();
        agent::complete(&registry, &id, "finished");

        let result = stop_task(&registry, &id);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("terminal state"),
            "Error should mention terminal state"
        );
    }

    #[test]
    fn test_stop_not_found() {
        let registry = new_shared_registry();
        let result = stop_task(&registry, "nonexistent");
        assert!(result.is_err());
    }
}

//! Shell task lifecycle — spawn, collect output, and kill background processes.
//!
//! All functions operate on [`SharedTaskRegistry`] and expect the caller to
//! generate the task ID up-front (via [`TaskRegistry::generate_id`]).
//!
//! # Key functions
//!
//! | Function           | Purpose                                         |
//! |--------------------|-------------------------------------------------|
//! | `spawn`            | Spawn `bash -c <cmd>`, register as Running      |
//! | `collect_output`   | Non-blocking `try_wait` + pipe read              |
//! | `kill`             | SIGKILL a running task, set status to Killed     |
//! | `kill_for_agent`   | Kill all shell tasks owned by a specific agent   |

use std::io::Read as IoRead;
use std::process::{Command, Stdio};

use anyhow::{anyhow, Result};

use crate::models::{TaskStateBase, TaskStatus, TaskType};
use super::SharedTaskRegistry;
use super::types::{LocalBashTaskState, TaskState};

/// Spawn a background shell task and register it in the registry.
///
/// The command is run via `bash -c` with stdout and stderr piped for
/// later collection by [`collect_output`].
///
/// # Returns
///
/// The task ID string (e.g. `b001`) on success.
pub fn spawn(
    registry: &SharedTaskRegistry,
    command: &str,
    description: &str,
    agent_id: Option<String>,
) -> Result<String> {
    let child = Command::new("bash")
        .arg("-c")
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn shell task: {}", e))?;

    let mut reg = registry
        .lock()
        .map_err(|e| anyhow!("Registry lock error: {}", e))?;

    let task_id = reg.generate_id(&TaskType::LocalBash);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let state = TaskState::LocalBash(LocalBashTaskState {
        base: TaskStateBase {
            id: task_id.clone(),
            task_type: TaskType::LocalBash,
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
        command: command.to_string(),
        stdout: String::new(),
        stderr: String::new(),
        child: Some(child),
        backgrounded: false,
        agent_id,
    });

    reg.register(state);
    Ok(task_id)
}

/// Non-blocking check on a shell task's child process.
///
/// If the process has exited, reads all remaining stdout/stderr from the
/// pipes and updates the task's status to `Completed` or `Failed`.
///
/// # Returns
///
/// `Some((status, stdout, stderr))` if the task exists, `None` otherwise.
pub fn collect_output(
    registry: &SharedTaskRegistry,
    task_id: &str,
) -> Option<(TaskStatus, String, String)> {
    let mut reg = registry.lock().ok()?;
    let task = reg.get_mut(task_id)?;

    let shell = task.as_shell_mut()?;

    if shell.base.status == TaskStatus::Running {
        if let Some(ref mut child) = shell.child {
            match child.try_wait() {
                Ok(Some(exit_status)) => {
                    // Process exited — drain pipes
                    let mut child = shell.child.take().unwrap();
                    if let Some(ref mut stdout) = child.stdout {
                        let mut buf = String::new();
                        let _ = stdout.read_to_string(&mut buf);
                        shell.stdout = buf;
                    }
                    if let Some(ref mut stderr) = child.stderr {
                        let mut buf = String::new();
                        let _ = stderr.read_to_string(&mut buf);
                        shell.stderr = buf;
                    }
                    let _ = child.wait(); // reap zombie

                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);

                    shell.base.status = if exit_status.success() {
                        TaskStatus::Completed
                    } else {
                        TaskStatus::Failed
                    };
                    shell.base.end_time = Some(now);
                }
                Ok(None) => {
                    // Still running — nothing to collect yet
                }
                Err(_) => {
                    shell.base.status = TaskStatus::Failed;
                }
            }
        }
    }

    Some((
        shell.base.status.clone(),
        shell.stdout.clone(),
        shell.stderr.clone(),
    ))
}

/// Kill a running shell task by sending SIGKILL to its child process.
///
/// Sets the task status to [`TaskStatus::Killed`] and reaps the child.
///
/// # Returns
///
/// `true` if the task was found, was running, and was successfully killed.
pub fn kill(registry: &SharedTaskRegistry, task_id: &str) -> bool {
    let mut reg = match registry.lock() {
        Ok(r) => r,
        Err(_) => return false,
    };

    let task = match reg.get_mut(task_id) {
        Some(t) => t,
        None => return false,
    };

    let shell = match task.as_shell_mut() {
        Some(s) => s,
        None => return false,
    };

    if shell.base.status != TaskStatus::Running {
        return false;
    }

    if let Some(ref mut child) = shell.child {
        let _ = child.kill();
        let _ = child.wait();
    }
    shell.child = None;
    shell.base.status = TaskStatus::Killed;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    shell.base.end_time = Some(now);

    true
}

/// Kill all running shell tasks that were spawned by a specific agent.
///
/// Used during agent task cleanup to ensure no orphan processes remain
/// after an agent terminates or is killed.
pub fn kill_for_agent(registry: &SharedTaskRegistry, agent_id: &str) {
    let mut reg = match registry.lock() {
        Ok(r) => r,
        Err(_) => return,
    };

    // Collect IDs first to avoid holding mutable borrow during iteration.
    let target_ids: Vec<String> = reg
        .list()
        .iter()
        .filter_map(|task| {
            if let Some(shell) = task.as_shell() {
                if shell.base.status == TaskStatus::Running
                    && shell.agent_id.as_deref() == Some(agent_id)
                {
                    return Some(shell.base.id.clone());
                }
            }
            None
        })
        .collect();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    for id in target_ids {
        if let Some(task) = reg.get_mut(&id) {
            if let Some(shell) = task.as_shell_mut() {
                if let Some(ref mut child) = shell.child {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                shell.child = None;
                shell.base.status = TaskStatus::Killed;
                shell.base.end_time = Some(now);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::new_shared_registry;

    #[test]
    fn test_spawn_and_collect() {
        let registry = new_shared_registry();
        let id = spawn(&registry, "echo hello", "test echo", None).unwrap();
        assert!(id.starts_with('b'));

        // Wait a bit for the process to finish
        std::thread::sleep(std::time::Duration::from_millis(200));

        let result = collect_output(&registry, &id);
        assert!(result.is_some());
        let (status, stdout, _) = result.unwrap();
        assert_eq!(status, TaskStatus::Completed);
        assert!(stdout.contains("hello"));
    }

    #[test]
    fn test_kill_running_task() {
        let registry = new_shared_registry();
        let id = spawn(&registry, "sleep 60", "long sleep", None).unwrap();
        assert!(kill(&registry, &id));

        let reg = registry.lock().unwrap();
        let task = reg.get(&id).unwrap();
        assert_eq!(*task.status(), TaskStatus::Killed);
    }

    #[test]
    fn test_kill_for_agent() {
        let registry = new_shared_registry();
        let id1 = spawn(&registry, "sleep 60", "task 1", Some("agent-x".into())).unwrap();
        let id2 = spawn(&registry, "sleep 60", "task 2", Some("agent-x".into())).unwrap();
        let _id3 = spawn(&registry, "sleep 60", "task 3", Some("agent-y".into())).unwrap();

        kill_for_agent(&registry, "agent-x");

        let reg = registry.lock().unwrap();
        assert_eq!(*reg.get(&id1).unwrap().status(), TaskStatus::Killed);
        assert_eq!(*reg.get(&id2).unwrap().status(), TaskStatus::Killed);
        assert_eq!(*reg.get(&_id3).unwrap().status(), TaskStatus::Running);

        // Clean up: kill the remaining task
        drop(reg);
        kill(&registry, &_id3);
    }
}

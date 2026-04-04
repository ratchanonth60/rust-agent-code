//! Task manager — tracks background shell processes.
//!
//! Each task is a child process spawned via `std::process::Command`.
//! Tasks are identified by short hex IDs and can be queried or killed.

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};

/// Status of a background task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Running,
    Completed,
    Failed,
    Stopped,
}

/// A tracked background task.
struct Task {
    /// Short hex identifier.
    id: String,
    /// Shell command that was run.
    command: String,
    /// Optional human description.
    description: String,
    /// The child process (None once collected).
    child: Option<Child>,
    /// Captured stdout after completion.
    stdout: String,
    /// Captured stderr after completion.
    stderr: String,
    /// Current status.
    status: TaskStatus,
}

/// Manages background tasks for the session.
#[derive(Default)]
pub struct TaskManager {
    tasks: HashMap<String, Task>,
    next_id: u32,
}

impl TaskManager {
    /// Create an empty task manager.
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            next_id: 1,
        }
    }

    /// Spawn a new background task. Returns the task ID.
    pub fn spawn(&mut self, command: &str, description: &str) -> anyhow::Result<String> {
        let id = format!("{:04x}", self.next_id);
        self.next_id += 1;

        let child = Command::new("bash")
            .arg("-c")
            .arg(command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        self.tasks.insert(
            id.clone(),
            Task {
                id: id.clone(),
                command: command.to_string(),
                description: description.to_string(),
                child: Some(child),
                stdout: String::new(),
                stderr: String::new(),
                status: TaskStatus::Running,
            },
        );

        Ok(id)
    }

    /// Get the output of a task. Collects output if the process has finished.
    pub fn get_output(&self, task_id: &str) -> Option<(TaskStatus, String, String)> {
        // We can't mutate through &self, so we return what we have.
        // The caller should use `collect_output` first for fresh data.
        let task = self.tasks.get(task_id)?;
        Some((task.status, task.stdout.clone(), task.stderr.clone()))
    }

    /// Try to collect output from a finished process.
    pub fn collect_output(&mut self, task_id: &str) -> Option<(TaskStatus, String, String)> {
        let task = self.tasks.get_mut(task_id)?;

        if task.status == TaskStatus::Running {
            if let Some(ref mut child) = task.child {
                // Non-blocking check
                match child.try_wait() {
                    Ok(Some(exit_status)) => {
                        // Process exited — collect output
                        let mut child = task.child.take().unwrap();
                        if let Some(ref mut stdout) = child.stdout {
                            use std::io::Read;
                            let mut buf = String::new();
                            let _ = stdout.read_to_string(&mut buf);
                            task.stdout = buf;
                        }
                        if let Some(ref mut stderr) = child.stderr {
                            use std::io::Read;
                            let mut buf = String::new();
                            let _ = stderr.read_to_string(&mut buf);
                            task.stderr = buf;
                        }
                        let _ = child.wait();
                        task.status = if exit_status.success() {
                            TaskStatus::Completed
                        } else {
                            TaskStatus::Failed
                        };
                    }
                    Ok(None) => {
                        // Still running
                    }
                    Err(_) => {
                        task.status = TaskStatus::Failed;
                    }
                }
            }
        }

        Some((task.status, task.stdout.clone(), task.stderr.clone()))
    }

    /// Stop a running task. Returns `true` if the task was found and killed.
    pub fn stop(&mut self, task_id: &str) -> bool {
        if let Some(task) = self.tasks.get_mut(task_id) {
            if task.status == TaskStatus::Running {
                if let Some(ref mut child) = task.child {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                task.child = None;
                task.status = TaskStatus::Stopped;
                return true;
            }
        }
        false
    }

    /// List all tracked tasks with their status.
    pub fn list(&mut self) -> Vec<(String, String, String, TaskStatus)> {
        // First, collect output from all tasks
        let ids: Vec<String> = self.tasks.keys().cloned().collect();
        for id in &ids {
            self.collect_output(id);
        }

        let mut result: Vec<_> = self
            .tasks
            .values()
            .map(|t| {
                (
                    t.id.clone(),
                    t.command.clone(),
                    t.description.clone(),
                    t.status,
                )
            })
            .collect();
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_and_collect() {
        let mut mgr = TaskManager::new();
        let id = mgr.spawn("echo hello", "test echo").unwrap();
        assert_eq!(id, "0001");

        // Wait a bit for the process to finish
        std::thread::sleep(std::time::Duration::from_millis(100));

        let result = mgr.collect_output(&id);
        assert!(result.is_some());
        let (status, stdout, _) = result.unwrap();
        assert_eq!(status, TaskStatus::Completed);
        assert!(stdout.contains("hello"));
    }

    #[test]
    fn stop_running_task() {
        let mut mgr = TaskManager::new();
        let id = mgr.spawn("sleep 60", "long sleep").unwrap();
        assert!(mgr.stop(&id));

        let result = mgr.get_output(&id);
        assert!(result.is_some());
        assert_eq!(result.unwrap().0, TaskStatus::Stopped);
    }

    #[test]
    fn task_not_found() {
        let mgr = TaskManager::new();
        assert!(mgr.get_output("9999").is_none());
    }
}

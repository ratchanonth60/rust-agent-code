//! Worktree tools — create and manage isolated git worktrees.
//!
//! [`EnterWorktreeTool`] creates a temporary git worktree so the agent
//! works on an isolated copy of the repository.  [`ExitWorktreeTool`]
//! cleans up and returns to the original working directory.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::tools::{Tool, ToolContext, ToolResult};

// ── Shared state ────────────────────────────────────────────────────────

/// Tracks the active worktree so `ExitWorktreeTool` can clean up.
#[derive(Debug, Clone, Default)]
pub struct WorktreeState {
    /// Original working directory before entering the worktree.
    pub original_cwd: Option<PathBuf>,
    /// Path to the created worktree.
    pub worktree_path: Option<PathBuf>,
    /// Branch name created for the worktree.
    pub worktree_branch: Option<String>,
}

/// Thread-safe shared worktree state.
pub type SharedWorktreeState = Arc<Mutex<WorktreeState>>;

/// Create a new shared worktree state.
pub fn new_shared_worktree_state() -> SharedWorktreeState {
    Arc::new(Mutex::new(WorktreeState::default()))
}

// ── EnterWorktreeTool ───────────────────────────────────────────────────

/// Create an isolated git worktree for safe experimentation.
pub struct EnterWorktreeTool {
    pub state: SharedWorktreeState,
}

#[async_trait]
impl Tool for EnterWorktreeTool {
    fn name(&self) -> &str {
        "EnterWorktree"
    }

    fn description(&self) -> &str {
        "Create an isolated git worktree for safe experimentation. \
         The worktree gets its own branch based on HEAD."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Optional name for the worktree. Random name if omitted."
                }
            }
        })
    }

    fn is_destructive(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        // Check if already in a worktree
        {
            let state = self.state.lock()
                .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
            if state.worktree_path.is_some() {
                return Ok(ToolResult::err(json!({
                    "error": "Already in a worktree. Use ExitWorktree first."
                })));
            }
        }

        // Check if we're in a git repo
        let in_git = std::process::Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .current_dir(&ctx.cwd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if !in_git {
            return Ok(ToolResult::err(json!({
                "error": "Not inside a git repository"
            })));
        }

        let name = input["name"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("wt-{:08x}", rand_id()));

        let worktree_dir = ctx.cwd.join(".claude").join("worktrees").join(&name);
        let branch_name = format!("worktree/{}", name);

        // Create parent directory
        std::fs::create_dir_all(worktree_dir.parent().unwrap_or(&ctx.cwd))?;

        // Create the worktree with a new branch
        let output = std::process::Command::new("git")
            .args([
                "worktree", "add",
                "-b", &branch_name,
                &worktree_dir.to_string_lossy(),
                "HEAD",
            ])
            .current_dir(&ctx.cwd)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(ToolResult::err(json!({
                "error": format!("Failed to create worktree: {}", stderr)
            })));
        }

        // Store state
        {
            let mut state = self.state.lock()
                .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
            state.original_cwd = Some(ctx.cwd.clone());
            state.worktree_path = Some(worktree_dir.clone());
            state.worktree_branch = Some(branch_name.clone());
        }

        Ok(ToolResult::ok(json!({
            "status": "worktree_created",
            "worktree_path": worktree_dir.to_string_lossy(),
            "branch": branch_name,
            "message": format!(
                "Worktree created at {} on branch {}. \
                 Use ExitWorktree when done.",
                worktree_dir.display(), branch_name
            )
        })))
    }
}

// ── ExitWorktreeTool ────────────────────────────────────────────────────

/// Clean up and exit the current git worktree.
pub struct ExitWorktreeTool {
    pub state: SharedWorktreeState,
}

#[async_trait]
impl Tool for ExitWorktreeTool {
    fn name(&self) -> &str {
        "ExitWorktree"
    }

    fn description(&self) -> &str {
        "Exit the current git worktree and return to the original directory. \
         Removes the worktree if no changes were made."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "keep": {
                    "type": "boolean",
                    "description": "Keep the worktree even if no changes. Default: false."
                }
            }
        })
    }

    fn is_destructive(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let keep = input["keep"].as_bool().unwrap_or(false);

        let (original_cwd, worktree_path, branch) = {
            let state = self.state.lock()
                .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
            match (&state.original_cwd, &state.worktree_path, &state.worktree_branch) {
                (Some(cwd), Some(wt), Some(br)) => (cwd.clone(), wt.clone(), br.clone()),
                _ => {
                    return Ok(ToolResult::err(json!({
                        "error": "Not currently in a worktree"
                    })));
                }
            }
        };

        // Check if there are uncommitted changes in the worktree
        let has_changes = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&worktree_path)
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);

        let mut cleaned_up = false;

        if !keep && !has_changes {
            // Remove worktree and branch
            let _ = std::process::Command::new("git")
                .args(["worktree", "remove", &worktree_path.to_string_lossy()])
                .current_dir(&original_cwd)
                .output();

            let _ = std::process::Command::new("git")
                .args(["branch", "-D", &branch])
                .current_dir(&original_cwd)
                .output();

            cleaned_up = true;
        }

        // Clear state
        {
            let mut state = self.state.lock()
                .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
            *state = WorktreeState::default();
        }

        Ok(ToolResult::ok(json!({
            "status": "worktree_exited",
            "original_cwd": original_cwd.to_string_lossy(),
            "cleaned_up": cleaned_up,
            "had_changes": has_changes,
            "message": if cleaned_up {
                "Worktree removed (no changes). Returned to original directory.".to_string()
            } else if has_changes {
                format!(
                    "Worktree kept at {} (has uncommitted changes). Branch: {}",
                    worktree_path.display(), branch
                )
            } else {
                format!("Worktree kept at {}. Branch: {}", worktree_path.display(), branch)
            }
        })))
    }
}

/// Generate a simple pseudo-random hex ID.
fn rand_id() -> u32 {
    use std::time::SystemTime;
    let d = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    (d.as_nanos() & 0xFFFF_FFFF) as u32
}

use std::path::Path;
use std::process::Command;

/// Get git context: branch, short status, recent commits.
pub fn get_git_context(cwd: &Path) -> String {
    let mut parts = Vec::new();

    // Current branch
    if let Some(branch) = run_git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        parts.push(format!("Branch: {}", branch));
    } else {
        return String::new(); // Not a git repo
    }

    // Short status
    if let Some(status) = run_git(cwd, &["status", "--short", "--branch"]) {
        if !status.trim().is_empty() {
            let lines: Vec<&str> = status.lines().take(15).collect();
            parts.push(format!("Status:\n{}", lines.join("\n")));
        }
    }

    // Recent commits (last 5)
    if let Some(log) = run_git(cwd, &["log", "--oneline", "-5"]) {
        if !log.trim().is_empty() {
            parts.push(format!("Recent commits:\n{}", log.trim()));
        }
    }

    parts.join("\n")
}

fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
            } else {
                None
            }
        })
}

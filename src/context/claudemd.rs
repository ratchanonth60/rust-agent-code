use std::path::Path;

/// Load CLAUDE.md files from standard locations, returning concatenated content.
///
/// Searches (in order):
/// 1. `~/.claude/CLAUDE.md` (global)
/// 2. `<cwd>/CLAUDE.md` (project root)
/// 3. `<cwd>/.claude/CLAUDE.md` (project .claude dir)
pub fn load_claudemd_files(cwd: &Path) -> String {
    let mut parts = Vec::new();

    // Global CLAUDE.md
    if let Some(home) = dirs::home_dir() {
        let global = home.join(".claude").join("CLAUDE.md");
        if let Ok(content) = std::fs::read_to_string(&global) {
            if !content.trim().is_empty() {
                parts.push(format!("## Global (~/.claude/CLAUDE.md)\n\n{}", content.trim()));
            }
        }
    }

    // Project root CLAUDE.md
    let project_root = cwd.join("CLAUDE.md");
    if let Ok(content) = std::fs::read_to_string(&project_root) {
        if !content.trim().is_empty() {
            parts.push(format!("## Project (CLAUDE.md)\n\n{}", content.trim()));
        }
    }

    // Project .claude/CLAUDE.md
    let project_claude_dir = cwd.join(".claude").join("CLAUDE.md");
    if let Ok(content) = std::fs::read_to_string(&project_claude_dir) {
        if !content.trim().is_empty() {
            parts.push(format!("## Project (.claude/CLAUDE.md)\n\n{}", content.trim()));
        }
    }

    parts.join("\n\n")
}

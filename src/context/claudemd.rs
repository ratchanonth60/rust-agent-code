//! Loads `CLAUDE.md` instruction files from global and project locations.

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
        try_load_section(&home.join(".claude").join("CLAUDE.md"), "Global (~/.claude/CLAUDE.md)", &mut parts);
    }

    try_load_section(&cwd.join("CLAUDE.md"),                   "Project (CLAUDE.md)",        &mut parts);
    try_load_section(&cwd.join(".claude").join("CLAUDE.md"),   "Project (.claude/CLAUDE.md)", &mut parts);

    parts.join("\n\n")
}

/// Reads `path`; if non-empty, pushes a `## <label>\n\n<content>` section.
fn try_load_section(path: &Path, label: &str, parts: &mut Vec<String>) {
    let Ok(content) = std::fs::read_to_string(path) else { return };
    let content = content.trim();
    if !content.is_empty() {
        parts.push(format!("## {label}\n\n{content}"));
    }
}

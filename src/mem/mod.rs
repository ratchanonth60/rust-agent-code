//! Agent memory system — file-based persistence and RAG prompts.
//!
//! The LLM is instructed to maintain a `MEMORY.md` index file and
//! individual topic files under `~/.rust-agent/memory/`.  This module
//! builds the system-prompt section that teaches the LLM how to use
//! this memory layer.

use std::path::PathBuf;

/// Returns the path to the agent's local memory directory (`~/.rust-agent/memory/`).
pub fn get_auto_mem_path() -> PathBuf {
    // Determine the base user directory (e.g. ~/.rust-agent)
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home_dir.join(".rust-agent").join("memory")
}

/// Builds the system-prompt section that teaches the LLM how to
/// manage its persistent memory files.
///
/// If `MEMORY.md` exists it is appended verbatim so the LLM has
/// access to its own prior notes.
pub fn build_memory_prompt() -> String {
    let memory_dir = get_auto_mem_path();
    let dir_str = memory_dir.to_string_lossy();

    let mut prompt = format!(
        "You are a helpful AI Agent operating in the local system.
You can execute bash commands and read/write files.

# Memory
You have a persistent, file-based memory system at `{}`.
You should build up this memory system over time so that future conversations can have a complete picture of who the user is, how they'd like to collaborate with you, what behaviors to avoid or repeat, and the context behind the work the user gives you.

## How to save memories
Saving a memory is a two-step process:

**Step 1** — write the memory to its own file using standard markdown mapping to (user/feedback/project/reference) taxonomy.
**Step 2** — add a pointer to that file in `MEMORY.md`. Never write memory content directly into `MEMORY.md`. Keep index entries to one line under 150 characters: `- [Title](file.md) — hook`.

Use the ReadFile and WriteFile tools to access and evolve this structure.",
        dir_str
    );
    
    // Inject existing memory content directly if present
    let memory_file = memory_dir.join("MEMORY.md");
    if memory_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&memory_file) {
            prompt.push_str("\n\n# Your Current Memories\n");
            prompt.push_str(&content);
        }
    }
    
    prompt
}

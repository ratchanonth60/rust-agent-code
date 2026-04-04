//! Skill system — user-defined prompt templates loaded from disk.
//!
//! Skills are `.md` files stored in `~/.rust-agent/skills/` or
//! `<project>/.rust-agent/skills/`. Each file becomes a skill invocable
//! via `/skill <name>` or as an LLM tool.
//!
//! # Skill file format
//!
//! ```markdown
//! ---
//! name: commit
//! description: Create a git commit
//! ---
//!
//! Review the changes and create a commit...
//! ```

pub mod loader;
pub mod types;

pub use loader::load_skills;
pub use types::Skill;

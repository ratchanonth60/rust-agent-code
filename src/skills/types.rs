//! Skill type definitions.

/// A loaded skill template.
#[derive(Debug, Clone)]
pub struct Skill {
    /// Skill name (derived from filename or frontmatter).
    pub name: String,
    /// Short description shown in help.
    pub description: String,
    /// Full prompt template body (markdown).
    pub prompt: String,
    /// Source file path.
    pub source_path: std::path::PathBuf,
    /// Optional list of allowed tools during skill execution.
    pub allowed_tools: Option<Vec<String>>,
}

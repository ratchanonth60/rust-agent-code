//! Skill loader — discovers and parses skill files from standard directories.

use std::path::{Path, PathBuf};

use super::types::Skill;

/// Load skills from all standard skill directories.
///
/// Searches:
/// 1. `~/.rust-agent/skills/`
/// 2. `<cwd>/.rust-agent/skills/`
pub fn load_skills(cwd: &Path) -> Vec<Skill> {
    let mut skills = Vec::new();
    let mut paths = Vec::new();

    // Global skills
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".rust-agent").join("skills"));
    }

    // Project-local skills
    paths.push(cwd.join(".rust-agent").join("skills"));

    for dir in &paths {
        if dir.exists() {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|e| e == "md").unwrap_or(false) {
                        if let Some(skill) = parse_skill_file(&path) {
                            // Deduplicate by name (project-local overrides global)
                            skills.retain(|s: &Skill| s.name != skill.name);
                            skills.push(skill);
                        }
                    }
                }
            }
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Parse a single skill markdown file.
///
/// Supports optional YAML frontmatter delimited by `---` lines.
fn parse_skill_file(path: &PathBuf) -> Option<Skill> {
    let content = std::fs::read_to_string(path).ok()?;
    let filename = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    let mut name = filename.to_string();
    let mut description = String::new();
    let mut allowed_tools: Option<Vec<String>> = None;
    let mut prompt = content.clone();

    // Try to parse frontmatter
    if let Some(after_prefix) = content.strip_prefix("---") {
        if let Some(end_idx) = after_prefix.find("---") {
            let frontmatter = &after_prefix[..end_idx];
            prompt = after_prefix[end_idx + 3..].trim().to_string();

            for line in frontmatter.lines() {
                let line = line.trim();
                if let Some(val) = line.strip_prefix("name:") {
                    name = val.trim().to_string();
                } else if let Some(val) = line.strip_prefix("description:") {
                    description = val.trim().to_string();
                } else if let Some(val) = line.strip_prefix("allowed_tools:") {
                    allowed_tools = Some(
                        val.split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect(),
                    );
                }
            }
        }
    }

    if description.is_empty() {
        // Use first non-empty line as description
        description = prompt
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("No description")
            .chars()
            .take(80)
            .collect();
    }

    Some(Skill {
        name,
        description,
        prompt,
        source_path: path.clone(),
        allowed_tools,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skill_with_frontmatter() {
        let dir = std::env::temp_dir().join("rust-agent-test-skills");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test-skill.md");
        std::fs::write(
            &path,
            "---\nname: test\ndescription: A test skill\n---\n\nDo something useful.",
        )
        .unwrap();

        let skill = parse_skill_file(&path).unwrap();
        assert_eq!(skill.name, "test");
        assert_eq!(skill.description, "A test skill");
        assert!(skill.prompt.contains("Do something useful"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_skill_without_frontmatter() {
        let dir = std::env::temp_dir().join("rust-agent-test-skills2");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("simple.md");
        std::fs::write(&path, "Just a plain prompt.").unwrap();

        let skill = parse_skill_file(&path).unwrap();
        assert_eq!(skill.name, "simple");
        assert!(skill.description.contains("Just a plain"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}

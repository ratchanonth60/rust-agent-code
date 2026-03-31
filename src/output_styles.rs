use glob::glob;
use regex::Regex;
use std::fs;
use std::path::PathBuf;
use tracing::warn;

pub struct OutputStyle {
    pub name: String,
    pub prompt: String,
}

/// Reads the output styles applied locally (`./.rust-agent/output-styles`) and globally (`~/.rust-agent/output-styles`).
pub fn load_output_styles() -> Vec<OutputStyle> {
    let mut styles = Vec::new();
    
    // We will collect md files from global and local directories
    let mut paths_to_search = Vec::new();
    
    // 1. Global styles
    if let Some(home_dir) = dirs::home_dir() {
        paths_to_search.push(home_dir.join(".rust-agent").join("output-styles"));
    }
    
    // 2. Local project styles
    if let Ok(cwd) = std::env::current_dir() {
        paths_to_search.push(cwd.join(".rust-agent").join("output-styles"));
    }

    let frontmatter_regex = Regex::new(r"(?s)^---\n.*?\n---\n").unwrap();

    for base_dir in paths_to_search {
        let pattern = base_dir.join("*.md");
        let pattern_str = pattern.to_string_lossy();
        
        let entries = match glob(&pattern_str) {
            Ok(iter) => iter,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            if let Ok(content) = fs::read_to_string(&entry) {
                // Determine style name from filename
                let file_name = entry.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
                
                // Strip YAML frontmatter for the prompt body
                let prompt_body = frontmatter_regex.replace(&content, "").to_string();
                
                if !prompt_body.trim().is_empty() {
                    styles.push(OutputStyle {
                        name: file_name.to_string(),
                        prompt: prompt_body,
                    });
                }
            } else {
                warn!("Could not read output style file: {:?}", entry);
            }
        }
    }
    
    styles
}

/// Constructs a combined system prompt string summarizing all loaded Markdown output styles.
pub fn build_styles_prompt() -> String {
    let styles = load_output_styles();
    if styles.is_empty() {
        return String::new();
    }
    
    let mut prompt = String::from("\n# Output Styles\nPlease adhere to the following output styles when generating your response:\n\n");
    for style in styles {
        prompt.push_str(&format!("## Style: {}\n{}\n\n", style.name, style.prompt.trim()));
    }
    
    prompt
}

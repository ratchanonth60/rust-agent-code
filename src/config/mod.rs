//! Global configuration — persistent user preferences.
//!
//! Stored at `~/.rust-agent/config.json` and loaded at startup.
//! Slash commands like `/vim`, `/theme`, `/model` modify and persist this config.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ── Config path ──────────────────────────────────────────────────────

/// Returns the path to the global config file.
pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rust-agent")
        .join("config.json")
}

// ── Editor mode ──────────────────────────────────────────────────────

/// Editor input mode for the TUI prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EditorMode {
    Normal,
    Vim,
}

impl Default for EditorMode {
    fn default() -> Self {
        Self::Normal
    }
}

impl std::fmt::Display for EditorMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "normal"),
            Self::Vim => write!(f, "vim"),
        }
    }
}

// ── Global config ────────────────────────────────────────────────────

/// Persistent global configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Input mode for the TUI prompt.
    #[serde(default)]
    pub editor_mode: EditorMode,

    /// Active color theme name.
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Default model name override.
    #[serde(default)]
    pub default_model: Option<String>,

    /// Default provider name override.
    #[serde(default)]
    pub default_provider: Option<String>,

    /// Output style name (if using custom output styles).
    #[serde(default)]
    pub output_style: Option<String>,
}

fn default_theme() -> String {
    "default".to_string()
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            editor_mode: EditorMode::Normal,
            theme: default_theme(),
            default_model: None,
            default_provider: None,
            output_style: None,
        }
    }
}

impl GlobalConfig {
    /// Load the global config from disk, or return defaults if the file doesn't exist.
    pub fn load() -> Self {
        let path = config_path();
        if let Ok(json) = fs::read_to_string(&path) {
            serde_json::from_str(&json).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Persist the current config to disk.
    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config dir: {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self)
            .with_context(|| "Failed to serialize config")?;
        fs::write(&path, json)
            .with_context(|| format!("Failed to write config: {}", path.display()))?;
        Ok(())
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_sane() {
        let cfg = GlobalConfig::default();
        assert_eq!(cfg.editor_mode, EditorMode::Normal);
        assert_eq!(cfg.theme, "default");
        assert!(cfg.default_model.is_none());
    }

    #[test]
    fn roundtrip_serialize() {
        let cfg = GlobalConfig {
            editor_mode: EditorMode::Vim,
            theme: "monokai".to_string(),
            default_model: Some("claude-sonnet-4-6".to_string()),
            default_provider: None,
            output_style: None,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let loaded: GlobalConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.editor_mode, EditorMode::Vim);
        assert_eq!(loaded.theme, "monokai");
        assert_eq!(loaded.default_model, Some("claude-sonnet-4-6".to_string()));
    }
}

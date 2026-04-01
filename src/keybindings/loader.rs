use std::path::PathBuf;

use super::default_bindings::default_bindings;
use super::parser::parse_bindings;
use super::types::{KeybindingBlock, ParsedBinding};

// ---------------------------------------------------------------------------
// Config path
// ---------------------------------------------------------------------------

/// Returns the path to the user keybindings file.
///
/// Looks for `keybindings.json` in `~/.rust-agent/`.
pub fn get_keybindings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rust-agent")
        .join("keybindings.json")
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Result of loading keybindings, including any validation warnings.
#[derive(Debug, Clone)]
pub struct KeybindingsLoadResult {
    /// Merged binding list (defaults ++ user overrides).
    pub bindings: Vec<ParsedBinding>,
    /// Warnings from validation (reserved keys, parse errors, etc.).
    pub warnings: Vec<KeybindingWarning>,
}

/// A warning produced during keybinding validation.
#[derive(Debug, Clone)]
pub struct KeybindingWarning {
    pub severity: WarningSeverity,
    pub message: String,
    pub suggestion: Option<String>,
}

/// Severity of a keybinding warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningSeverity {
    Error,
    Warning,
}

/// Loads and merges default + user keybindings.
///
/// Reads `~/.rust-agent/keybindings.json` if it exists, parses the
/// binding blocks, and appends them after the defaults (last-match-wins).
/// Returns the merged bindings and any validation warnings.
///
/// If the file doesn't exist or can't be parsed, returns defaults with
/// appropriate warnings.
///
/// # JSON format
///
/// ```json
/// {
///   "bindings": [
///     { "context": "Chat", "bindings": { "ctrl+k": "chat:cancel" } }
///   ]
/// }
/// ```
pub fn load_keybindings() -> KeybindingsLoadResult {
    let default_parsed = parse_bindings(&default_bindings());
    let user_path = get_keybindings_path();

    let content = match std::fs::read_to_string(&user_path) {
        Ok(c) => c,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                // No user config — use defaults
                return KeybindingsLoadResult {
                    bindings: default_parsed,
                    warnings: vec![],
                };
            }
            return KeybindingsLoadResult {
                bindings: default_parsed,
                warnings: vec![KeybindingWarning {
                    severity: WarningSeverity::Error,
                    message: format!("Failed to read {}: {}", user_path.display(), e),
                    suggestion: None,
                }],
            };
        }
    };

    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return KeybindingsLoadResult {
                bindings: default_parsed,
                warnings: vec![KeybindingWarning {
                    severity: WarningSeverity::Error,
                    message: format!("Invalid JSON in keybindings.json: {}", e),
                    suggestion: Some("Check JSON syntax".to_string()),
                }],
            };
        }
    };

    // Expect { "bindings": [ ... ] }
    let bindings_value = match parsed.get("bindings") {
        Some(v) if v.is_array() => v,
        _ => {
            return KeybindingsLoadResult {
                bindings: default_parsed,
                warnings: vec![KeybindingWarning {
                    severity: WarningSeverity::Error,
                    message: "keybindings.json must have a \"bindings\" array".to_string(),
                    suggestion: Some("Use format: { \"bindings\": [ ... ] }".to_string()),
                }],
            };
        }
    };

    let user_blocks: Vec<KeybindingBlock> = match serde_json::from_value(bindings_value.clone()) {
        Ok(blocks) => blocks,
        Err(e) => {
            return KeybindingsLoadResult {
                bindings: default_parsed,
                warnings: vec![KeybindingWarning {
                    severity: WarningSeverity::Error,
                    message: format!("Invalid keybinding blocks: {}", e),
                    suggestion: Some(
                        "Each block must have \"context\" (string) and \"bindings\" (object)"
                            .to_string(),
                    ),
                }],
            };
        }
    };

    let user_parsed = parse_bindings(&user_blocks);
    let mut warnings = Vec::new();

    // Validate user bindings against reserved shortcuts
    for block in &user_blocks {
        for key in block.bindings.keys() {
            if let Some(reserved) = super::reserved::is_reserved(key) {
                warnings.push(KeybindingWarning {
                    severity: match reserved.severity {
                        super::reserved::ReservedSeverity::Error => WarningSeverity::Error,
                        super::reserved::ReservedSeverity::Warning => WarningSeverity::Warning,
                    },
                    message: format!("Key \"{}\" is reserved: {}", key, reserved.reason),
                    suggestion: Some("Choose a different key combination".to_string()),
                });
            }
        }
    }

    // Merge: defaults first, then user (last-match-wins)
    let mut merged = default_parsed;
    merged.extend(user_parsed);

    KeybindingsLoadResult {
        bindings: merged,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_returns_defaults_when_no_file() {
        let result = load_keybindings();
        assert!(!result.bindings.is_empty());
        // Should have bindings from all default contexts
    }

    #[test]
    fn keybindings_path_ends_with_expected() {
        let path = get_keybindings_path();
        assert!(path.ends_with("keybindings.json"));
    }
}

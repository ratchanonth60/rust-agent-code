use super::parser::normalize_key_for_comparison;

// ---------------------------------------------------------------------------
// Reserved shortcut types
// ---------------------------------------------------------------------------

/// Severity level for a reserved shortcut warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReservedSeverity {
    /// The key absolutely cannot be rebound (hardcoded app behaviour).
    Error,
    /// The key is likely intercepted by the OS or terminal.
    Warning,
}

/// A shortcut that is reserved and should not be rebound by users.
#[derive(Debug, Clone)]
pub struct ReservedShortcut {
    /// Normalized key string (e.g. `"ctrl+c"`).
    pub key: String,
    /// Human-readable reason why this key is reserved.
    pub reason: &'static str,
    pub severity: ReservedSeverity,
}

// ---------------------------------------------------------------------------
// Non-rebindable shortcuts
// ---------------------------------------------------------------------------

/// Shortcuts that cannot be rebound — they are hardcoded in the rust-agent.
///
/// These are checked during validation when a user attempts to override
/// them in `keybindings.json`.
pub fn non_rebindable() -> Vec<ReservedShortcut> {
    vec![
        ReservedShortcut {
            key: "ctrl+c".to_string(),
            reason: "Cannot be rebound — used for interrupt/exit (hardcoded)",
            severity: ReservedSeverity::Error,
        },
        ReservedShortcut {
            key: "ctrl+d".to_string(),
            reason: "Cannot be rebound — used for exit (hardcoded)",
            severity: ReservedSeverity::Error,
        },
        ReservedShortcut {
            key: "ctrl+m".to_string(),
            reason: "Cannot be rebound — identical to Enter in terminals (both send CR)",
            severity: ReservedSeverity::Error,
        },
    ]
}

// ---------------------------------------------------------------------------
// Terminal-reserved shortcuts
// ---------------------------------------------------------------------------

/// Terminal control shortcuts that are typically intercepted by the OS or
/// shell before reaching the application.
pub fn terminal_reserved() -> Vec<ReservedShortcut> {
    vec![
        ReservedShortcut {
            key: "ctrl+z".to_string(),
            reason: "Unix process suspend (SIGTSTP)",
            severity: ReservedSeverity::Warning,
        },
        ReservedShortcut {
            key: "ctrl+\\".to_string(),
            reason: "Terminal quit signal (SIGQUIT)",
            severity: ReservedSeverity::Error,
        },
    ]
}

// ---------------------------------------------------------------------------
// Platform-specific reserved shortcuts
// ---------------------------------------------------------------------------

/// macOS-specific shortcuts intercepted by the operating system.
#[cfg(target_os = "macos")]
fn macos_reserved() -> Vec<ReservedShortcut> {
    vec![
        ReservedShortcut { key: "cmd+c".into(), reason: "macOS system copy", severity: ReservedSeverity::Error },
        ReservedShortcut { key: "cmd+v".into(), reason: "macOS system paste", severity: ReservedSeverity::Error },
        ReservedShortcut { key: "cmd+x".into(), reason: "macOS system cut", severity: ReservedSeverity::Error },
        ReservedShortcut { key: "cmd+q".into(), reason: "macOS quit application", severity: ReservedSeverity::Error },
        ReservedShortcut { key: "cmd+w".into(), reason: "macOS close window/tab", severity: ReservedSeverity::Error },
        ReservedShortcut { key: "cmd+tab".into(), reason: "macOS app switcher", severity: ReservedSeverity::Error },
        ReservedShortcut { key: "cmd+space".into(), reason: "macOS Spotlight", severity: ReservedSeverity::Error },
    ]
}

#[cfg(not(target_os = "macos"))]
fn macos_reserved() -> Vec<ReservedShortcut> {
    vec![]
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Returns all reserved shortcuts for the current platform.
///
/// Includes non-rebindable (highest priority), terminal-reserved, and
/// platform-specific shortcuts. Used during validation to warn users
/// about bindings that won't work.
pub fn get_reserved_shortcuts() -> Vec<ReservedShortcut> {
    let mut reserved = non_rebindable();
    reserved.extend(terminal_reserved());
    reserved.extend(macos_reserved());
    reserved
}

/// Checks whether a key string matches any reserved shortcut.
///
/// Returns the matching [`ReservedShortcut`] if found, `None` otherwise.
/// Both the input and the reserved keys are normalized before comparison.
pub fn is_reserved(key: &str) -> Option<ReservedShortcut> {
    let normalized = normalize_key_for_comparison(key);
    get_reserved_shortcuts()
        .into_iter()
        .find(|r| normalize_key_for_comparison(&r.key) == normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_c_is_reserved() {
        let result = is_reserved("ctrl+c");
        assert!(result.is_some());
        assert_eq!(result.unwrap().severity, ReservedSeverity::Error);
    }

    #[test]
    fn ctrl_d_is_reserved() {
        assert!(is_reserved("ctrl+d").is_some());
    }

    #[test]
    fn ctrl_k_is_not_reserved() {
        assert!(is_reserved("ctrl+k").is_none());
    }

    #[test]
    fn ctrl_m_is_reserved() {
        assert!(is_reserved("ctrl+m").is_some());
    }
}

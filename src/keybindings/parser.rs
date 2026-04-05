use super::types::{
    Chord, KeybindingAction, KeybindingBlock, KeybindingContext, ParsedBinding, ParsedKeystroke,
};

// ---------------------------------------------------------------------------
// Keystroke parsing
// ---------------------------------------------------------------------------

/// Parses a single keystroke string like `"ctrl+shift+k"` into a
/// [`ParsedKeystroke`].
///
/// Supports various modifier aliases:
/// - **ctrl**: `ctrl`, `control`
/// - **alt**: `alt`, `opt`, `option`
/// - **meta**: `meta`, `cmd`, `command`, `super`, `win`
/// - **shift**: `shift`
///
/// Special key aliases are normalized: `esc` → `escape`, `return` → `enter`,
/// `space` → `" "`, arrow unicode symbols → `up`/`down`/`left`/`right`.
///
/// # Examples
///
/// ```
/// let ks = parse_keystroke("ctrl+shift+k");
/// assert!(ks.ctrl);
/// assert!(ks.shift);
/// assert_eq!(ks.key, "k");
/// ```
pub fn parse_keystroke(input: &str) -> ParsedKeystroke {
    let mut ks = ParsedKeystroke {
        key: String::new(),
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };

    for part in input.split('+') {
        let lower = part.to_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => ks.ctrl = true,
            "alt" | "opt" | "option" => ks.alt = true,
            "shift" => ks.shift = true,
            "meta" => ks.meta = true,
            // cmd/command/super/win — mapped to `meta` for our Rust agent
            // (we don't have a separate `super` field like the TS version)
            "cmd" | "command" | "super" | "win" => ks.meta = true,
            // Key aliases
            "esc" => ks.key = "escape".to_string(),
            "return" => ks.key = "enter".to_string(),
            "space" => ks.key = " ".to_string(),
            "\u{2191}" => ks.key = "up".to_string(),    // ↑
            "\u{2193}" => ks.key = "down".to_string(),  // ↓
            "\u{2190}" => ks.key = "left".to_string(),  // ←
            "\u{2192}" => ks.key = "right".to_string(), // →
            _ => ks.key = lower,
        }
    }

    ks
}

/// Parses a chord string like `"ctrl+k ctrl+s"` into a [`Chord`]
/// (a `Vec<ParsedKeystroke>`).
///
/// A lone `" "` is treated as the space key binding, not a separator.
///
/// # Examples
///
/// ```
/// let chord = parse_chord("ctrl+x ctrl+k");
/// assert_eq!(chord.len(), 2);
/// assert!(chord[0].ctrl);
/// assert_eq!(chord[0].key, "x");
/// ```
pub fn parse_chord(input: &str) -> Chord {
    // A lone space IS the space key binding
    if input == " " {
        return vec![parse_keystroke("space")];
    }
    input
        .split_whitespace()
        .map(parse_keystroke)
        .collect()
}

// ---------------------------------------------------------------------------
// Display formatting
// ---------------------------------------------------------------------------

/// Maps internal key names to human-readable display names.
///
/// For example, `"escape"` → `"Esc"`, `"up"` → `"↑"`.
fn key_to_display_name(key: &str) -> &str {
    match key {
        "escape" => "Esc",
        " " => "Space",
        "tab" => "Tab",
        "enter" => "Enter",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "up" => "↑",
        "down" => "↓",
        "left" => "←",
        "right" => "→",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "home" => "Home",
        "end" => "End",
        _ => key,
    }
}

/// Formats a [`ParsedKeystroke`] as a canonical display string
/// like `"ctrl+shift+k"`.
pub fn keystroke_to_string(ks: &ParsedKeystroke) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if ks.ctrl {
        parts.push("ctrl");
    }
    if ks.alt {
        parts.push("alt");
    }
    if ks.shift {
        parts.push("shift");
    }
    if ks.meta {
        parts.push("meta");
    }
    parts.push(key_to_display_name(&ks.key));
    parts.join("+")
}

/// Display platform for platform-aware shortcut formatting.
///
/// WSL and unknown are treated as Linux for display purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayPlatform {
    MacOS,
    Windows,
    Linux,
}

/// Formats a [`ParsedKeystroke`] as a platform-appropriate display string.
///
/// Uses `"opt"` for alt on macOS, `"alt"` elsewhere.
/// Uses `"cmd"` for meta on macOS, `"super"` elsewhere.
pub fn keystroke_to_display_string(ks: &ParsedKeystroke, platform: DisplayPlatform) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if ks.ctrl {
        parts.push("ctrl");
    }
    // Alt and meta both map to the same terminal modifier
    if ks.alt || ks.meta {
        parts.push(if platform == DisplayPlatform::MacOS {
            "opt"
        } else {
            "alt"
        });
    }
    if ks.shift {
        parts.push("shift");
    }
    parts.push(key_to_display_name(&ks.key));
    parts.join("+")
}

/// Formats a [`Chord`] as a platform-appropriate display string.
///
/// For example, `"ctrl+x ctrl+k"`.
pub fn chord_to_display_string(chord: &Chord, platform: DisplayPlatform) -> String {
    chord
        .iter()
        .map(|ks| keystroke_to_display_string(ks, platform))
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Binding block parsing
// ---------------------------------------------------------------------------

/// Parses raw [`KeybindingBlock`]s (from JSON config) into a flat list of
/// [`ParsedBinding`]s.
///
/// Each `(key, action)` pair in a block is converted to a [`ParsedBinding`]
/// with the block's context. Actions of `None` represent explicit unbindings.
///
/// # Examples
///
/// ```
/// let blocks = vec![KeybindingBlock {
///     context: "Chat".to_string(),
///     bindings: HashMap::from([("ctrl+k".to_string(), Some("chat:cancel".to_string()))]),
/// }];
/// let parsed = parse_bindings(&blocks);
/// assert_eq!(parsed.len(), 1);
/// ```
pub fn parse_bindings(blocks: &[KeybindingBlock]) -> Vec<ParsedBinding> {
    let mut bindings = Vec::new();
    for block in blocks {
        let context = match serde_json::from_value::<KeybindingContext>(
            serde_json::Value::String(block.context.clone()),
        ) {
            Ok(ctx) => ctx,
            Err(_) => continue, // Skip unknown contexts
        };
        for (key, action_str) in &block.bindings {
            let chord = parse_chord(key);
            let action = action_str
                .as_ref()
                .and_then(|s| KeybindingAction::parse_action(s));
            bindings.push(ParsedBinding {
                chord,
                action,
                context,
            });
        }
    }
    bindings
}

// ---------------------------------------------------------------------------
// Normalization (for comparison / validation)
// ---------------------------------------------------------------------------

/// Normalizes a key string for comparison.
///
/// Lowercases everything, sorts modifiers, and normalizes modifier
/// aliases (`control` → `ctrl`, `option`/`opt` → `alt`, `command`/`cmd` → `cmd`).
/// Chord steps (space-separated) are normalized independently.
///
/// # Examples
///
/// ```
/// assert_eq!(normalize_key_for_comparison("Control+K"), "ctrl+k");
/// assert_eq!(normalize_key_for_comparison("ctrl+x ctrl+b"), "ctrl+x ctrl+b");
/// ```
pub fn normalize_key_for_comparison(key: &str) -> String {
    key.split_whitespace()
        .map(normalize_step)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Normalizes a single keystroke step (e.g. `"Control+Shift+K"` → `"ctrl+shift+k"`).
fn normalize_step(step: &str) -> String {
    let parts: Vec<&str> = step.split('+').collect();
    let mut modifiers: Vec<String> = Vec::new();
    let mut main_key = String::new();

    for part in parts {
        let lower = part.trim().to_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => modifiers.push("ctrl".to_string()),
            "alt" | "opt" | "option" | "meta" => modifiers.push("alt".to_string()),
            "cmd" | "command" => modifiers.push("cmd".to_string()),
            "shift" => modifiers.push("shift".to_string()),
            _ => main_key = lower,
        }
    }

    modifiers.sort();
    modifiers.push(main_key);
    modifiers.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_key() {
        let ks = parse_keystroke("enter");
        assert_eq!(ks.key, "enter");
        assert!(!ks.ctrl);
        assert!(!ks.alt);
        assert!(!ks.shift);
        assert!(!ks.meta);
    }

    #[test]
    fn parse_ctrl_modifier() {
        let ks = parse_keystroke("ctrl+c");
        assert_eq!(ks.key, "c");
        assert!(ks.ctrl);
    }

    #[test]
    fn parse_multiple_modifiers() {
        let ks = parse_keystroke("ctrl+shift+k");
        assert_eq!(ks.key, "k");
        assert!(ks.ctrl);
        assert!(ks.shift);
        assert!(!ks.alt);
    }

    #[test]
    fn parse_esc_alias() {
        let ks = parse_keystroke("esc");
        assert_eq!(ks.key, "escape");
    }

    #[test]
    fn parse_chord_single() {
        let chord = parse_chord("ctrl+c");
        assert_eq!(chord.len(), 1);
        assert!(chord[0].ctrl);
        assert_eq!(chord[0].key, "c");
    }

    #[test]
    fn parse_chord_multi() {
        let chord = parse_chord("ctrl+x ctrl+k");
        assert_eq!(chord.len(), 2);
        assert!(chord[0].ctrl);
        assert_eq!(chord[0].key, "x");
        assert!(chord[1].ctrl);
        assert_eq!(chord[1].key, "k");
    }

    #[test]
    fn parse_chord_space() {
        let chord = parse_chord(" ");
        assert_eq!(chord.len(), 1);
        assert_eq!(chord[0].key, " ");
    }

    #[test]
    fn display_string_linux() {
        let ks = parse_keystroke("ctrl+shift+k");
        assert_eq!(
            keystroke_to_display_string(&ks, DisplayPlatform::Linux),
            "ctrl+shift+k"
        );
    }

    #[test]
    fn normalize_control_alias() {
        assert_eq!(normalize_key_for_comparison("Control+K"), "ctrl+k");
    }

    #[test]
    fn normalize_chord() {
        assert_eq!(
            normalize_key_for_comparison("ctrl+x ctrl+b"),
            "ctrl+x ctrl+b"
        );
    }
}

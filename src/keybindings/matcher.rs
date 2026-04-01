use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::types::ParsedKeystroke;

// ---------------------------------------------------------------------------
// KeyEvent → key name
// ---------------------------------------------------------------------------

/// Extracts the normalized key name from a crossterm [`KeyEvent`].
///
/// Maps crossterm's [`KeyCode`] variants to the string names used by
/// [`ParsedKeystroke::key`]. Returns `None` for unrecognised key codes.
///
/// # Examples
///
/// ```
/// use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
/// let event = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
/// assert_eq!(get_key_name(&event), Some("escape".to_string()));
/// ```
pub fn get_key_name(event: &KeyEvent) -> Option<String> {
    match event.code {
        KeyCode::Esc => Some("escape".to_string()),
        KeyCode::Enter => Some("enter".to_string()),
        KeyCode::Tab => Some("tab".to_string()),
        KeyCode::BackTab => Some("tab".to_string()), // shift+tab
        KeyCode::Backspace => Some("backspace".to_string()),
        KeyCode::Delete => Some("delete".to_string()),
        KeyCode::Up => Some("up".to_string()),
        KeyCode::Down => Some("down".to_string()),
        KeyCode::Left => Some("left".to_string()),
        KeyCode::Right => Some("right".to_string()),
        KeyCode::PageUp => Some("pageup".to_string()),
        KeyCode::PageDown => Some("pagedown".to_string()),
        KeyCode::Home => Some("home".to_string()),
        KeyCode::End => Some("end".to_string()),
        KeyCode::Char(c) => Some(c.to_lowercase().to_string()),
        KeyCode::F(n) => Some(format!("f{}", n)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Modifier matching
// ---------------------------------------------------------------------------

/// Checks whether the modifiers in a [`KeyEvent`] match the modifiers
/// specified in a [`ParsedKeystroke`].
///
/// Handles the terminal quirk where alt and meta are indistinguishable —
/// either `alt` or `meta` in the parsed binding will match the `ALT` modifier
/// from crossterm.
///
/// # BackTab quirk
///
/// When crossterm reports [`KeyCode::BackTab`], the shift modifier is implicit
/// (it's already `shift+tab`), so we skip the shift check for that key code.
fn modifiers_match(event: &KeyEvent, target: &ParsedKeystroke) -> bool {
    let mods = event.modifiers;
    let has_ctrl = mods.contains(KeyModifiers::CONTROL);
    let has_alt = mods.contains(KeyModifiers::ALT);
    let has_shift = mods.contains(KeyModifiers::SHIFT);

    // Ctrl
    if has_ctrl != target.ctrl {
        return false;
    }

    // Shift — BackTab already implies shift, so skip the check
    if event.code != KeyCode::BackTab && has_shift != target.shift {
        return false;
    }

    // Alt and meta are equivalent in terminals
    let target_needs_alt = target.alt || target.meta;
    if has_alt != target_needs_alt {
        return false;
    }

    true
}

// ---------------------------------------------------------------------------
// Keystroke matching
// ---------------------------------------------------------------------------

/// Checks whether a crossterm [`KeyEvent`] matches a [`ParsedKeystroke`].
///
/// Compares both the key name and all modifier flags, accounting for
/// terminal limitations (alt ≡ meta).
///
/// # Examples
///
/// ```
/// use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
/// let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
/// let target = parse_keystroke("ctrl+c");
/// assert!(matches_keystroke(&event, &target));
/// ```
pub fn matches_keystroke(event: &KeyEvent, target: &ParsedKeystroke) -> bool {
    let key_name = match get_key_name(event) {
        Some(name) => name,
        None => return false,
    };

    if key_name != target.key {
        return false;
    }

    modifiers_match(event, target)
}

/// Compares two [`ParsedKeystroke`]s for logical equality.
///
/// Collapses `alt`/`meta` into one logical modifier — legacy terminals
/// can't distinguish them, so `"alt+k"` and `"meta+k"` are treated as the
/// same keystroke.
pub fn keystrokes_equal(a: &ParsedKeystroke, b: &ParsedKeystroke) -> bool {
    a.key == b.key
        && a.ctrl == b.ctrl
        && a.shift == b.shift
        && (a.alt || a.meta) == (b.alt || b.meta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keybindings::parser::parse_keystroke;

    #[test]
    fn match_ctrl_c() {
        let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let target = parse_keystroke("ctrl+c");
        assert!(matches_keystroke(&event, &target));
    }

    #[test]
    fn no_match_wrong_modifier() {
        let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT);
        let target = parse_keystroke("ctrl+c");
        assert!(!matches_keystroke(&event, &target));
    }

    #[test]
    fn match_escape() {
        let event = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let target = parse_keystroke("escape");
        assert!(matches_keystroke(&event, &target));
    }

    #[test]
    fn match_enter() {
        let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let target = parse_keystroke("enter");
        assert!(matches_keystroke(&event, &target));
    }

    #[test]
    fn match_backtab_as_shift_tab() {
        let event = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
        let target = parse_keystroke("shift+tab");
        assert!(matches_keystroke(&event, &target));
    }

    #[test]
    fn keystrokes_equal_alt_meta() {
        let a = parse_keystroke("alt+k");
        let b = parse_keystroke("meta+k");
        assert!(keystrokes_equal(&a, &b));
    }

    #[test]
    fn keystrokes_not_equal_different_key() {
        let a = parse_keystroke("ctrl+k");
        let b = parse_keystroke("ctrl+j");
        assert!(!keystrokes_equal(&a, &b));
    }
}

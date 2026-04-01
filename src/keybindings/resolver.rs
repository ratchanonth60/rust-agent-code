use crossterm::event::KeyEvent;
use std::collections::HashSet;

use super::matcher::{get_key_name, keystrokes_equal, matches_keystroke};
use super::types::{
    ChordResolveResult, Chord, KeybindingAction, KeybindingContext, ParsedBinding,
    ParsedKeystroke,
};

// ---------------------------------------------------------------------------
// Simple (single-key) resolution
// ---------------------------------------------------------------------------

/// Resolves a [`KeyEvent`] to a [`KeybindingAction`] using single-keystroke
/// bindings only (no chord support).
///
/// Scans all `bindings` whose context is in `active_contexts` and returns
/// the last match (last-match-wins, so user overrides beat defaults).
///
/// Returns `None` when no binding matches. An explicit unbinding
/// (`action: None` in the parsed binding) also returns `None`.
///
/// # Arguments
///
/// * `event` — the crossterm [`KeyEvent`] to resolve
/// * `active_contexts` — contexts currently active in the UI
/// * `bindings` — the full binding list (defaults ++ user overrides)
pub fn resolve_key(
    event: &KeyEvent,
    active_contexts: &[KeybindingContext],
    bindings: &[ParsedBinding],
) -> Option<KeybindingAction> {
    let ctx_set: HashSet<KeybindingContext> =
        active_contexts.iter().copied().collect();

    let mut last_match: Option<&ParsedBinding> = None;

    for binding in bindings {
        // Single-key only
        if binding.chord.len() != 1 {
            continue;
        }
        if !ctx_set.contains(&binding.context) {
            continue;
        }
        if matches_keystroke(event, &binding.chord[0]) {
            last_match = Some(binding);
        }
    }

    last_match.and_then(|b| b.action.clone())
}

// ---------------------------------------------------------------------------
// Chord-aware resolution
// ---------------------------------------------------------------------------

/// Resolves a [`KeyEvent`] with chord-state awareness.
///
/// Handles multi-keystroke chord bindings like `ctrl+x ctrl+k`. The caller
/// must maintain the `pending` chord prefix across calls.
///
/// # Resolution order
///
/// 1. **Escape cancels** — if escape is pressed while a chord is pending,
///    returns [`ChordResolveResult::ChordCancelled`].
/// 2. **Prefix check** — if the current chord sequence is a prefix of any
///    longer binding (that isn't null-unbound), returns
///    [`ChordResolveResult::ChordStarted`] so the caller can wait for more keys.
/// 3. **Exact match** — last-match-wins among bindings matching the full chord.
/// 4. **No match** — returns `None` or `ChordCancelled` if a pending chord
///    was interrupted.
///
/// # Arguments
///
/// * `event` — the crossterm [`KeyEvent`]
/// * `active_contexts` — currently active contexts
/// * `bindings` — all parsed bindings
/// * `pending` — the pending chord prefix, or `None` if not in a chord
pub fn resolve_key_with_chord_state(
    event: &KeyEvent,
    active_contexts: &[KeybindingContext],
    bindings: &[ParsedBinding],
    pending: Option<&[ParsedKeystroke]>,
) -> ChordResolveResult {
    // Cancel chord on escape
    if event.code == crossterm::event::KeyCode::Esc && pending.is_some() {
        return ChordResolveResult::ChordCancelled;
    }

    // Build current keystroke
    let current_keystroke = match build_keystroke(event) {
        Some(ks) => ks,
        None => {
            return if pending.is_some() {
                ChordResolveResult::ChordCancelled
            } else {
                ChordResolveResult::None
            };
        }
    };

    // Build the full chord sequence to test
    let test_chord: Chord = match pending {
        Some(prefix) => {
            let mut c = prefix.to_vec();
            c.push(current_keystroke);
            c
        }
        None => vec![current_keystroke],
    };

    // Filter bindings by active contexts
    let ctx_set: HashSet<KeybindingContext> =
        active_contexts.iter().copied().collect();
    let context_bindings: Vec<&ParsedBinding> = bindings
        .iter()
        .filter(|b| ctx_set.contains(&b.context))
        .collect();

    // Check for longer chord prefixes. Use a map so null-overrides
    // shadow the default they unbind (prevents entering chord-wait
    // for a binding that's been explicitly removed).
    let mut chord_winners: std::collections::HashMap<String, bool> =
        std::collections::HashMap::new();
    for binding in &context_bindings {
        if binding.chord.len() > test_chord.len()
            && chord_prefix_matches(&test_chord, binding)
        {
            let key = super::types::chord_to_string(&binding.chord);
            chord_winners.insert(key, binding.action.is_some());
        }
    }
    let has_longer = chord_winners.values().any(|&has_action| has_action);

    if has_longer {
        return ChordResolveResult::ChordStarted(test_chord);
    }

    // Check for exact match (last one wins)
    let mut exact_match: Option<&ParsedBinding> = None;
    for binding in &context_bindings {
        if chord_exactly_matches(&test_chord, binding) {
            exact_match = Some(binding);
        }
    }

    if let Some(m) = exact_match {
        return match &m.action {
            None => ChordResolveResult::Unbound,
            Some(action) => ChordResolveResult::Match(action.clone()),
        };
    }

    // No match
    if pending.is_some() {
        ChordResolveResult::ChordCancelled
    } else {
        ChordResolveResult::None
    }
}

// ---------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------

/// Gets the display text for an action in a given context.
///
/// Searches in reverse order so user overrides (appended after defaults)
/// take precedence. Returns the chord string (e.g. `"ctrl+t"`) or `None`
/// if the action is not bound.
pub fn get_binding_display_text(
    action: &KeybindingAction,
    context: KeybindingContext,
    bindings: &[ParsedBinding],
) -> Option<String> {
    bindings
        .iter()
        .rev()
        .find(|b| {
            b.context == context
                && b.action.as_ref() == Some(action)
        })
        .map(|b| super::types::chord_to_string(&b.chord))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Builds a [`ParsedKeystroke`] from a crossterm [`KeyEvent`].
///
/// Returns `None` for unrecognised key codes.
fn build_keystroke(event: &KeyEvent) -> Option<ParsedKeystroke> {
    let key_name = get_key_name(event)?;
    let mods = event.modifiers;

    // Escape can falsely set ALT in some terminals — ignore it
    let effective_alt = if event.code == crossterm::event::KeyCode::Esc {
        false
    } else {
        mods.contains(crossterm::event::KeyModifiers::ALT)
    };

    Some(ParsedKeystroke {
        key: key_name,
        ctrl: mods.contains(crossterm::event::KeyModifiers::CONTROL),
        alt: effective_alt,
        shift: mods.contains(crossterm::event::KeyModifiers::SHIFT),
        meta: false, // crossterm doesn't distinguish meta from alt
    })
}

/// Checks whether `prefix` is a proper prefix of `binding`'s chord.
fn chord_prefix_matches(prefix: &[ParsedKeystroke], binding: &ParsedBinding) -> bool {
    if prefix.len() >= binding.chord.len() {
        return false;
    }
    for (i, pk) in prefix.iter().enumerate() {
        if !keystrokes_equal(pk, &binding.chord[i]) {
            return false;
        }
    }
    true
}

/// Checks whether `chord` exactly matches `binding`'s chord.
fn chord_exactly_matches(chord: &[ParsedKeystroke], binding: &ParsedBinding) -> bool {
    if chord.len() != binding.chord.len() {
        return false;
    }
    for (i, ck) in chord.iter().enumerate() {
        if !keystrokes_equal(ck, &binding.chord[i]) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keybindings::parser::parse_bindings;
    use crate::keybindings::types::{KeybindingBlock, KeybindingContext};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::collections::HashMap;

    fn make_test_bindings() -> Vec<ParsedBinding> {
        let blocks = vec![
            KeybindingBlock {
                context: "Chat".to_string(),
                bindings: HashMap::from([
                    ("enter".into(), Some("chat:submit".into())),
                    ("escape".into(), Some("chat:cancel".into())),
                    ("ctrl+x ctrl+k".into(), Some("chat:killAgents".into())),
                ]),
            },
            KeybindingBlock {
                context: "Global".to_string(),
                bindings: HashMap::from([
                    ("ctrl+c".into(), Some("app:interrupt".into())),
                    ("ctrl+d".into(), Some("app:exit".into())),
                ]),
            },
        ];
        parse_bindings(&blocks)
    }

    #[test]
    fn resolve_ctrl_c() {
        let bindings = make_test_bindings();
        let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let result = resolve_key(
            &event,
            &[KeybindingContext::Global],
            &bindings,
        );
        assert_eq!(result, Some(KeybindingAction::AppInterrupt));
    }

    #[test]
    fn resolve_no_match() {
        let bindings = make_test_bindings();
        let event = KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE);
        let result = resolve_key(
            &event,
            &[KeybindingContext::Global],
            &bindings,
        );
        assert_eq!(result, None);
    }

    #[test]
    fn resolve_context_filtering() {
        let bindings = make_test_bindings();
        let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        // Enter is only bound in Chat, not Global
        let result = resolve_key(
            &event,
            &[KeybindingContext::Global],
            &bindings,
        );
        assert_eq!(result, None);
    }

    #[test]
    fn chord_started() {
        let bindings = make_test_bindings();
        let event = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
        let result = resolve_key_with_chord_state(
            &event,
            &[KeybindingContext::Chat],
            &bindings,
            None,
        );
        match result {
            ChordResolveResult::ChordStarted(pending) => {
                assert_eq!(pending.len(), 1);
                assert!(pending[0].ctrl);
                assert_eq!(pending[0].key, "x");
            }
            other => panic!("Expected ChordStarted, got {:?}", other),
        }
    }
}

use super::types::KeybindingBlock;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Default bindings
// ---------------------------------------------------------------------------

/// Returns the default keybinding blocks for the rust-agent TUI.
///
/// These match the Claude Code TypeScript defaults, adapted for the
/// rust-agent's available contexts and actions. They are loaded first;
/// user overrides from `keybindings.json` are appended after these,
/// with last-match-wins semantics.
///
/// # Platform notes
///
/// - **Image paste**: Windows uses `alt+v` (ctrl+v is system paste);
///   other platforms use `ctrl+v`.
/// - **Mode cycle**: Uses `shift+tab` on most platforms; falls back to
///   `meta+m` on Windows terminals without VT mode.
pub fn default_bindings() -> Vec<KeybindingBlock> {
    let is_windows = cfg!(target_os = "windows");
    let image_paste_key = if is_windows { "alt+v" } else { "ctrl+v" };
    let mode_cycle_key = if is_windows { "meta+m" } else { "shift+tab" };

    vec![
        // -- Global --
        KeybindingBlock {
            context: "Global".to_string(),
            bindings: HashMap::from([
                ("ctrl+c".into(), Some("app:interrupt".into())),
                ("ctrl+d".into(), Some("app:exit".into())),
                ("ctrl+l".into(), Some("app:redraw".into())),
                ("ctrl+t".into(), Some("app:toggleTodos".into())),
                ("ctrl+o".into(), Some("app:toggleTranscript".into())),
                ("ctrl+r".into(), Some("history:search".into())),
            ]),
        },
        // -- Chat --
        KeybindingBlock {
            context: "Chat".to_string(),
            bindings: HashMap::from([
                ("escape".into(), Some("chat:cancel".into())),
                ("ctrl+x ctrl+k".into(), Some("chat:killAgents".into())),
                (mode_cycle_key.into(), Some("chat:cycleMode".into())),
                ("meta+p".into(), Some("chat:modelPicker".into())),
                ("meta+o".into(), Some("chat:fastMode".into())),
                ("meta+t".into(), Some("chat:thinkingToggle".into())),
                ("enter".into(), Some("chat:submit".into())),
                ("up".into(), Some("history:previous".into())),
                ("down".into(), Some("history:next".into())),
                ("ctrl+_".into(), Some("chat:undo".into())),
                ("ctrl+shift+-".into(), Some("chat:undo".into())),
                ("ctrl+x ctrl+e".into(), Some("chat:externalEditor".into())),
                ("ctrl+g".into(), Some("chat:externalEditor".into())),
                ("ctrl+s".into(), Some("chat:stash".into())),
                (image_paste_key.into(), Some("chat:imagePaste".into())),
            ]),
        },
        // -- Autocomplete --
        KeybindingBlock {
            context: "Autocomplete".to_string(),
            bindings: HashMap::from([
                ("tab".into(), Some("autocomplete:accept".into())),
                ("escape".into(), Some("autocomplete:dismiss".into())),
                ("up".into(), Some("autocomplete:previous".into())),
                ("down".into(), Some("autocomplete:next".into())),
            ]),
        },
        // -- Settings --
        KeybindingBlock {
            context: "Settings".to_string(),
            bindings: HashMap::from([
                ("escape".into(), Some("confirm:no".into())),
                ("up".into(), Some("select:previous".into())),
                ("down".into(), Some("select:next".into())),
                ("k".into(), Some("select:previous".into())),
                ("j".into(), Some("select:next".into())),
                ("ctrl+p".into(), Some("select:previous".into())),
                ("ctrl+n".into(), Some("select:next".into())),
                ("space".into(), Some("select:accept".into())),
                ("enter".into(), Some("settings:close".into())),
                ("/".into(), Some("settings:search".into())),
                ("r".into(), Some("settings:retry".into())),
            ]),
        },
        // -- Confirmation --
        KeybindingBlock {
            context: "Confirmation".to_string(),
            bindings: HashMap::from([
                ("y".into(), Some("confirm:yes".into())),
                ("n".into(), Some("confirm:no".into())),
                ("enter".into(), Some("confirm:yes".into())),
                ("escape".into(), Some("confirm:no".into())),
                ("up".into(), Some("confirm:previous".into())),
                ("down".into(), Some("confirm:next".into())),
                ("tab".into(), Some("confirm:nextField".into())),
                ("space".into(), Some("confirm:toggle".into())),
                ("shift+tab".into(), Some("confirm:cycleMode".into())),
                ("ctrl+e".into(), Some("confirm:toggleExplanation".into())),
                ("ctrl+d".into(), Some("permission:toggleDebug".into())),
            ]),
        },
        // -- Tabs --
        KeybindingBlock {
            context: "Tabs".to_string(),
            bindings: HashMap::from([
                ("tab".into(), Some("tabs:next".into())),
                ("shift+tab".into(), Some("tabs:previous".into())),
                ("right".into(), Some("tabs:next".into())),
                ("left".into(), Some("tabs:previous".into())),
            ]),
        },
        // -- Transcript --
        KeybindingBlock {
            context: "Transcript".to_string(),
            bindings: HashMap::from([
                ("ctrl+e".into(), Some("transcript:toggleShowAll".into())),
                ("ctrl+c".into(), Some("transcript:exit".into())),
                ("escape".into(), Some("transcript:exit".into())),
                ("q".into(), Some("transcript:exit".into())),
            ]),
        },
        // -- History Search --
        KeybindingBlock {
            context: "HistorySearch".to_string(),
            bindings: HashMap::from([
                ("ctrl+r".into(), Some("historySearch:next".into())),
                ("escape".into(), Some("historySearch:accept".into())),
                ("tab".into(), Some("historySearch:accept".into())),
                ("ctrl+c".into(), Some("historySearch:cancel".into())),
                ("enter".into(), Some("historySearch:execute".into())),
            ]),
        },
        // -- Task --
        KeybindingBlock {
            context: "Task".to_string(),
            bindings: HashMap::from([
                ("ctrl+b".into(), Some("task:background".into())),
            ]),
        },
        // -- ThemePicker --
        KeybindingBlock {
            context: "ThemePicker".to_string(),
            bindings: HashMap::from([
                ("ctrl+t".into(), Some("theme:toggleSyntaxHighlighting".into())),
            ]),
        },
        // -- Help --
        KeybindingBlock {
            context: "Help".to_string(),
            bindings: HashMap::from([
                ("escape".into(), Some("help:dismiss".into())),
            ]),
        },
        // -- Attachments --
        KeybindingBlock {
            context: "Attachments".to_string(),
            bindings: HashMap::from([
                ("right".into(), Some("attachments:next".into())),
                ("left".into(), Some("attachments:previous".into())),
                ("backspace".into(), Some("attachments:remove".into())),
                ("delete".into(), Some("attachments:remove".into())),
                ("down".into(), Some("attachments:exit".into())),
                ("escape".into(), Some("attachments:exit".into())),
            ]),
        },
        // -- Footer --
        KeybindingBlock {
            context: "Footer".to_string(),
            bindings: HashMap::from([
                ("up".into(), Some("footer:up".into())),
                ("ctrl+p".into(), Some("footer:up".into())),
                ("down".into(), Some("footer:down".into())),
                ("ctrl+n".into(), Some("footer:down".into())),
                ("right".into(), Some("footer:next".into())),
                ("left".into(), Some("footer:previous".into())),
                ("enter".into(), Some("footer:openSelected".into())),
                ("escape".into(), Some("footer:clearSelection".into())),
            ]),
        },
        // -- MessageSelector --
        KeybindingBlock {
            context: "MessageSelector".to_string(),
            bindings: HashMap::from([
                ("up".into(), Some("messageSelector:up".into())),
                ("down".into(), Some("messageSelector:down".into())),
                ("k".into(), Some("messageSelector:up".into())),
                ("j".into(), Some("messageSelector:down".into())),
                ("ctrl+p".into(), Some("messageSelector:up".into())),
                ("ctrl+n".into(), Some("messageSelector:down".into())),
                ("ctrl+up".into(), Some("messageSelector:top".into())),
                ("shift+up".into(), Some("messageSelector:top".into())),
                ("meta+up".into(), Some("messageSelector:top".into())),
                ("shift+k".into(), Some("messageSelector:top".into())),
                ("ctrl+down".into(), Some("messageSelector:bottom".into())),
                ("shift+down".into(), Some("messageSelector:bottom".into())),
                ("meta+down".into(), Some("messageSelector:bottom".into())),
                ("shift+j".into(), Some("messageSelector:bottom".into())),
                ("enter".into(), Some("messageSelector:select".into())),
            ]),
        },
        // -- DiffDialog --
        KeybindingBlock {
            context: "DiffDialog".to_string(),
            bindings: HashMap::from([
                ("escape".into(), Some("diff:dismiss".into())),
                ("left".into(), Some("diff:previousSource".into())),
                ("right".into(), Some("diff:nextSource".into())),
                ("up".into(), Some("diff:previousFile".into())),
                ("down".into(), Some("diff:nextFile".into())),
                ("enter".into(), Some("diff:viewDetails".into())),
            ]),
        },
        // -- ModelPicker --
        KeybindingBlock {
            context: "ModelPicker".to_string(),
            bindings: HashMap::from([
                ("left".into(), Some("modelPicker:decreaseEffort".into())),
                ("right".into(), Some("modelPicker:increaseEffort".into())),
            ]),
        },
        // -- Select --
        KeybindingBlock {
            context: "Select".to_string(),
            bindings: HashMap::from([
                ("up".into(), Some("select:previous".into())),
                ("down".into(), Some("select:next".into())),
                ("j".into(), Some("select:next".into())),
                ("k".into(), Some("select:previous".into())),
                ("ctrl+n".into(), Some("select:next".into())),
                ("ctrl+p".into(), Some("select:previous".into())),
                ("enter".into(), Some("select:accept".into())),
                ("escape".into(), Some("select:cancel".into())),
            ]),
        },
        // -- Plugin --
        KeybindingBlock {
            context: "Plugin".to_string(),
            bindings: HashMap::from([
                ("space".into(), Some("plugin:toggle".into())),
                ("i".into(), Some("plugin:install".into())),
            ]),
        },
    ]
}

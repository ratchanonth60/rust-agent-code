use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Keybinding Contexts
// ---------------------------------------------------------------------------

/// UI context that determines which keybindings are active.
///
/// Mirrors the TypeScript `KeybindingContextName` union.
/// The resolver checks a set of active contexts to decide which
/// bindings to evaluate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum KeybindingContext {
    Global,
    Chat,
    Autocomplete,
    Confirmation,
    Help,
    Transcript,
    HistorySearch,
    Task,
    ThemePicker,
    Settings,
    Tabs,
    Attachments,
    Footer,
    MessageSelector,
    DiffDialog,
    ModelPicker,
    Select,
    Plugin,
}

impl KeybindingContext {
    /// Human-readable description of what this context represents.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Global => "Always active — available in every screen",
            Self::Chat => "Chat input is focused",
            Self::Autocomplete => "Autocomplete dropdown is visible",
            Self::Confirmation => "A confirmation dialog is active",
            Self::Help => "Help screen is open",
            Self::Transcript => "Conversation transcript view",
            Self::HistorySearch => "History search overlay is active",
            Self::Task => "Task/background job panel is focused",
            Self::ThemePicker => "Theme picker dialog",
            Self::Settings => "Settings screen",
            Self::Tabs => "Tab bar is focused",
            Self::Attachments => "Attachment list is focused",
            Self::Footer => "Footer panel is focused",
            Self::MessageSelector => "Message selection mode",
            Self::DiffDialog => "Diff viewer dialog",
            Self::ModelPicker => "Model picker dialog",
            Self::Select => "Generic selection list",
            Self::Plugin => "Plugin management screen",
        }
    }
}

// ---------------------------------------------------------------------------
// Keybinding Actions
// ---------------------------------------------------------------------------

/// An action that can be triggered by a keybinding.
///
/// Covers 70+ actions grouped by domain, mirroring the TS
/// `KEYBINDING_ACTIONS` constant. Includes a `Command(String)` variant
/// for user-defined slash-command bindings (`command:<name>`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeybindingAction {
    // -- App --
    AppInterrupt,
    AppExit,
    AppRedraw,
    AppToggleTodos,
    AppToggleTranscript,
    AppToggleTerminal,
    AppGlobalSearch,
    AppQuickOpen,

    // -- History --
    HistorySearch,
    HistoryPrevious,
    HistoryNext,

    // -- Chat --
    ChatCancel,
    ChatKillAgents,
    ChatCycleMode,
    ChatModelPicker,
    ChatFastMode,
    ChatThinkingToggle,
    ChatSubmit,
    ChatNewline,
    ChatUndo,
    ChatExternalEditor,
    ChatStash,
    ChatImagePaste,
    ChatMessageActions,

    // -- Autocomplete --
    AutocompleteAccept,
    AutocompleteDismiss,
    AutocompletePrevious,
    AutocompleteNext,

    // -- Confirmation --
    ConfirmYes,
    ConfirmNo,
    ConfirmPrevious,
    ConfirmNext,
    ConfirmNextField,
    ConfirmPreviousField,
    ConfirmCycleMode,
    ConfirmToggle,
    ConfirmToggleExplanation,

    // -- Tabs --
    TabsNext,
    TabsPrevious,

    // -- Transcript --
    TranscriptToggleShowAll,
    TranscriptExit,

    // -- History Search --
    HistorySearchNext,
    HistorySearchAccept,
    HistorySearchCancel,
    HistorySearchExecute,

    // -- Task --
    TaskBackground,

    // -- Theme --
    ThemeToggleSyntaxHighlighting,

    // -- Help --
    HelpDismiss,

    // -- Attachments --
    AttachmentsNext,
    AttachmentsPrevious,
    AttachmentsRemove,
    AttachmentsExit,

    // -- Footer --
    FooterUp,
    FooterDown,
    FooterNext,
    FooterPrevious,
    FooterOpenSelected,
    FooterClearSelection,
    FooterClose,

    // -- Message Selector --
    MessageSelectorUp,
    MessageSelectorDown,
    MessageSelectorTop,
    MessageSelectorBottom,
    MessageSelectorSelect,

    // -- Diff --
    DiffDismiss,
    DiffPreviousSource,
    DiffNextSource,
    DiffBack,
    DiffViewDetails,
    DiffPreviousFile,
    DiffNextFile,

    // -- Model Picker --
    ModelPickerDecreaseEffort,
    ModelPickerIncreaseEffort,

    // -- Select --
    SelectNext,
    SelectPrevious,
    SelectAccept,
    SelectCancel,

    // -- Plugin --
    PluginToggle,
    PluginInstall,

    // -- Permission --
    PermissionToggleDebug,

    // -- Settings --
    SettingsSearch,
    SettingsRetry,
    SettingsClose,

    // -- Voice --
    VoicePushToTalk,

    // -- User-defined slash command (e.g. "command:commit") --
    Command(String),
}

impl KeybindingAction {
    /// Parses an action string like `"chat:submit"` or `"command:commit"`.
    ///
    /// Returns `None` for unrecognised action strings.
    pub fn parse_action(s: &str) -> Option<Self> {
        if let Some(cmd) = s.strip_prefix("command:") {
            return Some(Self::Command(cmd.to_string()));
        }
        let action = match s {
            "app:interrupt" => Self::AppInterrupt,
            "app:exit" => Self::AppExit,
            "app:redraw" => Self::AppRedraw,
            "app:toggleTodos" => Self::AppToggleTodos,
            "app:toggleTranscript" => Self::AppToggleTranscript,
            "app:toggleTerminal" => Self::AppToggleTerminal,
            "app:globalSearch" => Self::AppGlobalSearch,
            "app:quickOpen" => Self::AppQuickOpen,

            "history:search" => Self::HistorySearch,
            "history:previous" => Self::HistoryPrevious,
            "history:next" => Self::HistoryNext,

            "chat:cancel" => Self::ChatCancel,
            "chat:killAgents" => Self::ChatKillAgents,
            "chat:cycleMode" => Self::ChatCycleMode,
            "chat:modelPicker" => Self::ChatModelPicker,
            "chat:fastMode" => Self::ChatFastMode,
            "chat:thinkingToggle" => Self::ChatThinkingToggle,
            "chat:submit" => Self::ChatSubmit,
            "chat:newline" => Self::ChatNewline,
            "chat:undo" => Self::ChatUndo,
            "chat:externalEditor" => Self::ChatExternalEditor,
            "chat:stash" => Self::ChatStash,
            "chat:imagePaste" => Self::ChatImagePaste,
            "chat:messageActions" => Self::ChatMessageActions,

            "autocomplete:accept" => Self::AutocompleteAccept,
            "autocomplete:dismiss" => Self::AutocompleteDismiss,
            "autocomplete:previous" => Self::AutocompletePrevious,
            "autocomplete:next" => Self::AutocompleteNext,

            "confirm:yes" => Self::ConfirmYes,
            "confirm:no" => Self::ConfirmNo,
            "confirm:previous" => Self::ConfirmPrevious,
            "confirm:next" => Self::ConfirmNext,
            "confirm:nextField" => Self::ConfirmNextField,
            "confirm:previousField" => Self::ConfirmPreviousField,
            "confirm:cycleMode" => Self::ConfirmCycleMode,
            "confirm:toggle" => Self::ConfirmToggle,
            "confirm:toggleExplanation" => Self::ConfirmToggleExplanation,

            "tabs:next" => Self::TabsNext,
            "tabs:previous" => Self::TabsPrevious,

            "transcript:toggleShowAll" => Self::TranscriptToggleShowAll,
            "transcript:exit" => Self::TranscriptExit,

            "historySearch:next" => Self::HistorySearchNext,
            "historySearch:accept" => Self::HistorySearchAccept,
            "historySearch:cancel" => Self::HistorySearchCancel,
            "historySearch:execute" => Self::HistorySearchExecute,

            "task:background" => Self::TaskBackground,
            "theme:toggleSyntaxHighlighting" => Self::ThemeToggleSyntaxHighlighting,
            "help:dismiss" => Self::HelpDismiss,

            "attachments:next" => Self::AttachmentsNext,
            "attachments:previous" => Self::AttachmentsPrevious,
            "attachments:remove" => Self::AttachmentsRemove,
            "attachments:exit" => Self::AttachmentsExit,

            "footer:up" => Self::FooterUp,
            "footer:down" => Self::FooterDown,
            "footer:next" => Self::FooterNext,
            "footer:previous" => Self::FooterPrevious,
            "footer:openSelected" => Self::FooterOpenSelected,
            "footer:clearSelection" => Self::FooterClearSelection,
            "footer:close" => Self::FooterClose,

            "messageSelector:up" => Self::MessageSelectorUp,
            "messageSelector:down" => Self::MessageSelectorDown,
            "messageSelector:top" => Self::MessageSelectorTop,
            "messageSelector:bottom" => Self::MessageSelectorBottom,
            "messageSelector:select" => Self::MessageSelectorSelect,

            "diff:dismiss" => Self::DiffDismiss,
            "diff:previousSource" => Self::DiffPreviousSource,
            "diff:nextSource" => Self::DiffNextSource,
            "diff:back" => Self::DiffBack,
            "diff:viewDetails" => Self::DiffViewDetails,
            "diff:previousFile" => Self::DiffPreviousFile,
            "diff:nextFile" => Self::DiffNextFile,

            "modelPicker:decreaseEffort" => Self::ModelPickerDecreaseEffort,
            "modelPicker:increaseEffort" => Self::ModelPickerIncreaseEffort,

            "select:next" => Self::SelectNext,
            "select:previous" => Self::SelectPrevious,
            "select:accept" => Self::SelectAccept,
            "select:cancel" => Self::SelectCancel,

            "plugin:toggle" => Self::PluginToggle,
            "plugin:install" => Self::PluginInstall,

            "permission:toggleDebug" => Self::PermissionToggleDebug,

            "settings:search" => Self::SettingsSearch,
            "settings:retry" => Self::SettingsRetry,
            "settings:close" => Self::SettingsClose,

            "voice:pushToTalk" => Self::VoicePushToTalk,

            _ => return None,
        };
        Some(action)
    }
}

impl fmt::Display for KeybindingAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Command(name) => write!(f, "command:{}", name),
            Self::AppInterrupt => write!(f, "app:interrupt"),
            Self::AppExit => write!(f, "app:exit"),
            Self::ChatSubmit => write!(f, "chat:submit"),
            Self::ChatCancel => write!(f, "chat:cancel"),
            Self::HistoryPrevious => write!(f, "history:previous"),
            Self::HistoryNext => write!(f, "history:next"),
            other => write!(f, "{:?}", other),
        }
    }
}

// ---------------------------------------------------------------------------
// Parsed keystroke primitives
// ---------------------------------------------------------------------------

/// A single parsed keystroke such as `ctrl+k` or `shift+enter`.
///
/// Produced by [`super::parser::parse_keystroke`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParsedKeystroke {
    /// The base key name (lowercase), e.g. `"k"`, `"enter"`, `"escape"`.
    pub key: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl fmt::Display for ParsedKeystroke {
    /// Formats as `"ctrl+shift+k"` style string.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.ctrl { parts.push("ctrl"); }
        if self.alt { parts.push("alt"); }
        if self.shift { parts.push("shift"); }
        if self.meta { parts.push("meta"); }
        parts.push(&self.key);
        write!(f, "{}", parts.join("+"))
    }
}

/// A chord is a sequence of keystrokes (e.g. `ctrl+x ctrl+k`).
///
/// Single-key bindings have exactly one element.
pub type Chord = Vec<ParsedKeystroke>;

/// Formats a chord for display (e.g. `"ctrl+x ctrl+k"`).
pub fn chord_to_string(chord: &Chord) -> String {
    chord.iter()
        .map(|k| k.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Binding types
// ---------------------------------------------------------------------------

/// A fully parsed keybinding: chord → action within a context.
///
/// When `action` is `None`, this entry explicitly unbinds a default shortcut.
#[derive(Debug, Clone)]
pub struct ParsedBinding {
    pub chord: Chord,
    pub action: Option<KeybindingAction>,
    pub context: KeybindingContext,
}

/// A raw keybinding block as it appears in `keybindings.json`.
///
/// # JSON format
///
/// ```json
/// { "context": "Chat", "bindings": { "ctrl+k": "chat:cancel", "ctrl+x ctrl+k": null } }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindingBlock {
    pub context: String,
    pub bindings: std::collections::HashMap<String, Option<String>>,
}

// ---------------------------------------------------------------------------
// Chord resolution result
// ---------------------------------------------------------------------------

/// The result of attempting to resolve a keystroke (possibly mid-chord).
#[derive(Debug, Clone)]
pub enum ChordResolveResult {
    /// A complete match — fire this action.
    Match(KeybindingAction),
    /// No binding matched.
    None,
    /// The key was explicitly unbound (`null` in config).
    Unbound,
    /// The keystroke is a valid chord prefix; waiting for more keys.
    ChordStarted(Vec<ParsedKeystroke>),
    /// A pending chord was cancelled (escape or non-matching key).
    ChordCancelled,
}

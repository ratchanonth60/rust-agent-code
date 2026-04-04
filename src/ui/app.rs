//! TUI application loop — Claude Code-style terminal interface.
//!
//! Three-zone layout: scrollable conversation, status line, and input prompt.
//! Uses typed [`MessageEntry`] variants to render user, assistant, tool, and
//! permission messages with the correct visual style.

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, MouseEventKind};
use ratatui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame, Terminal,
};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

use crate::commands::{CommandContext, CommandRegistry, CommandResult};
use crate::tools::ask_user::QuestionRequest;

// ── Claude Code style characters ─────────────────────────────────────────
const ASSISTANT_PREFIX: &str = "  \u{23BF} "; // ⎿ (left square bracket extension)
const DIVIDER_CHAR: char = '\u{2500}'; // ─ (box-drawing horizontal)
const DOT: &str = "\u{25CF}"; // ● (filled circle)
const PROMPT_CHAR: &str = ">";
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const AUTOCOMPLETE_MAX_ITEMS: usize = 5;
const FILE_SCAN_MAX: usize = 5000;
const FILE_SUGGEST_DEBOUNCE: Duration = Duration::from_millis(50);

fn welcome_art() -> String {
    use std::io::Write;
    let msg = "R U S T   A G E N T";
    let width = msg.len() + 4; // padding inside the speech bubble
    let mut buf = Vec::new();
    ferris_says::say(msg, width, &mut buf).unwrap_or_else(|_| {
        let _ = write!(buf, "🦀 Rust Agent 🦀");
    });
    String::from_utf8_lossy(&buf).to_string()
}

/// Events sent from the engine background task to the TUI for rendering.
pub enum UiEvent {
    LLMResponse(String),
    LLMError(String),
    ToolStarted(String),
    ToolFinished(String),
    StreamDelta(String),
    StreamStart,
    StreamEnd,
    PermissionRequest {
        tool_name: String,
        description: String,
        response_tx: oneshot::Sender<PermissionResponse>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionResponse {
    Allow,
    Deny,
    AlwaysAllow,
}

// ── Typed message entries ────────────────────────────────────────────────

#[derive(Clone)]
enum MessageEntry {
    /// User prompt: "> text"
    User(String),
    /// Assistant text: "  ⎿ text"
    Assistant(String),
    /// Tool started: "  ● ToolName"  (dim while running, green when done)
    ToolUse {
        name: String,
        done: bool,
        error: bool,
    },
    /// Error message (red)
    Error(String),
    /// System/info message (dim)
    System(String),
    /// Horizontal divider
    Divider,
    /// Permission request line
    Permission {
        tool_name: String,
        description: String,
    },
    /// Interactive question from AskUserQuestionTool
    Question {
        question: String,
        #[allow(dead_code)]
        options: Vec<String>,
    },
}

#[derive(Clone)]
enum AutocompleteKind {
    Command,
    File,
}

#[derive(Clone)]
struct AutocompleteItem {
    display: String,
    insert: String,
    kind: AutocompleteKind,
}

pub struct App {
    pub input: String,
    messages: Vec<MessageEntry>,
    pub exit: bool,
    pub running_tool: Option<String>,
    pub frame_ticker: usize,
    pub current_stream: Option<String>,
    tx_to_engine: mpsc::Sender<String>,
    rx_from_engine: mpsc::Receiver<UiEvent>,
    /// Receiver for interactive questions from AskUserQuestionTool.
    rx_questions: mpsc::Receiver<QuestionRequest>,
    bindings: Vec<crate::keybindings::ParsedBinding>,
    scroll_offset: u32,
    /// Set when streaming is active to suppress the duplicate LLMResponse.
    streamed_this_turn: bool,
    /// True when the user has manually scrolled — suppresses auto-scroll until
    /// a new message is submitted.
    user_scrolled: bool,
    pending_permission: Option<PendingPermission>,
    /// Pending question from AskUserQuestionTool awaiting user input.
    pending_question: Option<PendingQuestion>,
    pub cost_tracker: Option<Arc<Mutex<crate::engine::cost_tracker::CostTracker>>>,
    /// Terminal width for divider rendering.
    term_width: u16,
    /// Conversation area height (updated each frame).
    conv_height: u16,
    /// True after user submits a query until the first response event arrives.
    waiting_for_response: bool,
    /// Submitted chat inputs (newest at the end) for up/down recall.
    input_history: Vec<String>,
    /// Cursor into `input_history` while browsing; `None` means not browsing.
    history_cursor: Option<usize>,
    /// Draft input captured before entering history navigation.
    history_draft: Option<String>,
    /// Registry of slash commands.
    command_registry: CommandRegistry,
    /// Active autocomplete candidates.
    autocomplete_items: Vec<AutocompleteItem>,
    /// Selected autocomplete row.
    autocomplete_selected: usize,
    /// Prevent immediate reopen after explicit dismiss.
    autocomplete_dismissed_input: Option<String>,
    /// Marks input-driven autocomplete state as stale.
    autocomplete_dirty: bool,
    /// Timestamp of latest file completion refresh.
    last_file_refresh: Option<Instant>,
    /// Cached workspace file index for file suggestions.
    file_index: Option<Vec<String>>,
}

struct PendingPermission {
    tool_name: String,
    #[allow(dead_code)]
    description: String,
    response_tx: oneshot::Sender<PermissionResponse>,
}

/// A question from `AskUserQuestionTool` waiting for the user's typed answer.
struct PendingQuestion {
    response_tx: oneshot::Sender<String>,
}

impl App {
    pub fn new(
        tx_to_engine: mpsc::Sender<String>,
        rx_from_engine: mpsc::Receiver<UiEvent>,
        rx_questions: mpsc::Receiver<QuestionRequest>,
        command_registry: CommandRegistry,
    ) -> Self {
        let load_result = crate::keybindings::load_keybindings();
        if !load_result.warnings.is_empty() {
            tracing::warn!("Keybinding warnings: {:?}", load_result.warnings);
        }
        Self {
            input: String::new(),
            messages: vec![MessageEntry::System(welcome_art())],
            exit: false,
            running_tool: None,
            frame_ticker: 0,
            current_stream: None,
            tx_to_engine,
            rx_from_engine,
            rx_questions,
            bindings: load_result.bindings,
            scroll_offset: 0,
            streamed_this_turn: false,
            user_scrolled: false,
            pending_permission: None,
            pending_question: None,
            cost_tracker: None,
            term_width: 80,
            conv_height: 24,
            waiting_for_response: false,
            input_history: Vec::new(),
            history_cursor: None,
            history_draft: None,
            command_registry,
            autocomplete_items: Vec::new(),
            autocomplete_selected: 0,
            autocomplete_dismissed_input: None,
            autocomplete_dirty: true,
            last_file_refresh: None,
            file_index: None,
        }
    }

    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        while !self.exit {
            terminal.draw(|f| {
                self.ui(f);
            })?;

            while let Ok(event) = self.rx_from_engine.try_recv() {
                match event {
                    UiEvent::LLMResponse(res) => {
                        self.waiting_for_response = false;
                        // Skip if content was already pushed by StreamEnd
                        if self.streamed_this_turn {
                            self.streamed_this_turn = false;
                        } else {
                            self.messages.push(MessageEntry::Assistant(res));
                            self.auto_scroll();
                        }
                    }
                    UiEvent::LLMError(err) => {
                        self.waiting_for_response = false;
                        self.messages.push(MessageEntry::Error(err));
                        self.auto_scroll();
                    }
                    UiEvent::ToolStarted(name) => {
                        self.waiting_for_response = false;
                        self.running_tool = Some(name.clone());
                        self.messages.push(MessageEntry::ToolUse {
                            name,
                            done: false,
                            error: false,
                        });
                        self.auto_scroll();
                    }
                    UiEvent::ToolFinished(name) => {
                        self.running_tool = None;
                        // Mark the matching tool entry as done
                        for msg in self.messages.iter_mut().rev() {
                            if let MessageEntry::ToolUse {
                                name: ref n,
                                ref mut done,
                                ..
                            } = msg
                            {
                                if *n == name {
                                    *done = true;
                                    break;
                                }
                            }
                        }
                    }
                    UiEvent::StreamStart => {
                        self.waiting_for_response = false;
                        self.current_stream = Some(String::new());
                        self.streamed_this_turn = true;
                    }
                    UiEvent::StreamDelta(text) => {
                        if let Some(ref mut stream) = self.current_stream {
                            stream.push_str(&text);
                        } else {
                            self.current_stream = Some(text);
                        }
                        self.auto_scroll();
                    }
                    UiEvent::StreamEnd => {
                        if let Some(stream) = self.current_stream.take() {
                            if !stream.is_empty() {
                                self.messages.push(MessageEntry::Assistant(stream));
                            }
                        }
                        self.auto_scroll();
                    }
                    UiEvent::PermissionRequest {
                        tool_name,
                        description,
                        response_tx,
                    } => {
                        self.messages.push(MessageEntry::Permission {
                            tool_name: tool_name.clone(),
                            description: description.clone(),
                        });
                        self.pending_permission = Some(PendingPermission {
                            tool_name,
                            description,
                            response_tx,
                        });
                        self.auto_scroll();
                    }
                }
            }

            self.maybe_refresh_autocomplete();

            // Poll for incoming questions from AskUserQuestionTool
            while let Ok(qreq) = self.rx_questions.try_recv() {
                let mut question_text = format!("  ? {}", qreq.question);
                if !qreq.options.is_empty() {
                    for (i, opt) in qreq.options.iter().enumerate() {
                        question_text.push_str(&format!("\n    {}. {}", i + 1, opt));
                    }
                }
                self.messages.push(MessageEntry::Question {
                    question: question_text,
                    options: qreq.options,
                });
                self.pending_question = Some(PendingQuestion {
                    response_tx: qreq.response_tx,
                });
                self.auto_scroll();
            }

            if event::poll(Duration::from_millis(50))? {
                self.frame_ticker = self.frame_ticker.wrapping_add(1);
                match event::read()? {
                    Event::Key(key) => {
                    // Question prompt intercept — send typed answer back to AskUserQuestionTool
                    if self.pending_question.is_some() {
                        if let KeyCode::Enter = key.code {
                            let answer = self.input.trim().to_string();
                            if !answer.is_empty() {
                                let q = self.pending_question.take().unwrap();
                                self.messages.push(MessageEntry::System(format!(
                                    "  → {}", answer
                                )));
                                let _ = q.response_tx.send(answer);
                                self.input.clear();
                                self.auto_scroll();
                            }
                            continue;
                        }
                        match key.code {
                            KeyCode::Char(c) => {
                                self.input.push(c);
                                self.mark_autocomplete_dirty();
                            }
                            KeyCode::Backspace => {
                                self.input.pop();
                                self.mark_autocomplete_dirty();
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // Permission prompt intercept
                    if self.pending_permission.is_some() {
                        let response = match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                                Some(PermissionResponse::Allow)
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') => {
                                Some(PermissionResponse::Deny)
                            }
                            KeyCode::Char('a') | KeyCode::Char('A') => {
                                Some(PermissionResponse::AlwaysAllow)
                            }
                            _ => None,
                        };

                        if let Some(resp) = response {
                            let perm = self.pending_permission.take().unwrap();
                            let label = match &resp {
                                PermissionResponse::Allow => "allowed",
                                PermissionResponse::Deny => "denied",
                                PermissionResponse::AlwaysAllow => "always allowed",
                            };
                            self.messages.push(MessageEntry::System(format!(
                                "  {} {}",
                                perm.tool_name, label
                            )));
                            let _ = perm.response_tx.send(resp);
                            self.auto_scroll();
                            continue;
                        }
                        continue;
                    }

                    use crate::keybindings::{resolve_key, KeybindingAction, KeybindingContext};
                    let mut active_contexts = vec![KeybindingContext::Global, KeybindingContext::Chat];
                    if self.is_autocomplete_visible() {
                        active_contexts.push(KeybindingContext::Autocomplete);
                    }
                    if let Some(action) = resolve_key(&key, &active_contexts, &self.bindings) {
                        match action {
                            KeybindingAction::AppInterrupt | KeybindingAction::AppExit => {
                                self.exit = true;
                            }
                            KeybindingAction::AppRedraw => {}
                            KeybindingAction::ChatSubmit => {
                                let submitted = self.input.trim().to_string();
                                // If there's a pending question, the user's input
                                // is the answer — send it back to the tool.
                                if let Some(pq) = self.pending_question.take() {
                                    if !submitted.is_empty() {
                                        self.messages.push(MessageEntry::User(submitted.clone()));
                                        let _ = pq.response_tx.send(submitted);
                                        self.input.clear();
                                        self.auto_scroll();
                                    } else {
                                        // Put it back — don't send empty answer
                                        self.pending_question = Some(pq);
                                    }
                                } else if submitted == "quit" || submitted == "exit" {
                                    self.exit = true;
                                } else if submitted.starts_with('/') {
                                    self.handle_slash_command(&submitted);
                                    self.input.clear();
                                    self.clear_autocomplete();
                                } else if !submitted.is_empty() {
                                    self.push_history_entry(submitted.clone());
                                    self.messages.push(MessageEntry::Divider);
                                    self.messages.push(MessageEntry::User(submitted.clone()));
                                    let _ = self.tx_to_engine.send(submitted).await;
                                    self.input.clear();
                                    self.reset_history_navigation();
                                    self.clear_autocomplete();
                                    self.user_scrolled = false;
                                    self.waiting_for_response = true;
                                    self.auto_scroll();
                                }
                            }
                            KeybindingAction::ChatCancel => {
                                self.input.clear();
                                self.reset_history_navigation();
                                self.clear_autocomplete();
                            }
                            KeybindingAction::AutocompleteAccept => {
                                self.accept_autocomplete();
                            }
                            KeybindingAction::AutocompleteDismiss => {
                                self.dismiss_autocomplete();
                            }
                            KeybindingAction::AutocompletePrevious => {
                                self.autocomplete_previous();
                            }
                            KeybindingAction::AutocompleteNext => {
                                self.autocomplete_next();
                            }
                            KeybindingAction::HistoryPrevious => {
                                self.navigate_history_previous();
                            }
                            KeybindingAction::HistoryNext => {
                                self.navigate_history_next();
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Char(c) => {
                                self.reset_history_navigation();
                                self.input.push(c);
                                self.mark_autocomplete_dirty();
                            }
                            KeyCode::Backspace => {
                                self.reset_history_navigation();
                                self.input.pop();
                                self.mark_autocomplete_dirty();
                            }
                            KeyCode::Up => {
                                self.user_scrolled = true;
                                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                            }
                            KeyCode::Down => {
                                self.user_scrolled = true;
                                self.scroll_offset = self.scroll_offset.saturating_add(1);
                            }
                            KeyCode::PageUp => {
                                self.user_scrolled = true;
                                self.scroll_offset = self.scroll_offset.saturating_sub(10);
                            }
                            KeyCode::PageDown => {
                                self.user_scrolled = true;
                                self.scroll_offset = self.scroll_offset.saturating_add(10);
                            }
                            _ => {}
                        }
                    }
                } // end Event::Key
                    Event::Mouse(mouse) => {
                        match mouse.kind {
                            MouseEventKind::ScrollUp => {
                                self.user_scrolled = true;
                                self.scroll_offset = self.scroll_offset.saturating_sub(3);
                            }
                            MouseEventKind::ScrollDown => {
                                self.user_scrolled = true;
                                self.scroll_offset = self.scroll_offset.saturating_add(3);
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                } // end match event
            } else {
                self.frame_ticker = self.frame_ticker.wrapping_add(1);
            }
        }
        Ok(())
    }

    fn auto_scroll(&mut self) {
        if self.user_scrolled {
            return;
        }
        let total_lines: usize = self.messages.iter().map(|m| self.entry_line_count(m)).sum();
        let stream_lines = self
            .current_stream
            .as_ref()
            .map(|s| self.wrapped_line_count(s, ASSISTANT_PREFIX.len()))
            .unwrap_or(0);
        let visible = self.conv_height as usize;
        self.scroll_offset = (total_lines + stream_lines).saturating_sub(visible) as u32;
    }

    /// Count visual lines for a message, accounting for soft wrapping.
    fn entry_line_count(&self, entry: &MessageEntry) -> usize {
        let w = self.term_width.saturating_sub(2) as usize; // same padding as render
        if w == 0 {
            return 1;
        }
        match entry {
            MessageEntry::User(t) => self.wrapped_line_count(t, 2), // "> " prefix
            MessageEntry::Assistant(t) => self.wrapped_line_count(t, ASSISTANT_PREFIX.len()),
            MessageEntry::Error(t) => self.wrapped_line_count(t, ASSISTANT_PREFIX.len()),
            MessageEntry::System(t) => self.wrapped_line_count(t, 0),
            MessageEntry::ToolUse { .. } => 1,
            MessageEntry::Divider => 1,
            MessageEntry::Permission { .. } => 2,
            MessageEntry::Question { question, .. } => self.wrapped_line_count(question, 0),
        }
    }

    /// Count visual lines after soft wrapping, given a prefix width.
    fn wrapped_line_count(&self, text: &str, prefix_len: usize) -> usize {
        let w = self.term_width.saturating_sub(2) as usize;
        if w == 0 {
            return text.lines().count().max(1);
        }
        let mut count = 0usize;
        for line in text.lines() {
            let total_chars = prefix_len + line.len();
            if total_chars == 0 {
                count += 1;
            } else {
                count += (total_chars + w - 1) / w; // ceil division
            }
        }
        count.max(1)
    }

    fn handle_slash_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let command_name = parts
            .first()
            .copied()
            .unwrap_or("")
            .trim_start_matches('/');
        let args = if parts.len() > 1 {
            parts[1..].join(" ")
        } else {
            String::new()
        };

        // Special case: /help needs access to the registry for listing commands.
        if command_name == "help" {
            let commands = self.command_registry.list();
            let help_text = crate::commands::help::build_help_text(&commands);
            self.messages.push(MessageEntry::System(help_text));
            self.auto_scroll();
            return;
        }

        if let Some(command) = self.command_registry.find(command_name) {
            let ctx = CommandContext {
                cost_tracker: self.cost_tracker.clone(),
                cwd: std::env::current_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from(".")),
            };

            match command.execute(&args, &ctx) {
                Ok(result) => match result {
                    CommandResult::Text(text) => {
                        self.messages.push(MessageEntry::System(text));
                    }
                    CommandResult::Clear => {
                        self.messages.clear();
                        self.scroll_offset = 0;
                        self.messages.push(MessageEntry::System(
                            "  Conversation cleared.".to_string(),
                        ));
                    }
                    CommandResult::Exit => {
                        self.exit = true;
                    }
                    CommandResult::Silent => {}
                    CommandResult::Prompt(_prompt_cmd) => {
                        // TODO: send prompt_cmd.content to engine with allowed_tools filter
                        self.messages.push(MessageEntry::System(
                            "  Prompt commands not yet wired to engine.".to_string(),
                        ));
                    }
                },
                Err(e) => {
                    self.messages
                        .push(MessageEntry::Error(format!("  Command error: {}", e)));
                }
            }
        } else {
            self.messages.push(MessageEntry::System(format!(
                "  Unknown command: /{}",
                command_name
            )));
        }
        self.auto_scroll();
    }

    fn is_autocomplete_visible(&self) -> bool {
        self.pending_permission.is_none()
            && self.pending_question.is_none()
            && !self.autocomplete_items.is_empty()
    }

    fn mark_autocomplete_dirty(&mut self) {
        self.autocomplete_dirty = true;
        if self
            .autocomplete_dismissed_input
            .as_ref()
            .map(|d| d != &self.input)
            .unwrap_or(false)
        {
            self.autocomplete_dismissed_input = None;
        }
    }

    fn clear_autocomplete(&mut self) {
        self.autocomplete_items.clear();
        self.autocomplete_selected = 0;
        self.autocomplete_dismissed_input = None;
        self.autocomplete_dirty = false;
    }

    fn maybe_refresh_autocomplete(&mut self) {
        if !self.autocomplete_dirty {
            return;
        }
        if self.pending_permission.is_some() || self.pending_question.is_some() {
            self.autocomplete_items.clear();
            self.autocomplete_selected = 0;
            self.autocomplete_dirty = false;
            return;
        }

        if self
            .autocomplete_dismissed_input
            .as_ref()
            .map(|d| d == &self.input)
            .unwrap_or(false)
        {
            self.autocomplete_items.clear();
            self.autocomplete_selected = 0;
            self.autocomplete_dirty = false;
            return;
        }

        let mut next_items = self.build_command_suggestions();
        if next_items.is_empty() {
            if let Some(query) = self.extract_file_query() {
                let should_debounce = self
                    .last_file_refresh
                    .map(|t| t.elapsed() < FILE_SUGGEST_DEBOUNCE)
                    .unwrap_or(false);
                if should_debounce {
                    return;
                }
                next_items = self.build_file_suggestions(&query);
                self.last_file_refresh = Some(Instant::now());
            }
        }

        self.autocomplete_items = next_items;
        if self.autocomplete_selected >= self.autocomplete_items.len() {
            self.autocomplete_selected = 0;
        }
        self.autocomplete_dirty = false;
    }

    fn build_command_suggestions(&self) -> Vec<AutocompleteItem> {
        let token = self.input.split_whitespace().next().unwrap_or("");
        if !token.starts_with('/') {
            return Vec::new();
        }

        let needle = token.trim_start_matches('/').to_lowercase();
        let mut seen: HashSet<String> = HashSet::new();
        let mut scored: Vec<(usize, String)> = Vec::new();

        for command in self.command_registry.list() {
            let mut candidates = vec![command.name().to_string()];
            candidates.extend(command.aliases().into_iter().map(|a| a.to_string()));
            for name in candidates {
                if !seen.insert(name.clone()) {
                    continue;
                }
                let lower = name.to_lowercase();
                if needle.is_empty() || lower.contains(&needle) {
                    let score = if lower.starts_with(&needle) { 0 } else { 1 };
                    scored.push((score, name));
                }
            }
        }

        scored.sort_by(|a, b| a.cmp(b));
        scored
            .into_iter()
            .take(AUTOCOMPLETE_MAX_ITEMS)
            .map(|(_, name)| AutocompleteItem {
                display: format!("/{}", name),
                insert: format!("/{} ", name),
                kind: AutocompleteKind::Command,
            })
            .collect()
    }

    fn extract_file_query(&self) -> Option<String> {
        let token = self.input.split_whitespace().last()?;
        if token.starts_with('@') {
            Some(token.trim_start_matches('@').to_lowercase())
        } else {
            None
        }
    }

    fn build_file_suggestions(&mut self, query: &str) -> Vec<AutocompleteItem> {
        if self.file_index.is_none() {
            self.file_index = Some(self.scan_workspace_files());
        }
        let files = self.file_index.clone().unwrap_or_default();
        let mut scored: Vec<(usize, String)> = files
            .into_iter()
            .filter_map(|path| {
                let lower = path.to_lowercase();
                if query.is_empty() || lower.contains(query) {
                    let score = if lower.starts_with(query) { 0 } else { 1 };
                    Some((score, path))
                } else {
                    None
                }
            })
            .collect();
        scored.sort_by(|a, b| a.cmp(b));

        scored
            .into_iter()
            .take(AUTOCOMPLETE_MAX_ITEMS)
            .map(|(_, path)| AutocompleteItem {
                display: format!("@{}", path),
                insert: format!("@{}", path),
                kind: AutocompleteKind::File,
            })
            .collect()
    }

    fn scan_workspace_files(&self) -> Vec<String> {
        fn walk(dir: &std::path::Path, out: &mut Vec<String>, root: &std::path::Path) {
            if out.len() >= FILE_SCAN_MAX {
                return;
            }
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                if out.len() >= FILE_SCAN_MAX {
                    return;
                }
                let path = entry.path();
                let name = entry.file_name();
                if let Some(name_str) = name.to_str() {
                    if matches!(name_str, ".git" | "target" | "node_modules") {
                        continue;
                    }
                }
                if path.is_dir() {
                    walk(&path, out, root);
                } else if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_string_lossy().to_string());
                }
            }
        }

        let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let mut out = Vec::new();
        walk(&root, &mut out, &root);
        out
    }

    fn accept_autocomplete(&mut self) {
        if self.autocomplete_items.is_empty() {
            return;
        }
        let idx = self
            .autocomplete_selected
            .min(self.autocomplete_items.len().saturating_sub(1));
        let item = self.autocomplete_items[idx].clone();

        match item.kind {
            AutocompleteKind::Command => {
                let mut parts = self.input.splitn(2, ' ');
                let _first = parts.next();
                if let Some(rest) = parts.next() {
                    self.input = format!("{}{}", item.insert, rest.trim_start());
                } else {
                    self.input = item.insert;
                }
            }
            AutocompleteKind::File => {
                self.input = Self::replace_last_token(&self.input, &item.insert);
            }
        }

        self.autocomplete_items.clear();
        self.autocomplete_selected = 0;
        self.autocomplete_dismissed_input = None;
        self.autocomplete_dirty = true;
    }

    fn dismiss_autocomplete(&mut self) {
        self.autocomplete_dismissed_input = Some(self.input.clone());
        self.autocomplete_items.clear();
        self.autocomplete_selected = 0;
        self.autocomplete_dirty = false;
    }

    fn autocomplete_previous(&mut self) {
        if self.autocomplete_items.is_empty() {
            return;
        }
        if self.autocomplete_selected == 0 {
            self.autocomplete_selected = self.autocomplete_items.len() - 1;
        } else {
            self.autocomplete_selected -= 1;
        }
    }

    fn autocomplete_next(&mut self) {
        if self.autocomplete_items.is_empty() {
            return;
        }
        self.autocomplete_selected = (self.autocomplete_selected + 1) % self.autocomplete_items.len();
    }

    fn replace_last_token(input: &str, replacement: &str) -> String {
        if let Some((idx, _)) = input.char_indices().rev().find(|(_, ch)| ch.is_whitespace()) {
            let prefix = &input[..=idx];
            format!("{}{}", prefix, replacement)
        } else {
            replacement.to_string()
        }
    }

    fn push_history_entry(&mut self, submitted: String) {
        let is_duplicate = self
            .input_history
            .last()
            .map(|s| s == &submitted)
            .unwrap_or(false);
        if !is_duplicate {
            self.input_history.push(submitted);
        }
    }

    fn reset_history_navigation(&mut self) {
        self.history_cursor = None;
        self.history_draft = None;
    }

    fn navigate_history_previous(&mut self) {
        if self.input_history.is_empty() {
            return;
        }

        let next_cursor = match self.history_cursor {
            Some(idx) => idx.saturating_sub(1),
            None => {
                self.history_draft = Some(self.input.clone());
                self.input_history.len() - 1
            }
        };

        self.history_cursor = Some(next_cursor);
        self.input = self.input_history[next_cursor].clone();
    }

    fn navigate_history_next(&mut self) {
        let Some(idx) = self.history_cursor else {
            return;
        };

        if idx + 1 < self.input_history.len() {
            let next_idx = idx + 1;
            self.history_cursor = Some(next_idx);
            self.input = self.input_history[next_idx].clone();
        } else {
            self.history_cursor = None;
            self.input = self.history_draft.take().unwrap_or_default();
        }
    }

    // ── Rendering ────────────────────────────────────────────────────────

    fn ui(&mut self, f: &mut Frame) {
        let area = f.size();
        self.term_width = area.width;

        let autocomplete_height = if self.is_autocomplete_visible() {
            self.autocomplete_items.len().min(AUTOCOMPLETE_MAX_ITEMS) as u16
        } else {
            0
        };

        // Layout: [conversation] [status line] [autocomplete] [prompt]
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),    // conversation
                Constraint::Length(1), // status line
                Constraint::Length(autocomplete_height), // autocomplete dropdown
                Constraint::Length(1), // prompt input
            ])
            .split(area);

        self.conv_height = chunks[0].height;
        self.render_conversation(f, chunks[0]);
        self.render_status_line(f, chunks[1]);
        if autocomplete_height > 0 {
            self.render_autocomplete(f, chunks[2]);
        }
        self.render_prompt(f, chunks[3]);
    }

    fn render_autocomplete(&self, f: &mut Frame, area: Rect) {
        let mut lines: Vec<Line> = Vec::new();
        for (idx, item) in self
            .autocomplete_items
            .iter()
            .take(AUTOCOMPLETE_MAX_ITEMS)
            .enumerate()
        {
            let selected = idx == self.autocomplete_selected;
            let marker = if selected { "▶" } else { " " };
            let kind = match item.kind {
                AutocompleteKind::Command => "cmd",
                AutocompleteKind::File => "file",
            };
            let style = if selected {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{} {} ", marker, kind), style),
                Span::styled(item.display.clone(), style),
            ]));
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
    }

    fn render_conversation(&self, f: &mut Frame, area: Rect) {
        let w = area.width.saturating_sub(2) as usize; // padding
        let dim = Style::default().fg(Color::DarkGray);
        let mut lines: Vec<Line> = Vec::new();

        for entry in &self.messages {
            match entry {
                MessageEntry::User(text) => {
                    for line in text.lines() {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("{} ", PROMPT_CHAR),
                                Style::default()
                                    .fg(Color::White)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(line.to_string(), Style::default().fg(Color::White)),
                        ]));
                    }
                }
                MessageEntry::Assistant(text) => {
                    for (i, line) in text.lines().enumerate() {
                        let prefix = if i == 0 { ASSISTANT_PREFIX } else { "    " };
                        lines.push(Line::from(vec![
                            Span::styled(prefix.to_string(), dim),
                            Span::raw(line.to_string()),
                        ]));
                    }
                }
                MessageEntry::ToolUse { name, done, error } => {
                    if *error {
                        lines.push(Line::from(vec![
                            Span::raw("  "),
                            Span::styled(DOT, Style::default().fg(Color::Red)),
                            Span::raw(" "),
                            Span::styled(name.clone(), Style::default().fg(Color::Red)),
                        ]));
                    } else if *done {
                        lines.push(Line::from(vec![
                            Span::raw("  "),
                            Span::styled(DOT, Style::default().fg(Color::Green)),
                            Span::raw(" "),
                            Span::styled(
                                name.clone(),
                                Style::default()
                                    .fg(Color::White)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]));
                    } else {
                        let spinner = SPINNER_FRAMES[self.frame_ticker % SPINNER_FRAMES.len()];
                        lines.push(Line::from(vec![
                            Span::raw("  "),
                            Span::styled(spinner.to_string(), Style::default().fg(Color::Cyan)),
                            Span::raw(" "),
                            Span::styled(
                                name.clone(),
                                Style::default()
                                    .fg(Color::White)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]));
                    }
                }
                MessageEntry::Error(text) => {
                    for line in text.lines() {
                        lines.push(Line::from(vec![
                            Span::styled(ASSISTANT_PREFIX.to_string(), dim),
                            Span::styled(line.to_string(), Style::default().fg(Color::Red)),
                        ]));
                    }
                }
                MessageEntry::System(text) => {
                    for line in text.lines() {
                        lines.push(Line::from(Span::styled(line.to_string(), dim)));
                    }
                }
                MessageEntry::Divider => {
                    let divider: String = std::iter::repeat(DIVIDER_CHAR).take(w.min(80)).collect();
                    lines.push(Line::from(Span::styled(divider, dim)));
                }
                MessageEntry::Permission {
                    tool_name,
                    description,
                } => {
                    lines.push(Line::from(vec![
                        Span::styled("  ", dim),
                        Span::styled(
                            tool_name.clone(),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!(" - {}", description),
                            Style::default().fg(Color::Yellow),
                        ),
                    ]));
                    lines.push(Line::from(Span::styled(
                        "  Allow? (y)es / (n)o / (a)lways",
                        Style::default().fg(Color::Yellow),
                    )));
                }
                MessageEntry::Question { question, .. } => {
                    for line in question.lines() {
                        lines.push(Line::from(Span::styled(
                            line.to_string(),
                            Style::default().fg(Color::Cyan),
                        )));
                    }
                }
            }
        }

        // Thinking spinner
        if self.waiting_for_response {
            let spinner = SPINNER_FRAMES[self.frame_ticker % SPINNER_FRAMES.len()];
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", spinner),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    "Thinking...",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
        }

        // Streaming text
        if let Some(ref stream) = self.current_stream {
            if !stream.is_empty() {
                for (i, line) in stream.lines().enumerate() {
                    let prefix = if i == 0 { ASSISTANT_PREFIX } else { "    " };
                    lines.push(Line::from(vec![
                        Span::styled(prefix.to_string(), dim),
                        Span::raw(line.to_string()),
                    ]));
                }
            } else {
                // Empty stream — push a blank streaming line for the cursor
                lines.push(Line::from(vec![
                    Span::styled(ASSISTANT_PREFIX.to_string(), dim),
                ]));
            }
            // Blinking cursor — always on the last streaming line
            let cursor = if self.frame_ticker % 10 < 5 {
                "\u{2588}" // full block
            } else {
                " "
            };
            if let Some(last) = lines.last_mut() {
                last.spans.push(Span::styled(
                    cursor.to_string(),
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }

        let para = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset.min(u16::MAX as u32) as u16, 0));

        f.render_widget(para, area);
    }

    fn render_status_line(&self, f: &mut Frame, area: Rect) {
        let dim = Style::default().fg(Color::DarkGray);
        let w = area.width as usize;

        // Left: model/tool status
        let left = if let Some(ref tool) = self.running_tool {
            format!(" {} ...", tool)
        } else if self.current_stream.is_some() {
            " streaming...".to_string()
        } else if self.waiting_for_response {
            let spinner = SPINNER_FRAMES[self.frame_ticker % SPINNER_FRAMES.len()];
            format!(" {} thinking...", spinner)
        } else {
            String::new()
        };

        // Right: cost if available
        let right = if let Some(ref tracker) = self.cost_tracker {
            if let Ok(t) = tracker.lock() {
                if t.total_cost_usd > 0.0 {
                    format!("${:.4} ", t.total_cost_usd)
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let fill_len = w.saturating_sub(left.len() + right.len());
        let fill: String = std::iter::repeat(DIVIDER_CHAR).take(fill_len).collect();

        let line = Line::from(vec![
            Span::styled(left, dim),
            Span::styled(fill, dim),
            Span::styled(right, dim),
        ]);

        f.render_widget(Paragraph::new(vec![line]), area);
    }

    fn render_prompt(&self, f: &mut Frame, area: Rect) {
        if self.pending_permission.is_some() {
            let line = Line::from(vec![
                Span::styled(
                    format!("{} ", PROMPT_CHAR),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    "Allow? (y)es / (n)o / (a)lways",
                    Style::default().fg(Color::Yellow),
                ),
            ]);
            f.render_widget(Paragraph::new(vec![line]), area);
        } else if self.pending_question.is_some() {
            // Show user input with cyan prompt while answering a question
            let cursor_vis = if self.frame_ticker % 10 < 5 { "\u{2588}" } else { " " };
            let line = Line::from(vec![
                Span::styled(
                    format!("{} ", PROMPT_CHAR),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::raw(self.input.clone()),
                Span::styled(cursor_vis.to_string(), Style::default().fg(Color::DarkGray)),
            ]);
            f.render_widget(Paragraph::new(vec![line]), area);
        } else {
            let cursor_vis = if self.frame_ticker % 10 < 5 {
                "\u{2588}" // full block cursor
            } else {
                " "
            };

            let line = Line::from(vec![
                Span::styled(
                    format!("{} ", PROMPT_CHAR),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(self.input.clone()),
                Span::styled(cursor_vis.to_string(), Style::default().fg(Color::DarkGray)),
            ]);
            f.render_widget(Paragraph::new(vec![line]), area);
        }
    }
}

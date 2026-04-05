//! TUI application loop — Claude Code-style terminal interface.
//!
//! Three-zone layout: scrollable conversation, status line, and input prompt.
//! Uses typed [`MessageEntry`] variants to render user, assistant, tool, and
//! permission messages with the correct visual style.
//!
//! # Module layout
//!
//! The `App` implementation is split across several files for maintainability:
//!
//! | File                  | Responsibility                                |
//! |-----------------------|-----------------------------------------------|
//! | `app.rs` (this file)  | Types, state, constructor, main event loop    |
//! | `app/render.rs`       | All rendering (conversation, status, prompt)  |
//! | `app/autocomplete.rs` | Slash command and `@file` autocomplete        |
//! | `app/commands_handler.rs` | Slash command parsing and dispatch         |
//! | `app/dialog_handler.rs`   | Dialog open/close/result lifecycle        |
//! | `app/history.rs`      | Input history up/down navigation              |

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, MouseEventKind};
use ratatui::{backend::Backend, Terminal};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

use crate::commands::CommandRegistry;
use crate::tools::ask_user::QuestionRequest;
use crate::ui::dialogs::{ActiveDialog, Dialog, DialogAction};

mod autocomplete;
mod commands_handler;
mod dialog_handler;
mod history;
mod render;

// ── Claude Code style characters ─────────────────────────────────────────
const ASSISTANT_PREFIX: &str = "  \u{23BF} "; // ⎿ (left square bracket extension)
const DOT: &str = "\u{25CF}"; // ● (filled circle)
const PROMPT_CHAR: &str = ">";
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const RAIL_FRAMES: &[char] = &['─', '╌', '┄', '╌'];
const HERO_TITLE: &str = "RUST AGENT";
const HERO_TAGLINE: &str = "AI coding companion for your terminal";
const AUTOCOMPLETE_MAX_ITEMS: usize = 5;
const FILE_SCAN_MAX: usize = 5000;
const FILE_SUGGEST_DEBOUNCE: Duration = Duration::from_millis(50);

/// Build the startup banner rendered at the beginning of a chat session.
///
/// # Returns
///
/// A `String` containing ASCII/Unicode banner text for the initial system line.
/// If `ferris_says` fails, the function falls back to a simple crab label.
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
    /// Current autocomplete candidate list.
    autocomplete_items: Vec<AutocompleteItem>,
    /// Selected item index in `autocomplete_items`.
    autocomplete_selected: usize,
    /// Last input string explicitly dismissed by autocomplete escape.
    autocomplete_dismissed_input: Option<String>,
    /// Set when input changed and autocomplete should recompute.
    autocomplete_dirty: bool,
    /// Last timestamp of file suggestion recompute for debounce.
    last_file_refresh: Option<Instant>,
    /// Cached workspace file index for file autocomplete.
    file_index: Option<Vec<String>>,
    /// Currently active dialog overlay (if any).
    active_dialog: ActiveDialog,
    /// Boxed dialog widget for the current overlay.
    dialog_widget: Option<Box<dyn Dialog>>,
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
    /// Create a fresh `App` state used by the TUI runtime.
    ///
    /// # Parameters
    ///
    /// - `tx_to_engine`: Channel sender used to forward user prompts to the engine.
    /// - `rx_from_engine`: Channel receiver used to collect UI events from the engine.
    /// - `rx_questions`: Channel receiver for interactive AskUserQuestion prompts.
    /// - `command_registry`: Registry containing all available slash commands.
    ///
    /// # Returns
    ///
    /// A fully initialized `App` with welcome text, loaded keybindings, and default
    /// UI/autocomplete/history state.
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
            active_dialog: ActiveDialog::None,
            dialog_widget: None,
        }
    }

    /// Run the main asynchronous TUI loop until exit is requested.
    ///
    /// # Parameters
    ///
    /// - `terminal`: Active ratatui terminal used to draw frames each tick.
    ///
    /// # Returns
    ///
    /// - `Ok(())`: The app exited cleanly.
    /// - `Err(anyhow::Error)`: Terminal I/O or event polling failed.
    ///
    /// # Behavior
    ///
    /// This loop handles engine events, interactive questions, keyboard/mouse input,
    /// message rendering state, autocomplete refresh, and graceful shutdown.
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
                        self.messages.push(MessageEntry::System(format!(
                            "  ↳ starting tool: {}",
                            name
                        )));
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
                        self.messages.push(MessageEntry::System(format!(
                            "  ✓ finished tool: {}",
                            name
                        )));
                        self.auto_scroll();
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
                    // Dialog overlay intercept — route all keys to the active dialog.
                    // When open, no other input handler runs.
                    if let Some(ref mut widget) = self.dialog_widget {
                        let action = widget.handle_key(key);
                        match action {
                            DialogAction::Continue => {}
                            DialogAction::Select(value) => {
                                self.handle_dialog_result(&value);
                                self.close_dialog();
                            }
                            DialogAction::Cancel => {
                                self.close_dialog();
                            }
                        }
                        continue;
                    }

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

    /// Recompute conversation scroll offset when auto-follow mode is active.
    ///
    /// # Behavior
    ///
    /// Uses estimated wrapped line counts for message history plus stream buffer,
    /// then pins the viewport to the bottom unless the user manually scrolled.
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

    /// Estimate rendered line count for one message entry.
    ///
    /// # Parameters
    ///
    /// - `entry`: Message variant whose on-screen height should be computed.
    ///
    /// # Returns
    ///
    /// Number of visual lines after accounting for prefixes and soft wrapping.
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

    /// Count wrapped visual lines for a text block with a fixed prefix width.
    ///
    /// # Parameters
    ///
    /// - `text`: Raw text that will be displayed.
    /// - `prefix_len`: Prefix width added to each line before wrapping.
    ///
    /// # Returns
    ///
    /// Total number of visual lines required for the wrapped content.
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
                count += total_chars.div_ceil(w);
            }
        }
        count.max(1)
    }

    // NOTE: The following methods have been extracted to separate files:
    //
    //   handle_slash_command()           → app/commands_handler.rs
    //   open_dialog(), close_dialog(),
    //     handle_dialog_result()         → app/dialog_handler.rs
    //   push_history_entry(),
    //     reset_history_navigation(),
    //     navigate_history_previous(),
    //     navigate_history_next()        → app/history.rs
    //   render methods (ui, etc.)        → app/render.rs
    //   autocomplete methods             → app/autocomplete.rs

}

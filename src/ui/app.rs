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
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

// ── Claude Code style characters ─────────────────────────────────────────
const ASSISTANT_PREFIX: &str = "  \u{23BF} "; // ⎿ (left square bracket extension)
const DIVIDER_CHAR: char = '\u{2500}'; // ─ (box-drawing horizontal)
const DOT: &str = "\u{25CF}"; // ● (filled circle)
const PROMPT_CHAR: &str = ">";
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

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
    bindings: Vec<crate::keybindings::ParsedBinding>,
    scroll_offset: u32,
    /// Set when streaming is active to suppress the duplicate LLMResponse.
    streamed_this_turn: bool,
    /// True when the user has manually scrolled — suppresses auto-scroll until
    /// a new message is submitted.
    user_scrolled: bool,
    pending_permission: Option<PendingPermission>,
    pub cost_tracker: Option<Arc<Mutex<crate::engine::cost_tracker::CostTracker>>>,
    /// Terminal width for divider rendering.
    term_width: u16,
    /// Conversation area height (updated each frame).
    conv_height: u16,
    /// True after user submits a query until the first response event arrives.
    waiting_for_response: bool,
}

struct PendingPermission {
    tool_name: String,
    #[allow(dead_code)]
    description: String,
    response_tx: oneshot::Sender<PermissionResponse>,
}

impl App {
    pub fn new(
        tx_to_engine: mpsc::Sender<String>,
        rx_from_engine: mpsc::Receiver<UiEvent>,
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
            bindings: load_result.bindings,
            scroll_offset: 0,
            streamed_this_turn: false,
            user_scrolled: false,
            pending_permission: None,
            cost_tracker: None,
            term_width: 80,
            conv_height: 24,
            waiting_for_response: false,
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

            if event::poll(Duration::from_millis(50))? {
                self.frame_ticker = self.frame_ticker.wrapping_add(1);
                match event::read()? {
                    Event::Key(key) => {
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
                    let active_contexts = vec![KeybindingContext::Global, KeybindingContext::Chat];
                    if let Some(action) = resolve_key(&key, &active_contexts, &self.bindings) {
                        match action {
                            KeybindingAction::AppInterrupt | KeybindingAction::AppExit => {
                                self.exit = true;
                            }
                            KeybindingAction::AppRedraw => {}
                            KeybindingAction::ChatSubmit => {
                                let submitted = self.input.trim().to_string();
                                if submitted == "quit" || submitted == "exit" {
                                    self.exit = true;
                                } else if submitted.starts_with('/') {
                                    self.handle_slash_command(&submitted);
                                    self.input.clear();
                                } else if !submitted.is_empty() {
                                    self.messages.push(MessageEntry::Divider);
                                    self.messages.push(MessageEntry::User(submitted.clone()));
                                    let _ = self.tx_to_engine.send(submitted).await;
                                    self.input.clear();
                                    self.user_scrolled = false;
                                    self.waiting_for_response = true;
                                    self.auto_scroll();
                                }
                            }
                            KeybindingAction::ChatCancel => {
                                self.input.clear();
                            }
                            KeybindingAction::HistoryPrevious | KeybindingAction::HistoryNext => {}
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Char(c) => self.input.push(c),
                            KeyCode::Backspace => {
                                self.input.pop();
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
        let command = parts.first().copied().unwrap_or("");

        match command {
            "/help" => {
                self.messages.push(MessageEntry::System(
                    "  /help  - Show this help\n  /clear - Clear conversation\n  /cost  - Show token usage and cost\n  /exit  - Exit the agent".to_string(),
                ));
            }
            "/clear" => {
                self.messages.clear();
                self.scroll_offset = 0;
                self.messages
                    .push(MessageEntry::System("  Conversation cleared.".to_string()));
            }
            "/cost" => {
                if let Some(ref tracker) = self.cost_tracker {
                    if let Ok(t) = tracker.lock() {
                        self.messages
                            .push(MessageEntry::System(format!("  {}", t.format_total_cost())));
                    }
                } else {
                    self.messages.push(MessageEntry::System(
                        "  Cost tracking not available.".to_string(),
                    ));
                }
            }
            "/exit" | "/quit" => {
                self.exit = true;
            }
            _ => {
                self.messages.push(MessageEntry::System(format!(
                    "  Unknown command: {}",
                    command
                )));
            }
        }
        self.auto_scroll();
    }

    // ── Rendering ────────────────────────────────────────────────────────

    fn ui(&mut self, f: &mut Frame) {
        let area = f.size();
        self.term_width = area.width;

        // Layout: [conversation] [status line] [prompt]
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),    // conversation
                Constraint::Length(1), // status line
                Constraint::Length(1), // prompt input
            ])
            .split(area);

        self.conv_height = chunks[0].height;
        self.render_conversation(f, chunks[0]);
        self.render_status_line(f, chunks[1]);
        self.render_prompt(f, chunks[2]);
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

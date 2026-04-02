//! TUI application loop — Claude Code-style terminal interface.
//!
//! Three-zone layout: scrollable conversation, status line, and input prompt.
//! Uses typed [`MessageEntry`] variants to render user, assistant, tool, and
//! permission messages with the correct visual style.

use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
    Terminal,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use anyhow::Result;
use tokio::sync::{mpsc, oneshot};

// ── Claude Code style characters ─────────────────────────────────────────
const ASSISTANT_PREFIX: &str = "  \u{23BF} "; // ⎿ (left square bracket extension)
const DIVIDER_CHAR: char = '\u{2500}';         // ─ (box-drawing horizontal)
const DOT: &str = "\u{25CF}";                  // ● (filled circle)
const PROMPT_CHAR: &str = ">";

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
    ToolUse { name: String, done: bool, error: bool },
    /// Error message (red)
    Error(String),
    /// System/info message (dim)
    System(String),
    /// Horizontal divider
    Divider,
    /// Permission request line
    Permission { tool_name: String, description: String },
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
    scroll_offset: u16,
    pending_permission: Option<PendingPermission>,
    pub cost_tracker: Option<Arc<Mutex<crate::engine::cost_tracker::CostTracker>>>,
    /// Terminal width for divider rendering.
    term_width: u16,
}

struct PendingPermission {
    tool_name: String,
    #[allow(dead_code)]
    description: String,
    response_tx: oneshot::Sender<PermissionResponse>,
}

impl App {
    pub fn new(tx_to_engine: mpsc::Sender<String>, rx_from_engine: mpsc::Receiver<UiEvent>) -> Self {
        let load_result = crate::keybindings::load_keybindings();
        if !load_result.warnings.is_empty() {
            tracing::warn!("Keybinding warnings: {:?}", load_result.warnings);
        }
        Self {
            input: String::new(),
            messages: vec![],
            exit: false,
            running_tool: None,
            frame_ticker: 0,
            current_stream: None,
            tx_to_engine,
            rx_from_engine,
            bindings: load_result.bindings,
            scroll_offset: 0,
            pending_permission: None,
            cost_tracker: None,
            term_width: 80,
        }
    }

    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        while !self.exit {
            terminal.draw(|f| {
                self.term_width = f.size().width;
                self.ui(f);
            })?;

            while let Ok(event) = self.rx_from_engine.try_recv() {
                match event {
                    UiEvent::LLMResponse(res) => {
                        self.messages.push(MessageEntry::Assistant(res));
                        self.auto_scroll();
                    }
                    UiEvent::LLMError(err) => {
                        self.messages.push(MessageEntry::Error(err));
                        self.auto_scroll();
                    }
                    UiEvent::ToolStarted(name) => {
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
                        self.current_stream = Some(String::new());
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
                if let Event::Key(key) = event::read()? {
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
                    let active_contexts =
                        vec![KeybindingContext::Global, KeybindingContext::Chat];
                    if let Some(action) =
                        resolve_key(&key, &active_contexts, &self.bindings)
                    {
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
                                    self.messages
                                        .push(MessageEntry::User(submitted.clone()));
                                    let _ = self.tx_to_engine.send(submitted).await;
                                    self.input.clear();
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
                            _ => {}
                        }
                    }
                }
            } else {
                self.frame_ticker = self.frame_ticker.wrapping_add(1);
            }
        }
        Ok(())
    }

    fn auto_scroll(&mut self) {
        let total_lines: usize = self
            .messages
            .iter()
            .map(|m| self.entry_line_count(m))
            .sum();
        let stream_lines = self
            .current_stream
            .as_ref()
            .map(|s| s.lines().count().max(1) + 1)
            .unwrap_or(0);
        self.scroll_offset = (total_lines + stream_lines).saturating_sub(10) as u16;
    }

    fn entry_line_count(&self, entry: &MessageEntry) -> usize {
        match entry {
            MessageEntry::User(t) | MessageEntry::Assistant(t) | MessageEntry::Error(t) => {
                t.lines().count().max(1)
            }
            MessageEntry::System(t) => t.lines().count().max(1),
            MessageEntry::ToolUse { .. } => 1,
            MessageEntry::Divider => 1,
            MessageEntry::Permission { .. } => 2,
        }
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

    fn ui(&self, f: &mut Frame) {
        let area = f.size();

        // Layout: [conversation] [status line] [prompt]
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),    // conversation
                Constraint::Length(1), // status line
                Constraint::Length(1), // prompt input
            ])
            .split(area);

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
                                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
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
                    let (dot_style, name_style) = if *error {
                        (
                            Style::default().fg(Color::Red),
                            Style::default().fg(Color::Red),
                        )
                    } else if *done {
                        (
                            Style::default().fg(Color::Green),
                            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                        )
                    } else {
                        // blinking dim dot for in-progress
                        let vis = if self.frame_ticker % 10 < 5 {
                            Style::default().fg(Color::DarkGray)
                        } else {
                            Style::default().fg(Color::Black)
                        };
                        (vis, Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
                    };

                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(DOT, dot_style),
                        Span::raw(" "),
                        Span::styled(name.clone(), name_style),
                    ]));
                }
                MessageEntry::Error(text) => {
                    for line in text.lines() {
                        lines.push(Line::from(vec![
                            Span::styled(ASSISTANT_PREFIX.to_string(), dim),
                            Span::styled(
                                line.to_string(),
                                Style::default().fg(Color::Red),
                            ),
                        ]));
                    }
                }
                MessageEntry::System(text) => {
                    for line in text.lines() {
                        lines.push(Line::from(Span::styled(line.to_string(), dim)));
                    }
                }
                MessageEntry::Divider => {
                    let divider: String =
                        std::iter::repeat(DIVIDER_CHAR).take(w.min(80)).collect();
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
            }
            // Blinking cursor
            let cursor = if self.frame_ticker % 10 < 5 {
                "\u{2588}" // full block
            } else {
                " "
            };
            let last_idx = lines.len().saturating_sub(1);
            if let Some(last) = lines.get_mut(last_idx) {
                last.spans.push(Span::styled(
                    cursor.to_string(),
                    Style::default().fg(Color::DarkGray),
                ));
            } else {
                lines.push(Line::from(Span::styled(
                    cursor.to_string(),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        let para = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));

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
                Span::styled(
                    cursor_vis.to_string(),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            f.render_widget(Paragraph::new(vec![line]), area);
        }
    }
}

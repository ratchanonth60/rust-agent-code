use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
    Terminal,
};
use std::time::Duration;
use anyhow::Result;
use tokio::sync::{mpsc, oneshot};

/// Events sent from the engine background task to the TUI for rendering.
pub enum UiEvent {
    /// The LLM produced a final text response (non-streaming fallback).
    LLMResponse(String),
    /// The LLM call failed with an error message.
    LLMError(String),
    /// A tool execution has started (carries the tool name).
    ToolStarted(String),
    /// A tool execution has finished (carries the tool name).
    ToolFinished(String),
    /// A streaming text delta arrived from the LLM.
    StreamDelta(String),
    /// Streaming has started for a new response.
    StreamStart,
    /// Streaming has finished — the current_stream is finalized.
    StreamEnd,
    /// Permission prompt: engine asks the user to allow/deny a tool.
    PermissionRequest {
        tool_name: String,
        description: String,
        response_tx: oneshot::Sender<PermissionResponse>,
    },
}

/// The user's response to a permission prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionResponse {
    /// Allow this one invocation.
    Allow,
    /// Deny this one invocation.
    Deny,
    /// Allow all future invocations of this tool (for this session).
    AlwaysAllow,
}

/// The interactive TUI application state, managing input, messages, and channel I/O.
pub struct App {
    pub input: String,
    pub messages: Vec<String>,
    pub exit: bool,
    pub running_tool: Option<String>,
    pub frame_ticker: usize,
    /// Accumulates streaming text from the LLM. `None` when not streaming.
    pub current_stream: Option<String>,
    tx_to_engine: mpsc::Sender<String>,
    rx_from_engine: mpsc::Receiver<UiEvent>,
    /// Loaded keybindings (defaults + user overrides).
    bindings: Vec<crate::keybindings::ParsedBinding>,
    /// Auto-scroll offset (number of lines to skip from top).
    scroll_offset: u16,
    /// Pending permission prompt awaiting user input (Y/n/a).
    pending_permission: Option<PendingPermission>,
}

/// State for an active permission prompt.
struct PendingPermission {
    tool_name: String,
    description: String,
    response_tx: oneshot::Sender<PermissionResponse>,
}

impl App {
    /// Creates a new App with the given engine communication channels.
    pub fn new(tx_to_engine: mpsc::Sender<String>, rx_from_engine: mpsc::Receiver<UiEvent>) -> Self {
        let load_result = crate::keybindings::load_keybindings();
        if !load_result.warnings.is_empty() {
            tracing::warn!("Keybinding warnings: {:?}", load_result.warnings);
        }
        Self {
            input: String::new(),
            messages: vec![
                "Welcome to the Rust AI Agent.".to_string(),
                "Type your query clearly. Press Ctrl+C or type 'quit' to exit.".to_string(),
            ],
            exit: false,
            running_tool: None,
            frame_ticker: 0,
            current_stream: None,
            tx_to_engine,
            rx_from_engine,
            bindings: load_result.bindings,
            scroll_offset: 0,
            pending_permission: None,
        }
    }

    /// Runs the main TUI event loop: draws frames, polls engine events, and handles keyboard input.
    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        while !self.exit {
            // Draw UI
            terminal.draw(|f| self.ui(f))?;

            // Process async events from the LLM Engine (non-blocking)
            while let Ok(event) = self.rx_from_engine.try_recv() {
                match event {
                    UiEvent::LLMResponse(res) => {
                        self.messages.push(format!("Agent: {}", res));
                        self.auto_scroll();
                    }
                    UiEvent::LLMError(err) => {
                        self.messages.push(format!("Error: {}", err));
                        self.auto_scroll();
                    }
                    UiEvent::ToolStarted(name) => self.running_tool = Some(name),
                    UiEvent::ToolFinished(_) => self.running_tool = None,
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
                                self.messages.push(format!("Agent: {}", stream));
                            }
                        }
                        self.auto_scroll();
                    }
                    UiEvent::PermissionRequest { tool_name, description, response_tx } => {
                        self.messages.push(format!(
                            "[Permission] {} — {} (Y/n/a)",
                            tool_name, description
                        ));
                        self.pending_permission = Some(PendingPermission {
                            tool_name,
                            description,
                            response_tx,
                        });
                        self.auto_scroll();
                    }
                }
            }

            // Process terminal input events (non-blocking interval 50ms for smoother streaming)
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    // If a permission prompt is pending, intercept Y/n/a keys
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
                            self.messages.push(format!(
                                "[Permission] {} → {:?}",
                                perm.tool_name, resp
                            ));
                            let _ = perm.response_tx.send(resp);
                            self.auto_scroll();
                            continue; // skip normal key handling
                        }
                        // Ignore other keys while permission prompt is active
                        continue;
                    }

                    // Route via keybinding system
                    use crate::keybindings::{resolve_key, KeybindingAction, KeybindingContext};
                    let active_contexts = vec![
                        KeybindingContext::Global,
                        KeybindingContext::Chat,
                    ];
                    if let Some(action) = resolve_key(
                        &key,
                        &active_contexts,
                        &self.bindings,
                    ) {
                        match action {
                            KeybindingAction::AppInterrupt | KeybindingAction::AppExit => {
                                self.exit = true;
                            }
                            KeybindingAction::AppRedraw => {
                                // Handled automatically by terminal.draw above
                            }
                            KeybindingAction::ChatSubmit => {
                                let submitted = self.input.trim().to_string();
                                if submitted == "quit" || submitted == "exit" {
                                    self.exit = true;
                                } else if !submitted.is_empty() {
                                    self.messages.push(format!("You: {}", submitted));

                                    // Send query to engine thread
                                    let _ = self.tx_to_engine.send(submitted).await;
                                    self.input.clear();
                                    self.auto_scroll();
                                }
                            }
                            KeybindingAction::ChatCancel => {
                                self.input.clear();
                            }
                            KeybindingAction::HistoryPrevious => {
                                // TODO: Command history
                            }
                            KeybindingAction::HistoryNext => {
                                // TODO: Command history
                            }
                            _ => {
                                // Other actions not yet handled
                            }
                        }
                    } else {
                        // Fallback text input
                        match key.code {
                            KeyCode::Char(c) => {
                                self.input.push(c);
                            }
                            KeyCode::Backspace => {
                                self.input.pop();
                            }
                            _ => {}
                        }
                    }
                }
            } else {
                // Render tick
                self.frame_ticker = self.frame_ticker.wrapping_add(1);
            }
        }
        Ok(())
    }

    /// Auto-scroll to show the latest content.
    fn auto_scroll(&mut self) {
        let total_lines: usize = self.messages.iter().map(|m| m.lines().count().max(1)).sum();
        if let Some(ref stream) = self.current_stream {
            let stream_lines = stream.lines().count().max(1);
            self.scroll_offset = (total_lines + stream_lines).saturating_sub(10) as u16;
        } else {
            self.scroll_offset = total_lines.saturating_sub(10) as u16;
        }
    }

    /// Renders the two-panel layout: scrollable conversation history on top, input prompt on bottom.
    fn ui(&self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Min(5),    // Messages area
                Constraint::Length(3), // Input area
            ])
            .split(f.size());

        // Build display lines: finalized messages + streaming text
        let mut lines: Vec<Line> = Vec::new();
        for msg in &self.messages {
            for line in msg.lines() {
                lines.push(Line::from(Span::raw(line.to_string())));
            }
        }

        // Show streaming text with a cursor indicator
        if let Some(ref stream) = self.current_stream {
            if !stream.is_empty() {
                for line in stream.lines() {
                    lines.push(Line::from(vec![
                        Span::styled("Agent: ", Style::default().fg(Color::Green)),
                        Span::raw(line.to_string()),
                    ]));
                }
            }
            // Blinking cursor at end
            let cursor = if self.frame_ticker % 10 < 5 { "▌" } else { " " };
            lines.push(Line::from(Span::styled(cursor, Style::default().fg(Color::Green))));
        }

        let messages_block = Paragraph::new(lines)
            .block(Block::default().title(" Conversation ").borders(Borders::ALL))
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));

        f.render_widget(messages_block, chunks[0]);

        // Command Input UI
        let mut input_display = self.input.clone();
        if self.pending_permission.is_some() {
            input_display = "Press Y to allow, N to deny, A to always allow".to_string();
        } else if let Some(ref tool) = self.running_tool {
            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let spinner = frames[self.frame_ticker % frames.len()];
            input_display = format!("{}  [ {} {} ]", input_display, spinner, tool);
        }

        let input_color = if self.pending_permission.is_some() {
            Color::Red
        } else if self.running_tool.is_some() || self.current_stream.is_some() {
            Color::Cyan
        } else {
            Color::Yellow
        };

        let input_text = Paragraph::new(input_display.as_str())
            .style(Style::default().fg(input_color))
            .block(Block::default().title(" > ").borders(Borders::ALL));

        f.render_widget(input_text, chunks[1]);
    }
}

use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, Paragraph},
    Frame,
    Terminal,
};
use std::time::Duration;
use anyhow::Result;
use tokio::sync::mpsc;

/// Events sent from the engine background task to the TUI for rendering.
pub enum UiEvent {
    /// The LLM produced a final text response.
    LLMResponse(String),
    /// The LLM call failed with an error message.
    LLMError(String),
    /// A tool execution has started (carries the tool name).
    ToolStarted(String),
    /// A tool execution has finished (carries the tool name).
    ToolFinished(String),
}

/// The interactive TUI application state, managing input, messages, and channel I/O.
pub struct App {
    pub input: String,
    pub messages: Vec<String>,
    pub exit: bool,
    pub running_tool: Option<String>,
    pub frame_ticker: usize,
    tx_to_engine: mpsc::Sender<String>,
    rx_from_engine: mpsc::Receiver<UiEvent>,
    /// Loaded keybindings (defaults + user overrides).
    bindings: Vec<crate::keybindings::ParsedBinding>,
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
            tx_to_engine,
            rx_from_engine,
            bindings: load_result.bindings,
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
                    UiEvent::LLMResponse(res) => self.messages.push(format!("🤖 Agent: {}", res)),
                    UiEvent::LLMError(err) => self.messages.push(format!("❌ Error: {}", err)),
                    UiEvent::ToolStarted(name) => self.running_tool = Some(name),
                    UiEvent::ToolFinished(_) => self.running_tool = None,
                }
            }

            // Process terminal input events (non-blocking interval 100ms)
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
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
                                    self.messages.push(format!("🧑 You: {}", submitted));

                                    // Send query to engine thread
                                    let _ = self.tx_to_engine.send(submitted).await;
                                    self.input.clear();
                                }
                            }
                            KeybindingAction::ChatCancel => {
                                self.input.clear(); // Clear input
                            }
                            KeybindingAction::HistoryPrevious => {
                                // Previous command history
                            }
                            KeybindingAction::HistoryNext => {
                                // Next command history
                            }
                            _ => {
                                // Other actions not yet handled in the TUI
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
                // If poll timed out, it's a render tick
                self.frame_ticker = self.frame_ticker.wrapping_add(1);
            }
        }
        Ok(())
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

        // Chat History UI
        let messages_text: Vec<_> = self.messages
            .iter()
            .map(|m| ratatui::text::Line::from(Span::raw(m)))
            .collect();
            
        let messages_block = Paragraph::new(messages_text)
            .block(Block::default().title(" Conversation History ").borders(Borders::ALL));
            
        f.render_widget(messages_block, chunks[0]);

        // Command Input UI
        let mut input_display = self.input.clone();
        if let Some(ref tool) = self.running_tool {
            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let spinner = frames[self.frame_ticker % frames.len()];
            input_display = format!("{}  [ {} Running Background Tool: {} ]", input_display, spinner, tool);
        }

        let input_text = Paragraph::new(input_display.as_str())
            .style(Style::default().fg(if self.running_tool.is_some() { Color::Cyan } else { Color::Yellow }))
            .block(Block::default().title(" Prompt (Type 'quit' or Ctrl+C to exit) ").borders(Borders::ALL));
        
        f.render_widget(input_text, chunks[1]);
    }
}

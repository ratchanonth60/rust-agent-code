use crossterm::event::{self, Event, KeyCode, KeyModifiers};
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

pub enum UiEvent {
    LLMResponse(String),
    LLMError(String),
}

pub struct App {
    pub input: String,
    pub messages: Vec<String>,
    pub exit: bool,
    tx_to_engine: mpsc::Sender<String>,
    rx_from_engine: mpsc::Receiver<UiEvent>,
}

impl App {
    pub fn new(tx_to_engine: mpsc::Sender<String>, rx_from_engine: mpsc::Receiver<UiEvent>) -> Self {
        Self {
            input: String::new(),
            messages: vec![
                "Welcome to the Rust AI Agent.".to_string(),
                "Type your query clearly. Press Ctrl+C or type 'quit' to exit.".to_string(),
            ],
            exit: false,
            tx_to_engine,
            rx_from_engine,
        }
    }

    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        while !self.exit {
            // Draw UI
            terminal.draw(|f| self.ui(f))?;

            // Process async events from the LLM Engine (non-blocking)
            while let Ok(event) = self.rx_from_engine.try_recv() {
                match event {
                    UiEvent::LLMResponse(res) => self.messages.push(format!("🤖 Agent: {}", res)),
                    UiEvent::LLMError(err) => self.messages.push(format!("❌ Error: {}", err)),
                }
            }

            // Process terminal input events (non-blocking interval 100ms)
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.exit = true;
                        }
                        KeyCode::Enter => {
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
        }
        Ok(())
    }

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
        let input_text = Paragraph::new(self.input.as_str())
            .style(Style::default().fg(Color::Yellow))
            .block(Block::default().title(" Prompt (Type 'quit' or Ctrl+C to exit) ").borders(Borders::ALL));
        
        f.render_widget(input_text, chunks[1]);
    }
}

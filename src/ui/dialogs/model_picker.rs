//! Model picker dialog — select an LLM model from a list.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use super::{centered_rect, Dialog, DialogAction};

/// Known model names grouped by provider.
const MODELS: &[(&str, &[&str])] = &[
    ("Claude", &[
        "claude-opus-4-6",
        "claude-sonnet-4-6",
        "claude-haiku-4-5-20251001",
    ]),
    ("OpenAI", &[
        "gpt-4o",
        "gpt-4o-mini",
        "o3-mini",
    ]),
    ("Gemini", &[
        "gemini-2.5-flash",
        "gemini-2.5-pro",
    ]),
];

/// Interactive model picker dialog.
pub struct ModelPickerDialog {
    /// Flat list of all model names.
    items: Vec<String>,
    /// Currently highlighted index.
    selected: usize,
}

impl ModelPickerDialog {
    /// Create a new model picker with default model list.
    pub fn new() -> Self {
        let items: Vec<String> = MODELS
            .iter()
            .flat_map(|(_, models)| models.iter().map(|m| m.to_string()))
            .collect();
        Self { items, selected: 0 }
    }
}

impl Default for ModelPickerDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl Dialog for ModelPickerDialog {
    fn title(&self) -> &str {
        "Select Model"
    }

    fn handle_key(&mut self, key: KeyEvent) -> DialogAction {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                DialogAction::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < self.items.len() {
                    self.selected += 1;
                }
                DialogAction::Continue
            }
            KeyCode::Enter => {
                if let Some(model) = self.items.get(self.selected) {
                    DialogAction::Select(model.clone())
                } else {
                    DialogAction::Cancel
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => DialogAction::Cancel,
            _ => DialogAction::Continue,
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let width = 40u16;
        let height = (self.items.len() as u16 + 4).min(area.height);
        let rect = centered_rect(width, height, area);

        // Clear the area behind the dialog
        f.render_widget(Clear, rect);

        let mut lines: Vec<Line> = Vec::new();
        let mut provider_idx = 0;
        let mut item_idx = 0;

        for (provider, models) in MODELS {
            if provider_idx > 0 {
                lines.push(Line::from(""));
            }
            lines.push(Line::from(Span::styled(
                format!("  {}", provider),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            for model in *models {
                let is_selected = item_idx == self.selected;
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let prefix = if is_selected { " ▸ " } else { "   " };
                lines.push(Line::from(Span::styled(
                    format!("{}{}", prefix, model),
                    style,
                )));
                item_idx += 1;
            }
            provider_idx += 1;
        }

        let block = Block::default()
            .title(" Select Model (↑/↓ Enter Esc) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let para = Paragraph::new(lines).block(block);
        f.render_widget(para, rect);
    }
}

//! Theme picker dialog — select a color theme.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use super::{centered_rect, Dialog, DialogAction};

/// Interactive theme picker dialog.
pub struct ThemePickerDialog {
    /// Available theme names.
    items: Vec<String>,
    /// Currently highlighted index.
    selected: usize,
}

impl ThemePickerDialog {
    /// Create a new theme picker, loading available output styles.
    pub fn new() -> Self {
        let mut items = vec!["default".to_string()];

        // Load custom output styles
        let styles = crate::output_styles::load_output_styles();
        for style in &styles {
            items.push(style.name.clone());
        }

        Self { items, selected: 0 }
    }
}

impl Default for ThemePickerDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl Dialog for ThemePickerDialog {
    fn title(&self) -> &str {
        "Select Theme"
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
                if let Some(theme) = self.items.get(self.selected) {
                    DialogAction::Select(theme.clone())
                } else {
                    DialogAction::Cancel
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => DialogAction::Cancel,
            _ => DialogAction::Continue,
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let width = 35u16;
        let height = (self.items.len() as u16 + 3).min(area.height);
        let rect = centered_rect(width, height, area);

        f.render_widget(Clear, rect);

        let lines: Vec<Line> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let is_selected = i == self.selected;
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let prefix = if is_selected { " ▸ " } else { "   " };
                Line::from(Span::styled(format!("{}{}", prefix, name), style))
            })
            .collect();

        let block = Block::default()
            .title(" Select Theme (↑/↓ Enter Esc) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta));

        let para = Paragraph::new(lines).block(block);
        f.render_widget(para, rect);
    }
}

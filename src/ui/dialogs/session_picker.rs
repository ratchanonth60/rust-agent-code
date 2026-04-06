//! Session picker dialog — select a session to resume from a scrollable list.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::engine::session::{Session, SessionSummary};

use super::{centered_rect, Dialog, DialogAction};

/// Interactive session picker dialog.
pub struct SessionPickerDialog {
    /// Available sessions, newest first.
    sessions: Vec<SessionSummary>,
    /// Currently highlighted index.
    selected: usize,
    /// Scroll offset for the visible viewport.
    scroll_offset: usize,
}

impl SessionPickerDialog {
    /// Create a new session picker, loading sessions from disk.
    pub fn new() -> Self {
        let sessions = Session::list_sessions().unwrap_or_default();
        Self {
            sessions,
            selected: 0,
            scroll_offset: 0,
        }
    }
}

impl Dialog for SessionPickerDialog {
    fn title(&self) -> &str {
        "Resume Session"
    }

    fn handle_key(&mut self, key: KeyEvent) -> DialogAction {
        if self.sessions.is_empty() {
            return match key.code {
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => DialogAction::Cancel,
                _ => DialogAction::Continue,
            };
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                // Scroll adjustment happens at render time when we know viewport size
                DialogAction::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < self.sessions.len() {
                    self.selected += 1;
                }
                DialogAction::Continue
            }
            KeyCode::Enter => {
                DialogAction::Select(self.sessions[self.selected].id.clone())
            }
            KeyCode::Esc | KeyCode::Char('q') => DialogAction::Cancel,
            _ => DialogAction::Continue,
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let width = 72u16;

        if self.sessions.is_empty() {
            // Empty state
            let height = 5u16;
            let rect = centered_rect(width, height, area);
            f.render_widget(Clear, rect);

            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No saved sessions.",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
            ];
            let block = Block::default()
                .title(" Resume Session (Esc) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));
            f.render_widget(Paragraph::new(lines).block(block), rect);
            return;
        }

        // Compute visible area
        let max_rows = (area.height as usize).saturating_sub(6);
        let visible_rows = max_rows.min(self.sessions.len());
        let height = (visible_rows as u16 + 4).min(area.height);
        let rect = centered_rect(width, height, area);
        f.render_widget(Clear, rect);

        // Adjust scroll for current selection (we need interior mutability workaround)
        let scroll_offset = if self.selected < self.scroll_offset {
            self.selected
        } else if visible_rows > 0 && self.selected >= self.scroll_offset + visible_rows {
            self.selected - visible_rows + 1
        } else {
            self.scroll_offset
        };

        // Build visible rows
        let inner_width = (width as usize).saturating_sub(4); // borders + padding
        let mut lines: Vec<Line> = Vec::new();

        // Scroll-up indicator
        if scroll_offset > 0 {
            lines.push(Line::from(Span::styled(
                "  \u{25b4} more above",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let end = (scroll_offset + visible_rows).min(self.sessions.len());
        for (i, s) in self.sessions[scroll_offset..end].iter().enumerate() {
            let abs_idx = scroll_offset + i;
            let is_selected = abs_idx == self.selected;

            let short_id = &s.id[..8.min(s.id.len())];
            let time = format_relative_time(s.created_at);
            let cwd_short = shorten_path(&s.cwd, 20);
            let row = format!(
                " {} {:<8}  {:<8}  {:<20}  {:>3} msgs  {}",
                if is_selected { "\u{25b8}" } else { " " },
                short_id,
                time,
                truncate_str(&s.model, 20),
                s.message_count,
                cwd_short,
            );
            // Truncate to inner width
            let row = if row.len() > inner_width {
                format!("{}...", &row[..inner_width.saturating_sub(3)])
            } else {
                row
            };

            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(row, style)));
        }

        // Scroll-down indicator
        if end < self.sessions.len() {
            lines.push(Line::from(Span::styled(
                "  \u{25be} more below",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let block = Block::default()
            .title(format!(
                " Resume Session ({}/{}) \u{2191}/\u{2193} Enter Esc ",
                self.selected + 1,
                self.sessions.len()
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        f.render_widget(Paragraph::new(lines).block(block), rect);
    }
}

/// Format a unix timestamp into a relative time string.
fn format_relative_time(ts: u64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    let dt = UNIX_EPOCH + Duration::from_secs(ts);
    match dt.elapsed() {
        Ok(elapsed) => {
            let secs = elapsed.as_secs();
            if secs < 60 {
                "just now".to_string()
            } else if secs < 3600 {
                format!("{}m ago", secs / 60)
            } else if secs < 86400 {
                format!("{}h ago", secs / 3600)
            } else {
                format!("{}d ago", secs / 86400)
            }
        }
        Err(_) => "future".to_string(),
    }
}

/// Shorten a path for display, keeping the last `max_len` characters.
fn shorten_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        path.to_string()
    } else {
        format!("...{}", &path[path.len() - max_len + 3..])
    }
}

/// Truncate a string to `max_len`, adding "..." if needed.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

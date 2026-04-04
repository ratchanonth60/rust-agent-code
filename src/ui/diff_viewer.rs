//! Diff viewer — renders unified diffs with color-coded additions and deletions.
//!
//! Produces styled [`ratatui::text::Line`] values where additions are green,
//! deletions are red, and header/context lines are dimmed.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Render a unified diff string into colored ratatui `Line`s.
pub fn render_diff(diff_text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for raw_line in diff_text.lines() {
        let line = if raw_line.starts_with("+++") || raw_line.starts_with("---") {
            // File header
            Line::from(Span::styled(
                raw_line.to_string(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ))
        } else if raw_line.starts_with("@@") {
            // Hunk header
            Line::from(Span::styled(
                raw_line.to_string(),
                Style::default().fg(Color::Cyan),
            ))
        } else if raw_line.starts_with("diff ") {
            // Diff command line
            Line::from(Span::styled(
                raw_line.to_string(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ))
        } else if raw_line.starts_with('+') {
            // Addition
            Line::from(Span::styled(
                raw_line.to_string(),
                Style::default().fg(Color::Green),
            ))
        } else if raw_line.starts_with('-') {
            // Deletion
            Line::from(Span::styled(
                raw_line.to_string(),
                Style::default().fg(Color::Red),
            ))
        } else {
            // Context line
            Line::from(Span::styled(
                raw_line.to_string(),
                Style::default().fg(Color::DarkGray),
            ))
        };

        lines.push(line);
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_additions_in_green() {
        let diff = "+added line";
        let lines = render_diff(diff);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Green));
    }

    #[test]
    fn renders_deletions_in_red() {
        let diff = "-removed line";
        let lines = render_diff(diff);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Red));
    }

    #[test]
    fn renders_hunk_headers_in_cyan() {
        let diff = "@@ -1,3 +1,4 @@";
        let lines = render_diff(diff);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Cyan));
    }
}

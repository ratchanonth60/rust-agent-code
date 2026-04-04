//! Markdown → ratatui `Line` renderer.
//!
//! Parses markdown text using [`pulldown_cmark`] and converts it into
//! styled [`ratatui::text::Line`] values suitable for the TUI conversation view.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Render a markdown string into a list of styled ratatui `Line`s.
///
/// Handles: headings, bold, italic, code spans, code blocks, lists, and block quotes.
/// Code blocks are collected and can optionally be syntax highlighted via
/// [`crate::ui::highlight::highlight_code`].
pub fn render_markdown(text: &str, _width: usize) -> Vec<Line<'static>> {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(text, options);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut in_code_block = false;
    let mut code_block_buf = String::new();
    let mut code_lang = String::new();
    let mut list_depth: usize = 0;

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    flush_line(&mut current_spans, &mut lines);
                    let heading_style = Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD);
                    let prefix = "#".repeat(level as usize);
                    current_spans.push(Span::styled(
                        format!("{} ", prefix),
                        heading_style,
                    ));
                    style_stack.push(heading_style);
                }
                Tag::Strong => {
                    let bold = current_style(&style_stack).add_modifier(Modifier::BOLD);
                    style_stack.push(bold);
                }
                Tag::Emphasis => {
                    let italic = current_style(&style_stack).add_modifier(Modifier::ITALIC);
                    style_stack.push(italic);
                }
                Tag::CodeBlock(kind) => {
                    flush_line(&mut current_spans, &mut lines);
                    in_code_block = true;
                    code_block_buf.clear();
                    code_lang = match kind {
                        pulldown_cmark::CodeBlockKind::Fenced(lang) => lang.to_string(),
                        _ => String::new(),
                    };
                }
                Tag::List(_) => {
                    list_depth += 1;
                }
                Tag::Item => {
                    flush_line(&mut current_spans, &mut lines);
                    let indent = "  ".repeat(list_depth);
                    current_spans.push(Span::styled(
                        format!("{}• ", indent),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                Tag::BlockQuote(_) => {
                    flush_line(&mut current_spans, &mut lines);
                    current_spans.push(Span::styled(
                        "│ ".to_string(),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    style_stack.pop();
                    flush_line(&mut current_spans, &mut lines);
                }
                TagEnd::Strong | TagEnd::Emphasis => {
                    style_stack.pop();
                }
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    // Render code block lines with code styling
                    let code_style = Style::default().fg(Color::Green);
                    for code_line in code_block_buf.lines() {
                        lines.push(Line::from(vec![
                            Span::styled("  ", Style::default()),
                            Span::styled(code_line.to_string(), code_style),
                        ]));
                    }
                    code_block_buf.clear();
                    code_lang.clear();
                }
                TagEnd::List(_) => {
                    list_depth = list_depth.saturating_sub(1);
                }
                TagEnd::Item => {
                    flush_line(&mut current_spans, &mut lines);
                }
                TagEnd::BlockQuote(_) => {
                    flush_line(&mut current_spans, &mut lines);
                }
                TagEnd::Paragraph => {
                    flush_line(&mut current_spans, &mut lines);
                    lines.push(Line::from(""));
                }
                _ => {}
            },
            Event::Text(text) => {
                if in_code_block {
                    code_block_buf.push_str(&text);
                } else {
                    current_spans.push(Span::styled(
                        text.to_string(),
                        current_style(&style_stack),
                    ));
                }
            }
            Event::Code(code) => {
                current_spans.push(Span::styled(
                    format!("`{}`", code),
                    Style::default().fg(Color::Yellow),
                ));
            }
            Event::SoftBreak | Event::HardBreak => {
                flush_line(&mut current_spans, &mut lines);
            }
            Event::Rule => {
                flush_line(&mut current_spans, &mut lines);
                lines.push(Line::from(Span::styled(
                    "────────────────────".to_string(),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            _ => {}
        }
    }

    flush_line(&mut current_spans, &mut lines);
    lines
}

/// Get the current style from the stack.
fn current_style(stack: &[Style]) -> Style {
    stack.last().copied().unwrap_or_default()
}

/// Flush accumulated spans into a line.
fn flush_line(spans: &mut Vec<Span<'static>>, lines: &mut Vec<Line<'static>>) {
    if !spans.is_empty() {
        lines.push(Line::from(spans.drain(..).collect::<Vec<_>>()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_plain_text() {
        let lines = render_markdown("Hello world", 80);
        assert!(!lines.is_empty());
    }

    #[test]
    fn renders_heading() {
        let lines = render_markdown("# Title", 80);
        assert!(!lines.is_empty());
        // Should contain styled "# Title"
        let text: String = lines[0].spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("Title"));
    }

    #[test]
    fn renders_code_block() {
        let input = "```rust\nfn main() {}\n```";
        let lines = render_markdown(input, 80);
        let all_text: String = lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(all_text.contains("fn main()"));
    }
}

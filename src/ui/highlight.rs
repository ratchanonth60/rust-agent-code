//! Syntax highlighting for code blocks using [`syntect`].
//!
//! Converts source code into styled [`ratatui::text::Line`] values
//! with colors derived from a [`syntect`] theme.

use std::sync::LazyLock;

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// Cached syntax definitions — loaded once on first use.
static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
/// Cached highlight themes — loaded once on first use.
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// Highlight a code string and return styled ratatui `Line`s.
///
/// Falls back to plain (green) rendering if the language is unrecognised
/// or if syntect encounters an error.
pub fn highlight_code(code: &str, language: &str) -> Vec<Line<'static>> {
    let ss = &*SYNTAX_SET;
    let ts = &*THEME_SET;
    let theme = ts.themes.get("base16-ocean.dark").unwrap_or_else(|| {
        ts.themes.values().next().expect("No default themes")
    });

    let syntax = ss
        .find_syntax_by_token(language)
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let mut h = HighlightLines::new(syntax, theme);
    let mut lines = Vec::new();

    for line in LinesWithEndings::from(code) {
        match h.highlight_line(line, ss) {
            Ok(ranges) => {
                let spans: Vec<Span<'static>> = ranges
                    .iter()
                    .map(|(style, text)| {
                        let fg = Color::Rgb(
                            style.foreground.r,
                            style.foreground.g,
                            style.foreground.b,
                        );
                        Span::styled(
                            text.trim_end_matches('\n').to_string(),
                            Style::default().fg(fg),
                        )
                    })
                    .collect();
                lines.push(Line::from(spans));
            }
            Err(_) => {
                // Fallback: plain green
                lines.push(Line::from(Span::styled(
                    line.trim_end_matches('\n').to_string(),
                    Style::default().fg(Color::Green),
                )));
            }
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_rust_code() {
        let code = "fn main() {\n    println!(\"hello\");\n}";
        let lines = highlight_code(code, "rs");
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn highlight_unknown_language() {
        let code = "some text";
        let lines = highlight_code(code, "zzzunknown");
        assert!(!lines.is_empty());
    }
}

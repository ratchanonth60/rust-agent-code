use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

use super::{
    App, AutocompleteKind, MessageEntry, ASSISTANT_PREFIX, AUTOCOMPLETE_MAX_ITEMS,
    HERO_TAGLINE, HERO_TITLE, PROMPT_CHAR, RAIL_FRAMES, SPINNER_FRAMES,
};

impl App {
    /// Draw one complete frame for the terminal UI.
    ///
    /// # Parameters
    ///
    /// - `f`: Ratatui frame to render into for the current tick.
    ///
    /// # Behavior
    ///
    /// Splits the screen into conversation/status/autocomplete/prompt regions,
    /// then delegates rendering to specialized helpers.
    pub(super) fn ui(&mut self, f: &mut Frame) {
        let area = f.size();
        self.term_width = area.width;

        let autocomplete_height = if self.is_autocomplete_visible() {
            self.autocomplete_items.len().min(AUTOCOMPLETE_MAX_ITEMS) as u16
        } else {
            0
        };

        // Layout: [conversation] [status line] [autocomplete] [prompt]
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),    // conversation
                Constraint::Length(1), // status line
                Constraint::Length(autocomplete_height), // autocomplete
                Constraint::Length(1), // prompt input
            ])
            .split(area);

        self.conv_height = chunks[0].height;
        self.render_conversation(f, chunks[0]);
        self.render_status_line(f, chunks[1]);
        if autocomplete_height > 0 {
            self.render_autocomplete(f, chunks[2]);
        }
        self.render_prompt(f, chunks[3]);

        // Dialog overlay — rendered on top of everything
        if let Some(ref dialog) = self.dialog_widget {
            dialog.render(f, area);
        }
    }

    /// Render the autocomplete dropdown list.
    ///
    /// # Parameters
    ///
    /// - `f`: Ratatui frame for drawing.
    /// - `area`: Rectangular region allocated for autocomplete.
    fn render_autocomplete(&self, f: &mut Frame, area: Rect) {
        let mut lines: Vec<Line> = Vec::new();
        for (idx, item) in self
            .autocomplete_items
            .iter()
            .take(AUTOCOMPLETE_MAX_ITEMS)
            .enumerate()
        {
            let selected = idx == self.autocomplete_selected;
            let marker = if selected { "▶" } else { "·" };
            let kind = match item.kind {
                AutocompleteKind::Command => "CMD",
                AutocompleteKind::File => "FILE",
            };
            let style = if selected {
                Style::default().fg(Color::Black).bg(Color::LightCyan)
            } else {
                Style::default().fg(Color::Gray)
            };
            let kind_style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", marker), style),
                Span::styled(format!("{} ", kind), kind_style),
                Span::styled(item.display.clone(), style),
            ]));
        }

        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
    }

    /// Render the scrollable conversation timeline and streaming tail.
    ///
    /// # Parameters
    ///
    /// - `f`: Ratatui frame for drawing.
    /// - `area`: Rectangular region allocated for conversation content.
    ///
    /// # Behavior
    ///
    /// Draws historical messages, optional hero header, in-progress tool/activity
    /// indicators, and animated streaming cursor.
    fn render_conversation(&self, f: &mut Frame, area: Rect) {
        let w = area.width.saturating_sub(2) as usize; // padding
        let dim = Style::default().fg(Color::DarkGray);
        let mut lines: Vec<Line> = Vec::new();

        if self.scroll_offset == 0 {
            let reveal = (self.frame_ticker / 2).min(HERO_TAGLINE.chars().count());
            let tagline: String = HERO_TAGLINE.chars().take(reveal).collect();
            let hero_rule: String = std::iter::repeat_n(
                RAIL_FRAMES[self.frame_ticker % RAIL_FRAMES.len()],
                w.min(56),
            )
            .collect();
            lines.push(Line::from(vec![
                Span::styled("  ✦ ", Style::default().fg(Color::LightCyan)),
                Span::styled(
                    HERO_TITLE,
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  ", dim),
                Span::styled(hero_rule, dim),
            ]));
            if !tagline.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("    ", dim),
                    Span::styled(
                        tagline,
                        Style::default()
                            .fg(Color::Gray)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ]));
            }
            lines.push(Line::from(Span::raw("")));
        }

        for entry in &self.messages {
            match entry {
                MessageEntry::User(text) => {
                    for line in text.lines() {
                        lines.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(Color::Cyan)),
                            Span::styled(
                                format!("{} ", PROMPT_CHAR),
                                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                line.to_string(),
                                Style::default()
                                    .fg(Color::White)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]));
                    }
                }
                MessageEntry::Assistant(text) => {
                    for (i, line) in text.lines().enumerate() {
                        let prefix = if i == 0 { ASSISTANT_PREFIX } else { "    " };
                        lines.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                            Span::styled(
                                prefix.to_string(),
                                Style::default().fg(Color::DarkGray),
                            ),
                            Span::raw(line.to_string()),
                        ]));
                    }
                }
                MessageEntry::ToolUse { name, done, error } => {
                    if *error {
                        lines.push(Line::from(vec![
                            Span::raw("  "),
                            Span::styled(super::DOT, Style::default().fg(Color::Red)),
                            Span::raw(" "),
                            Span::styled(name.clone(), Style::default().fg(Color::Red)),
                        ]));
                    } else if *done {
                        lines.push(Line::from(vec![
                            Span::raw("  "),
                            Span::styled(super::DOT, Style::default().fg(Color::Green)),
                            Span::raw(" "),
                            Span::styled(
                                name.clone(),
                                Style::default()
                                    .fg(Color::White)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]));
                    } else {
                        let spinner = SPINNER_FRAMES[self.frame_ticker % SPINNER_FRAMES.len()];
                        lines.push(Line::from(vec![
                            Span::raw("  "),
                            Span::styled(spinner.to_string(), Style::default().fg(Color::LightCyan)),
                            Span::raw(" "),
                            Span::styled(
                                name.clone(),
                                Style::default()
                                    .fg(Color::White)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]));
                    }
                }
                MessageEntry::Error(text) => {
                    for line in text.lines() {
                        lines.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(Color::Red)),
                            Span::styled(ASSISTANT_PREFIX.to_string(), dim),
                            Span::styled(line.to_string(), Style::default().fg(Color::Red)),
                        ]));
                    }
                }
                MessageEntry::System(text) => {
                    for line in text.lines() {
                        lines.push(Line::from(Span::styled(line.to_string(), dim)));
                    }
                }
                MessageEntry::Divider => {
                    let divider_char = RAIL_FRAMES[self.frame_ticker % RAIL_FRAMES.len()];
                    let divider: String = std::iter::repeat_n(divider_char, w.min(80)).collect();
                    lines.push(Line::from(Span::styled(divider, Style::default().fg(Color::DarkGray))));
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
                MessageEntry::Question { question, .. } => {
                    for line in question.lines() {
                        lines.push(Line::from(Span::styled(
                            line.to_string(),
                            Style::default().fg(Color::Cyan),
                        )));
                    }
                }
            }
        }

        // Thinking spinner
        if self.waiting_for_response {
            let spinner = SPINNER_FRAMES[self.frame_ticker % SPINNER_FRAMES.len()];
            let thinking_dots = match self.frame_ticker % 3 {
                0 => ".",
                1 => "..",
                _ => "...",
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", spinner),
                    Style::default().fg(Color::LightCyan),
                ),
                Span::styled(
                    format!("Thinking{}", thinking_dots),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
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
            } else {
                // Empty stream — push a blank streaming line for the cursor
                lines.push(Line::from(vec![Span::styled(ASSISTANT_PREFIX.to_string(), dim)]));
            }
            // Blinking cursor — always on the last streaming line
            let cursor = if self.frame_ticker % 10 < 5 {
                "\u{2588}" // full block
            } else {
                " "
            };
            if let Some(last) = lines.last_mut() {
                last.spans.push(Span::styled(
                    cursor.to_string(),
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }

        let para = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset.min(u16::MAX as u32) as u16, 0));

        f.render_widget(para, area);
    }

    /// Render the one-line status rail for runtime activity and cost.
    ///
    /// # Parameters
    ///
    /// - `f`: Ratatui frame for drawing.
    /// - `area`: Rectangular region allocated for the status line.
    fn render_status_line(&self, f: &mut Frame, area: Rect) {
        let dim = Style::default().fg(Color::DarkGray);
        let w = area.width as usize;

        let pulse = if self.frame_ticker % 8 < 4 { "●" } else { "◌" };
        let left_body = if let Some(ref tool) = self.running_tool {
            format!("{} {}", SPINNER_FRAMES[self.frame_ticker % SPINNER_FRAMES.len()], tool)
        } else if self.current_stream.is_some() {
            format!("{} streaming", SPINNER_FRAMES[self.frame_ticker % SPINNER_FRAMES.len()])
        } else if self.waiting_for_response {
            format!("{} thinking", SPINNER_FRAMES[self.frame_ticker % SPINNER_FRAMES.len()])
        } else {
            "ready".to_string()
        };
        let left = format!(" {} rust-agent | {} ", pulse, left_body);

        // Middle: background task pill (if any running)
        let task_pill = self
            .task_registry
            .as_ref()
            .map(crate::tasks::pill_label::pill_label)
            .unwrap_or_default();

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

        let fill_len = w.saturating_sub(left.len() + task_pill.len() + right.len());
        let rail_char = RAIL_FRAMES[self.frame_ticker % RAIL_FRAMES.len()];
        let fill: String = std::iter::repeat_n(rail_char, fill_len).collect();

        let mut spans = vec![
            Span::styled(left, Style::default().fg(Color::Cyan)),
            Span::styled(fill, dim),
        ];
        if !task_pill.is_empty() {
            spans.push(Span::styled(
                task_pill,
                Style::default().fg(Color::Black).bg(Color::Yellow),
            ));
        }
        spans.push(Span::styled(right, Style::default().fg(Color::Green)));

        let line = Line::from(spans);

        f.render_widget(Paragraph::new(vec![line]), area);
    }

    /// Render the input prompt line.
    ///
    /// # Parameters
    ///
    /// - `f`: Ratatui frame for drawing.
    /// - `area`: Rectangular region allocated for prompt input.
    ///
    /// # Behavior
    ///
    /// Adapts prompt visuals for permission and question modes, and shows a
    /// blinking cursor during normal typing.
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
        } else if self.pending_question.is_some() {
            // Show user input with cyan prompt while answering a question
            let cursor_vis = if self.frame_ticker % 10 < 5 {
                "\u{2588}"
            } else {
                " "
            };
            let line = Line::from(vec![
                Span::styled(
                    format!("{} ", PROMPT_CHAR),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::raw(self.input.clone()),
                Span::styled(cursor_vis.to_string(), Style::default().fg(Color::DarkGray)),
            ]);
            f.render_widget(Paragraph::new(vec![line]), area);
        } else {
            let cursor_vis = if self.frame_ticker % 10 < 5 {
                "\u{2588}" // full block cursor
            } else {
                " "
            };

            let prompt_glyph = if self.frame_ticker % 12 < 6 {
                "❯"
            } else {
                PROMPT_CHAR
            };

            let line = Line::from(vec![
                Span::styled(
                    format!("{} ", prompt_glyph),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::raw(self.input.clone()),
                Span::styled(cursor_vis.to_string(), Style::default().fg(Color::DarkGray)),
            ]);
            f.render_widget(Paragraph::new(vec![line]), area);
        }
    }
}

//! Settings dialog — full configuration editor overlay.
//!
//! Displays all configurable settings grouped into three categories:
//!
//! | Group      | Settings                              |
//! |------------|---------------------------------------|
//! | **General**  | Editor Mode, Permission Mode        |
//! | **Display**  | Theme, Output Style                 |
//! | **Provider** | Default Provider, Default Model     |
//!
//! # Navigation
//!
//! - **↑/↓** (or **j/k**): move between settings
//! - **←/→** (or **h/l**) or **Enter**: cycle through available values
//! - **Esc**: save changes to `~/.rust-agent/config.json` and close
//!
//! # Architecture
//!
//! Settings are loaded from [`GlobalConfig`](crate::config::GlobalConfig) into
//! a flat list of [`SettingEntry`] items grouped by [`SettingGroup`]. When the
//! user modifies a value, the `dirty` flag is set. On close, all settings are
//! persisted back to disk via `GlobalConfig::save()`.
//!
//! The dialog implements the [`Dialog`](super::Dialog) trait so the TUI event
//! loop can route key events and render it as a centered overlay.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use super::{centered_rect, Dialog, DialogAction};

/// A single configurable setting with its available values.
#[derive(Clone)]
struct SettingEntry {
    /// Key used in GlobalConfig (e.g. "editor_mode").
    key: &'static str,
    /// Human-readable label.
    label: &'static str,
    /// Available values to cycle through.
    options: Vec<String>,
    /// Index of the currently selected value.
    current: usize,
}

/// Category grouping for the settings list.
#[derive(Clone)]
struct SettingGroup {
    title: &'static str,
    icon: &'static str,
    entries: Vec<SettingEntry>,
}

/// Interactive settings dialog.
pub struct SettingsDialog {
    groups: Vec<SettingGroup>,
    /// Flat index tracking: (group_idx, entry_idx) for the highlighted row.
    cursor: usize,
    /// Total number of selectable entries.
    total_entries: usize,
    /// Whether a setting was changed (triggers save on close).
    dirty: bool,
}

impl SettingsDialog {
    /// Create a new settings dialog, loading current values from GlobalConfig.
    pub fn new() -> Self {
        let cfg = crate::config::GlobalConfig::load();

        // Editor mode
        let editor_mode_opts = vec!["normal".to_string(), "vim".to_string()];
        let editor_mode_current = match cfg.editor_mode {
            crate::config::EditorMode::Vim => 1,
            _ => 0,
        };

        // Permission mode — display only, not persisted in GlobalConfig
        // but useful to show available modes
        let permission_opts = vec![
            "default".to_string(),
            "accept-edits".to_string(),
            "bypass-permissions".to_string(),
            "plan".to_string(),
            "dont-ask".to_string(),
        ];

        // Theme
        let mut theme_opts = vec!["default".to_string()];
        let styles = crate::output_styles::load_output_styles();
        for s in &styles {
            theme_opts.push(s.name.clone());
        }
        let theme_current = theme_opts
            .iter()
            .position(|t| *t == cfg.theme)
            .unwrap_or(0);

        // Output style
        let mut style_opts = vec!["(none)".to_string()];
        for s in &styles {
            style_opts.push(s.name.clone());
        }
        let style_current = cfg
            .output_style
            .as_ref()
            .and_then(|os| style_opts.iter().position(|s| s == os))
            .unwrap_or(0);

        // Provider
        let provider_opts = vec![
            "claude".to_string(),
            "gemini".to_string(),
            "openai".to_string(),
            "openai-compatible".to_string(),
        ];
        let provider_current = cfg
            .default_provider
            .as_ref()
            .and_then(|p| provider_opts.iter().position(|o| o == p))
            .unwrap_or(1); // default: gemini

        // Model — show known models
        let model_opts = vec![
            "(auto)".to_string(),
            "claude-opus-4-6".to_string(),
            "claude-sonnet-4-6".to_string(),
            "claude-haiku-4-5-20251001".to_string(),
            "gpt-4o".to_string(),
            "gpt-4o-mini".to_string(),
            "o3-mini".to_string(),
            "o4-mini".to_string(),
            "gemini-2.5-pro".to_string(),
            "gemini-2.5-flash".to_string(),
        ];
        let model_current = cfg
            .default_model
            .as_ref()
            .and_then(|m| model_opts.iter().position(|o| o == m))
            .unwrap_or(0);

        let groups = vec![
            SettingGroup {
                title: "General",
                icon: "⚙",
                entries: vec![
                    SettingEntry {
                        key: "editor_mode",
                        label: "Editor Mode",
                        options: editor_mode_opts,
                        current: editor_mode_current,
                    },
                    SettingEntry {
                        key: "permission_mode",
                        label: "Permission Mode",
                        options: permission_opts,
                        current: 0,
                    },
                ],
            },
            SettingGroup {
                title: "Display",
                icon: "◆",
                entries: vec![
                    SettingEntry {
                        key: "theme",
                        label: "Theme",
                        options: theme_opts,
                        current: theme_current,
                    },
                    SettingEntry {
                        key: "output_style",
                        label: "Output Style",
                        options: style_opts,
                        current: style_current,
                    },
                ],
            },
            SettingGroup {
                title: "Provider",
                icon: "◈",
                entries: vec![
                    SettingEntry {
                        key: "default_provider",
                        label: "Default Provider",
                        options: provider_opts,
                        current: provider_current,
                    },
                    SettingEntry {
                        key: "default_model",
                        label: "Default Model",
                        options: model_opts,
                        current: model_current,
                    },
                ],
            },
        ];

        let total_entries: usize = groups.iter().map(|g| g.entries.len()).sum();

        Self {
            groups,
            cursor: 0,
            total_entries,
            dirty: false,
        }
    }

    /// Convert flat cursor index to (group_idx, entry_idx).
    fn cursor_to_indices(&self) -> (usize, usize) {
        let mut remaining = self.cursor;
        for (gi, group) in self.groups.iter().enumerate() {
            if remaining < group.entries.len() {
                return (gi, remaining);
            }
            remaining -= group.entries.len();
        }
        (0, 0)
    }

    /// Get a mutable reference to the entry at the current cursor.
    fn current_entry_mut(&mut self) -> &mut SettingEntry {
        let (gi, ei) = self.cursor_to_indices();
        &mut self.groups[gi].entries[ei]
    }

    /// Cycle the current setting to the next value.
    fn cycle_next(&mut self) {
        let entry = self.current_entry_mut();
        if !entry.options.is_empty() {
            entry.current = (entry.current + 1) % entry.options.len();
            self.dirty = true;
        }
    }

    /// Cycle the current setting to the previous value.
    fn cycle_prev(&mut self) {
        let entry = self.current_entry_mut();
        if !entry.options.is_empty() {
            entry.current = if entry.current == 0 {
                entry.options.len() - 1
            } else {
                entry.current - 1
            };
            self.dirty = true;
        }
    }

    /// Persist all settings to GlobalConfig.
    fn save_settings(&self) {
        let mut cfg = crate::config::GlobalConfig::load();

        for group in &self.groups {
            for entry in &group.entries {
                let val = &entry.options[entry.current];
                match entry.key {
                    "editor_mode" => {
                        cfg.editor_mode = match val.as_str() {
                            "vim" => crate::config::EditorMode::Vim,
                            _ => crate::config::EditorMode::Normal,
                        };
                    }
                    "theme" => {
                        cfg.theme = val.clone();
                    }
                    "output_style" => {
                        cfg.output_style = if val == "(none)" {
                            None
                        } else {
                            Some(val.clone())
                        };
                    }
                    "default_provider" => {
                        cfg.default_provider = Some(val.clone());
                    }
                    "default_model" => {
                        cfg.default_model = if val == "(auto)" {
                            None
                        } else {
                            Some(val.clone())
                        };
                    }
                    // permission_mode is runtime-only, not persisted here
                    _ => {}
                }
            }
        }

        if let Err(e) = cfg.save() {
            tracing::error!("Failed to save settings: {}", e);
        }
    }
}

impl Default for SettingsDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl Dialog for SettingsDialog {
    fn title(&self) -> &str {
        "Settings"
    }

    fn handle_key(&mut self, key: KeyEvent) -> DialogAction {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                DialogAction::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.cursor + 1 < self.total_entries {
                    self.cursor += 1;
                }
                DialogAction::Continue
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                self.cycle_next();
                DialogAction::Continue
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.cycle_prev();
                DialogAction::Continue
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                if self.dirty {
                    self.save_settings();
                }
                DialogAction::Select("saved".to_string())
            }
            _ => DialogAction::Continue,
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let width = 56u16;
        // Calculate height: title + groups (title line + separator + entries) + footer
        let content_lines: usize = self
            .groups
            .iter()
            .map(|g| 2 + g.entries.len()) // group title + separator + entries
            .sum::<usize>()
            + 2; // top padding + footer
        let height = (content_lines as u16 + 3).min(area.height.saturating_sub(4));
        let rect = centered_rect(width, height, area);

        f.render_widget(Clear, rect);

        let mut lines: Vec<Line> = Vec::new();
        let mut flat_idx = 0usize;

        let dim = Style::default().fg(Color::DarkGray);
        let label_w = 20;

        for (group_idx, group) in self.groups.iter().enumerate() {
            if group_idx > 0 {
                lines.push(Line::from(""));
            }

            // Group title
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", group.icon),
                    Style::default().fg(Color::LightCyan),
                ),
                Span::styled(
                    group.title.to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));

            // Separator
            let sep: String = std::iter::repeat_n('─', (width as usize).saturating_sub(6)).collect();
            lines.push(Line::from(Span::styled(format!("  {}", sep), dim)));

            // Entries
            for entry in &group.entries {
                let is_selected = flat_idx == self.cursor;
                let value = &entry.options[entry.current];

                let prefix = if is_selected { " ▸ " } else { "   " };

                // Pad label to fixed width
                let label_padded = format!("{:<width$}", entry.label, width = label_w);

                let value_display = if entry.options.len() > 1 {
                    format!("◂ {} ▸", value)
                } else {
                    value.clone()
                };

                if is_selected {
                    lines.push(Line::from(vec![
                        Span::styled(
                            prefix.to_string(),
                            Style::default().fg(Color::LightCyan),
                        ),
                        Span::styled(
                            label_padded,
                            Style::default()
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            value_display,
                            Style::default()
                                .fg(Color::LightCyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(prefix.to_string(), dim),
                        Span::styled(label_padded, Style::default().fg(Color::Gray)),
                        Span::styled(value_display, Style::default().fg(Color::White)),
                    ]));
                }

                flat_idx += 1;
            }
        }

        // Footer hint
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                "  ↑↓ navigate  ◂▸/Enter cycle  Esc save & close",
                dim,
            ),
        ]));

        let dirty_indicator = if self.dirty { " *" } else { "" };
        let title = format!(" Settings{} (↑/↓ ◂/▸ Esc) ", dirty_indicator);

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::LightCyan));

        let para = Paragraph::new(lines).block(block);
        f.render_widget(para, rect);
    }
}

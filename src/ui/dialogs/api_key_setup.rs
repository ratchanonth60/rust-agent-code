//! API key setup dialog — first-run prompt for entering an API key or OAuth login.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::engine::ModelProvider;

use super::{centered_rect, Dialog, DialogAction};

/// All providers in selection order.
const ALL_PROVIDERS: [ModelProvider; 4] = [
    ModelProvider::Gemini,
    ModelProvider::Claude,
    ModelProvider::OpenAI,
    ModelProvider::OpenAICompatible,
];

/// Focus state within the setup dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupFocus {
    /// User is selecting a provider with Left/Right.
    ProviderSelector,
    /// User is typing an API key.
    ApiKeyInput,
    /// User is hovering the OAuth login button (Gemini only).
    OAuthButton,
}

/// Interactive API key setup dialog shown on first run when no key is configured.
pub struct ApiKeySetupDialog {
    /// Index into ALL_PROVIDERS for the selected provider.
    provider_index: usize,
    /// The API key being typed.
    input: String,
    /// Current input focus.
    focus: SetupFocus,
    /// Whether to show the key in plain text.
    show_key: bool,
    /// Validation error message.
    pub error: Option<String>,
}

impl ApiKeySetupDialog {
    /// Create a new setup dialog for the given provider.
    pub fn new(provider: ModelProvider) -> Self {
        let provider_index = ALL_PROVIDERS
            .iter()
            .position(|p| *p == provider)
            .unwrap_or(0);
        Self {
            provider_index,
            input: String::new(),
            focus: SetupFocus::ProviderSelector,
            show_key: false,
            error: None,
        }
    }

    /// Current selected provider.
    fn provider(&self) -> ModelProvider {
        ALL_PROVIDERS[self.provider_index]
    }

    /// Whether the current provider supports OAuth.
    fn has_oauth(&self) -> bool {
        matches!(self.provider(), ModelProvider::Gemini)
    }
}

impl Dialog for ApiKeySetupDialog {
    fn title(&self) -> &str {
        "API Key Setup"
    }

    fn handle_key(&mut self, key: KeyEvent) -> DialogAction {
        match key.code {
            KeyCode::Esc => DialogAction::Cancel,

            // Ctrl+S: toggle key visibility
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.show_key = !self.show_key;
                DialogAction::Continue
            }

            // Left/Right: cycle provider (when focused on provider selector)
            KeyCode::Left if self.focus == SetupFocus::ProviderSelector => {
                if self.provider_index > 0 {
                    self.provider_index -= 1;
                } else {
                    self.provider_index = ALL_PROVIDERS.len() - 1;
                }
                self.input.clear();
                self.error = None;
                DialogAction::Continue
            }
            KeyCode::Right if self.focus == SetupFocus::ProviderSelector => {
                self.provider_index = (self.provider_index + 1) % ALL_PROVIDERS.len();
                self.input.clear();
                self.error = None;
                DialogAction::Continue
            }

            // Tab/BackTab: cycle focus (ProviderSelector → ApiKeyInput → OAuthButton → ...)
            KeyCode::Tab => {
                self.focus = match self.focus {
                    SetupFocus::ProviderSelector => SetupFocus::ApiKeyInput,
                    SetupFocus::ApiKeyInput => {
                        if self.has_oauth() {
                            SetupFocus::OAuthButton
                        } else {
                            SetupFocus::ProviderSelector
                        }
                    }
                    SetupFocus::OAuthButton => SetupFocus::ProviderSelector,
                };
                self.error = None;
                DialogAction::Continue
            }
            KeyCode::BackTab => {
                self.focus = match self.focus {
                    SetupFocus::ProviderSelector => {
                        if self.has_oauth() {
                            SetupFocus::OAuthButton
                        } else {
                            SetupFocus::ApiKeyInput
                        }
                    }
                    SetupFocus::ApiKeyInput => SetupFocus::ProviderSelector,
                    SetupFocus::OAuthButton => SetupFocus::ApiKeyInput,
                };
                self.error = None;
                DialogAction::Continue
            }

            // Enter: submit
            KeyCode::Enter => match self.focus {
                SetupFocus::ProviderSelector => {
                    // Move to input field
                    self.focus = SetupFocus::ApiKeyInput;
                    DialogAction::Continue
                }
                SetupFocus::ApiKeyInput => {
                    let key = self.input.trim().to_string();
                    if key.is_empty() {
                        self.error = Some("API key cannot be empty".into());
                        DialogAction::Continue
                    } else {
                        let provider_tag = provider_tag(self.provider());
                        DialogAction::Select(format!("apikey:{}:{}", provider_tag, key))
                    }
                }
                SetupFocus::OAuthButton => {
                    DialogAction::Select("oauth:gemini".to_string())
                }
            },

            // Text input (only when focused on the input field)
            KeyCode::Char(c) if self.focus == SetupFocus::ApiKeyInput => {
                self.input.push(c);
                self.error = None;
                DialogAction::Continue
            }
            KeyCode::Backspace if self.focus == SetupFocus::ApiKeyInput => {
                self.input.pop();
                self.error = None;
                DialogAction::Continue
            }

            _ => DialogAction::Continue,
        }
    }

    fn render(&self, f: &mut Frame, area: Rect) {
        let width = 56u16;
        let height = if self.has_oauth() { 18u16 } else { 14u16 };
        let rect = centered_rect(width, height, area);
        f.render_widget(Clear, rect);

        let env_var = env_var_name(self.provider());
        let provider_name = provider_display_name(self.provider());
        let inner_width = (width as usize).saturating_sub(6);

        let provider_focused = self.focus == SetupFocus::ProviderSelector;
        let input_focused = self.focus == SetupFocus::ApiKeyInput;
        let oauth_focused = self.focus == SetupFocus::OAuthButton;

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(""));

        // Provider selector row with arrows
        let arrow_style = if provider_focused {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let name_style = if provider_focused {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        };
        let highlight = if provider_focused { " \u{25c0} " } else { "   " };
        let highlight_r = if provider_focused { " \u{25b6}" } else { "" };
        lines.push(Line::from(vec![
            Span::styled("  Provider: ", Style::default().fg(Color::DarkGray)),
            Span::styled(highlight, arrow_style),
            Span::styled(provider_name, name_style),
            Span::styled(highlight_r, arrow_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Env var:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(env_var, Style::default().fg(Color::Yellow)),
        ]));
        lines.push(Line::from(""));

        // Input field
        let input_border_color = if input_focused { Color::LightCyan } else { Color::DarkGray };
        let input_top = format!("  \u{250C}{}\u{2510}", "\u{2500}".repeat(inner_width));
        let input_bot = format!("  \u{2514}{}\u{2518}", "\u{2500}".repeat(inner_width));

        lines.push(Line::from(Span::styled(input_top, Style::default().fg(input_border_color))));

        // Display the key (masked or plain)
        let display_text = if self.input.is_empty() {
            if input_focused {
                "\u{2588}".to_string() // block cursor
            } else {
                "enter your API key...".to_string()
            }
        } else if self.show_key {
            let visible = &self.input[self.input.len().saturating_sub(inner_width - 1)..];
            if input_focused {
                format!("{}\u{2588}", visible)
            } else {
                visible.to_string()
            }
        } else {
            let stars = "*".repeat(self.input.len().min(inner_width - 1));
            if input_focused {
                format!("{}\u{2588}", stars)
            } else {
                stars
            }
        };
        let padded = format!("{:<width$}", display_text, width = inner_width);
        let input_style = if input_focused {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(vec![
            Span::styled("  \u{2502}", Style::default().fg(input_border_color)),
            Span::styled(padded, input_style),
            Span::styled("\u{2502}", Style::default().fg(input_border_color)),
        ]));

        lines.push(Line::from(Span::styled(input_bot, Style::default().fg(input_border_color))));

        // Error message
        if let Some(ref err) = self.error {
            lines.push(Line::from(Span::styled(
                format!("  {}", err),
                Style::default().fg(Color::Red),
            )));
        } else {
            lines.push(Line::from(""));
        }

        // OAuth section (Gemini only)
        if self.has_oauth() {
            lines.push(Line::from(Span::styled(
                "  Or authenticate with Google:",
                Style::default().fg(Color::DarkGray),
            )));
            let oauth_style = if oauth_focused {
                Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            let prefix = if oauth_focused { " \u{25b8} " } else { "   " };
            lines.push(Line::from(Span::styled(
                format!("{}Login via OAuth (opens browser)", prefix),
                oauth_style,
            )));
            lines.push(Line::from(""));
        }

        // Help footer
        let help = if self.has_oauth() {
            "  \u{25c0}/\u{25b6} provider  Tab switch  Enter  Esc  ^S show"
        } else {
            "  \u{25c0}/\u{25b6} provider  Tab switch  Enter  Esc  ^S"
        };
        lines.push(Line::from(Span::styled(
            help,
            Style::default().fg(Color::DarkGray),
        )));

        let block = Block::default()
            .title(format!(" {} Setup ", provider_name))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        f.render_widget(Paragraph::new(lines).block(block), rect);
    }
}

/// Map provider to its environment variable name.
fn env_var_name(provider: ModelProvider) -> &'static str {
    match provider {
        ModelProvider::Claude => "ANTHROPIC_API_KEY",
        ModelProvider::OpenAI => "OPENAI_API_KEY",
        ModelProvider::Gemini => "GEMINI_API_KEY",
        ModelProvider::OpenAICompatible => "OPENAI_COMPAT_API_KEY",
    }
}

/// Map provider to a display name.
fn provider_display_name(provider: ModelProvider) -> &'static str {
    match provider {
        ModelProvider::Claude => "Claude (Anthropic)",
        ModelProvider::OpenAI => "OpenAI",
        ModelProvider::Gemini => "Gemini (Google)",
        ModelProvider::OpenAICompatible => "OpenAI-Compatible",
    }
}

/// Short tag for encoding provider in Select value.
fn provider_tag(provider: ModelProvider) -> &'static str {
    match provider {
        ModelProvider::Claude => "claude",
        ModelProvider::OpenAI => "openai",
        ModelProvider::Gemini => "gemini",
        ModelProvider::OpenAICompatible => "openai-compat",
    }
}

/// Parse a provider tag back to ModelProvider.
pub fn parse_provider_tag(tag: &str) -> Option<ModelProvider> {
    match tag {
        "claude" => Some(ModelProvider::Claude),
        "openai" => Some(ModelProvider::OpenAI),
        "gemini" => Some(ModelProvider::Gemini),
        "openai-compat" => Some(ModelProvider::OpenAICompatible),
        _ => None,
    }
}

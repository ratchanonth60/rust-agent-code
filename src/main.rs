//! Rust Agent — a Rust port of Claude Code.
//!
//! Entry point for the CLI.  Supports three operating modes:
//! - **One-shot** (`--query "..."`) — run a single query and exit.
//! - **Bare** (`--bare`) — simple stdin/stdout REPL without TUI.
//! - **Interactive** (default) — full ratatui TUI with streaming,
//!   tool dots, permission prompts, and slash commands.

pub mod auth;
pub mod commands;
pub mod config;
pub mod context;
pub mod engine;
pub mod keybindings;
pub mod mcp;
pub mod mem;
pub mod models;
pub mod output_styles;
pub mod permissions;
pub mod plugins;
pub mod skills;
pub mod tasks;
pub mod tools;
pub mod ui;

use clap::Parser;
use tokio::sync::mpsc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use crate::engine::{EngineConfig, ModelProvider, QueryEngine};
use crate::permissions::PermissionMode;

/// Rust-based AI Agent CLI (ported from TypeScript)
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The input task or query for the agent
    #[arg(short, long)]
    query: Option<String>,

    /// Run in bare/simple mode (no TUI, stdin/stdout only)
    #[arg(long, default_value_t = false)]
    bare: bool,

    /// Automatically run commands and bypass permission prompts
    #[arg(long, default_value_t = false)]
    auto: bool,

    /// Provider backend to use
    #[arg(long, value_enum, default_value_t = ModelProvider::Gemini)]
    provider: ModelProvider,

    /// Model name to use with the selected provider
    #[arg(long)]
    model: Option<String>,

    /// API key override for selected provider
    #[arg(long)]
    api_key: Option<String>,

    /// API base URL override for selected provider
    #[arg(long)]
    api_base: Option<String>,

    /// Maximum output tokens per LLM call
    #[arg(long, default_value_t = 8192)]
    max_tokens: u32,

    /// Maximum budget in USD for this session
    #[arg(long)]
    max_budget: Option<f64>,

    /// Permission mode for tool authorization
    #[arg(long, value_enum, default_value_t = PermissionMode::Default)]
    permission_mode: PermissionMode,
}

/// Returns the default model name for a given provider.
fn default_model(provider: ModelProvider) -> &'static str {
    match provider {
        ModelProvider::Gemini => "gemini-2.5-flash",
        ModelProvider::OpenAI => "gpt-4o-mini",
        ModelProvider::Claude => "claude-sonnet-4-6",
        ModelProvider::OpenAICompatible => "gpt-4o-mini",
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Attempt to load .env files (project + global)
    let _ = dotenvy::dotenv();
    let global_env = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".rust-agent")
        .join(".env");
    let _ = dotenvy::from_path(&global_env);

    // In TUI mode ratatui owns the terminal; any write to stdout/stderr corrupts it.
    // Redirect tracing to a log file instead.
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(
            dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".rust-agent.log"),
        )
        .ok();
    if let Some(file) = log_file {
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .with_writer(std::sync::Mutex::new(file))
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("Setting default subscriber failed");
    } else {
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::WARN)
            .with_writer(std::io::stderr)
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("Setting default subscriber failed");
    }

    let args = Args::parse();

    info!("Starting Rust Agent...");

    let selected_model = args
        .model
        .clone()
        .unwrap_or_else(|| default_model(args.provider).to_string());
    info!(
        "Using provider {:?} with model {}",
        args.provider, selected_model
    );

    let config = EngineConfig {
        auto_mode: args.auto,
        bare_mode: args.bare,
        debug: false,
        max_budget_usd: args.max_budget,
        max_tokens: args.max_tokens,
        permission_mode: args.permission_mode,
    };

    // ── One-shot mode ────────────────────────────────────────────────────
    if let Some(q) = args.query {
        let engine = QueryEngine::new(
            selected_model,
            args.provider,
            args.api_key.clone(),
            args.api_base.clone(),
            config,
            None, // no TUI channel in one-shot mode
        )?
        .with_agent_tool();

        info!("Received query: {}", q);
        let cost_tracker = engine.cost_tracker.clone();
        let result = engine.query(&q, None).await;

        match result {
            Ok(res) => println!("{}", res),
            Err(e) => eprintln!("Error: {:?}", e),
        }

        print_cost_summary(&cost_tracker);

    // ── Bare mode ──────────────────────────────────────────────────────
    } else if args.bare {
        let engine = QueryEngine::new(
            selected_model,
            args.provider,
            args.api_key.clone(),
            args.api_base.clone(),
            config,
            None, // no TUI channel in bare mode
        )?
        .with_agent_tool();

        info!("Running in bare mode.");
        let cost_tracker = engine.cost_tracker.clone();
        let engine = std::sync::Arc::new(engine);

        let stdin = tokio::io::stdin();
        let reader = tokio::io::BufReader::new(stdin);

        use tokio::io::AsyncBufReadExt;
        let mut lines = reader.lines();

        eprintln!("Rust Agent (bare mode). Type your query, press Enter. Ctrl+D to exit.");
        loop {
            eprint!("> ");
            match lines.next_line().await? {
                Some(line) => {
                    let line = line.trim().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    if line == "quit" || line == "exit" {
                        break;
                    }
                    match engine.query(&line, None).await {
                        Ok(res) => println!("{}", res),
                        Err(e) => eprintln!("Error: {:?}", e),
                    }
                }
                None => break, // EOF
            }
        }

        print_cost_summary(&cost_tracker);

    // ── Interactive TUI mode ───────────────────────────────────────────
    } else {
        info!("Starting interactive UI...");

        // Check if API key is available; if not, run setup dialog first.
        // The setup dialog allows the user to select a different provider.
        let mut active_provider = args.provider;
        let mut active_model = selected_model.clone();
        let mut active_api_key = args.api_key.clone();

        if engine::resolve_api_key(args.provider, args.api_key.as_deref()).is_none() {
            info!("No API key found, launching setup dialog...");
            let setup_result = run_setup_flow(args.provider).await;
            match setup_result {
                SetupResult::ApiKey(provider, key) => {
                    let var_name = env_var_for_provider(provider);
                    // Persist to ~/.rust-agent/.env for future runs
                    save_api_key_to_env(var_name, &key);
                    std::env::set_var(var_name, &key);
                    // If user picked a different provider, update accordingly
                    if provider != args.provider {
                        active_provider = provider;
                        active_model = default_model(provider).to_string();
                        active_api_key = Some(key);
                    }
                }
                SetupResult::OAuthDone => {
                    // Token is now in credentials.json, resolve_gemini_token() will find it
                }
                SetupResult::Cancelled => {
                    eprintln!("No API key configured. Exiting.");
                    return Ok(());
                }
            }
        }

        // Channel: user questions from AskUserQuestionTool → TUI
        let (question_tx, question_rx) =
            mpsc::channel::<crate::tools::ask_user::QuestionRequest>(8);

        let engine = QueryEngine::new(
            active_model,
            active_provider,
            active_api_key,
            args.api_base.clone(),
            config,
            Some(question_tx),
        )?
        .with_agent_tool();

        // Channel: user input → engine background task
        let (tx_to_engine, mut rx_to_engine) = mpsc::channel::<String>(32);
        // Channel: engine events → TUI
        let (tx_to_ui, rx_to_ui) = mpsc::channel::<ui::app::UiEvent>(32);

        let cost_tracker = engine.cost_tracker.clone();
        let task_registry = engine.task_registry.clone();
        let engine_clone = std::sync::Arc::new(engine);

        tokio::spawn(async move {
            while let Some(query) = rx_to_engine.recv().await {
                // Handle session resume via special prefix.
                if let Some(payload) = query.strip_prefix("__resume:") {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(payload) {
                        if let Some(messages) =
                            parsed.get(r##"messages"##).and_then(|m| m.as_array())
                        {
                            let msgs: Vec<serde_json::Value> = messages.clone();
                            if let Ok(mut session) = engine_clone.session.lock() {
                                session.messages = msgs;
                                session.saved_message_count = session.messages.len();
                                session.header_written = true;
                            }
                            let _ = tx_to_ui
                                .send(ui::app::UiEvent::LLMResponse(
                                    "Session restored. You can continue the conversation."
                                        .to_string(),
                                ))
                                .await;
                            continue;
                        }
                    }
                    let _ = tx_to_ui
                        .send(ui::app::UiEvent::LLMError(
                            "Failed to parse resume payload.".to_string(),
                        ))
                        .await;
                    continue;
                }

                match engine_clone.query(&query, Some(tx_to_ui.clone())).await {
                    Ok(response) => {
                        let _ = tx_to_ui.send(ui::app::UiEvent::LLMResponse(response)).await;
                    }
                    Err(e) => {
                        let _ = tx_to_ui
                            .send(ui::app::UiEvent::LLMError(e.to_string()))
                            .await;
                    }
                }
            }
        });

        // Build the slash command registry
        let command_registry = commands::build_default_registry();

        // Enter interactive Ratatui mode
        let mut terminal = ui::setup_terminal()?;
        let mut app = ui::app::App::new(tx_to_engine, rx_to_ui, question_rx, command_registry);
        app.cost_tracker = Some(cost_tracker.clone());
        app.task_registry = Some(task_registry);

        let app_result = app.run(&mut terminal).await;

        ui::restore_terminal()?;

        // Print cost summary on exit
        print_cost_summary(&cost_tracker);

        if let Err(err) = app_result {
            eprintln!("App error: {:?}", err);
        }
    }

    Ok(())
}

/// Prints the session cost summary to stderr (if any cost was incurred).
fn print_cost_summary(
    cost_tracker: &std::sync::Arc<std::sync::Mutex<crate::engine::cost_tracker::CostTracker>>,
) {
    if let Ok(tracker) = cost_tracker.lock() {
        if tracker.total_cost_usd > 0.0 {
            eprintln!("\n{}", tracker.format_total_cost());
        }
    }
}

// ── API Key Setup Flow ──────────────────────────────────────────────────

/// Result of the pre-engine API key setup dialog.
enum SetupResult {
    /// User entered an API key directly (with the selected provider).
    ApiKey(ModelProvider, String),
    /// User completed OAuth login (token is in credentials.json).
    OAuthDone,
    /// User cancelled the setup.
    Cancelled,
}

/// Run a minimal TUI loop showing the API key setup dialog.
///
/// Returns when the user enters a key, completes OAuth, or cancels.
async fn run_setup_flow(provider: ModelProvider) -> SetupResult {
    use crossterm::event::{self, Event};
    use std::time::Duration;
    use ui::dialogs::api_key_setup::ApiKeySetupDialog;
    use ui::dialogs::{Dialog, DialogAction};

    loop {
        let mut terminal = match ui::setup_terminal() {
            Ok(t) => t,
            Err(_) => return SetupResult::Cancelled,
        };

        let mut dialog = ApiKeySetupDialog::new(provider);

        let result = loop {
            let _ = terminal.draw(|f| {
                let area = f.size();
                f.render_widget(
                    ratatui::widgets::Block::default()
                        .style(ratatui::style::Style::default().bg(ratatui::style::Color::Black)),
                    area,
                );
                dialog.render(f, area);
            });

            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    match dialog.handle_key(key) {
                        DialogAction::Continue => {}
                        DialogAction::Select(value) => break Some(value),
                        DialogAction::Cancel => break None,
                    }
                }
            }
        };

        let _ = ui::restore_terminal();

        match result {
            None => return SetupResult::Cancelled,
            Some(value) => {
                if let Some(rest) = value.strip_prefix("apikey:") {
                    // Format: "apikey:<provider_tag>:<key>"
                    if let Some((tag, key)) = rest.split_once(':') {
                        let provider = ui::dialogs::api_key_setup::parse_provider_tag(tag)
                            .unwrap_or(provider);
                        return SetupResult::ApiKey(provider, key.to_string());
                    }
                    // Fallback: no provider tag, treat entire rest as key
                    return SetupResult::ApiKey(provider, rest.to_string());
                } else if let Some(oauth_provider) = value.strip_prefix("oauth:") {
                    // Run OAuth flow in normal terminal (browser-based)
                    eprintln!("  Starting OAuth login...");
                    match crate::auth::oauth::run_oauth_flow(oauth_provider).await {
                        Ok(()) => {
                            // Verify token is now available
                            let token_ok = match oauth_provider {
                                "claude" | "anthropic" => {
                                    crate::auth::resolve_claude_token().ok().flatten().is_some()
                                }
                                _ => {
                                    crate::auth::resolve_gemini_token().ok().flatten().is_some()
                                }
                            };
                            if token_ok {
                                eprintln!("  Login successful!");
                                return SetupResult::OAuthDone;
                            }
                            eprintln!("  OAuth completed but no token found. Try again.");
                        }
                        Err(e) => {
                            eprintln!("  OAuth failed: {}", e);
                        }
                    }
                    eprintln!("  Press Enter to try again...");
                    let _ = std::io::stdin().read_line(&mut String::new());
                    // Loop back to show dialog again
                    continue;
                }
            }
        }
    }
}

/// Map a provider to its primary environment variable name.
fn env_var_for_provider(provider: ModelProvider) -> &'static str {
    match provider {
        ModelProvider::Claude => "ANTHROPIC_API_KEY",
        ModelProvider::OpenAI => "OPENAI_API_KEY",
        ModelProvider::Gemini => "GEMINI_API_KEY",
        ModelProvider::OpenAICompatible => "OPENAI_COMPAT_API_KEY",
    }
}

/// Save an API key to `~/.rust-agent/.env` for persistence across sessions.
fn save_api_key_to_env(var_name: &str, key: &str) {
    let env_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".rust-agent");
    let _ = std::fs::create_dir_all(&env_dir);
    let env_path = env_dir.join(".env");

    // Read existing content, replace or append the variable
    let existing = std::fs::read_to_string(&env_path).unwrap_or_default();
    let prefix = format!("{}=", var_name);
    let mut found = false;
    let mut lines: Vec<String> = existing
        .lines()
        .map(|line| {
            if line.starts_with(&prefix) {
                found = true;
                format!("{}={}", var_name, key)
            } else {
                line.to_string()
            }
        })
        .collect();
    if !found {
        lines.push(format!("{}={}", var_name, key));
    }
    let content = lines.join("\n") + "\n";
    let _ = std::fs::write(&env_path, content);

    // chmod 600 on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&env_path, std::fs::Permissions::from_mode(0o600));
    }
}

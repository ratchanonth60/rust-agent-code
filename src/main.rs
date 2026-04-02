pub mod engine;
pub mod models;
pub mod tools;
pub mod ui;
pub mod mem;
pub mod keybindings;
pub mod output_styles;
pub mod permissions;
pub mod context;

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

fn default_model(provider: ModelProvider) -> &'static str {
    match provider {
        ModelProvider::Gemini => "gemini-2.5-pro",
        ModelProvider::OpenAI => "gpt-4o-mini",
        ModelProvider::Claude => "claude-sonnet-4-20250514",
        ModelProvider::OpenAICompatible => "gpt-4o-mini",
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Attempt to load .env file
    let _ = dotenvy::dotenv();

    // Initialize tracing
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("Setting default subscriber failed");

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

    let engine = QueryEngine::new(
        selected_model,
        args.provider,
        args.api_key.clone(),
        args.api_base.clone(),
        config,
    )?;

    if let Some(q) = args.query {
        // One-shot mode
        info!("Received query: {}", q);
        let cost_tracker = engine.cost_tracker.clone();
        let result = engine.query(&q, None).await;

        match result {
            Ok(res) => println!("{}", res),
            Err(e) => eprintln!("Error: {:?}", e),
        }

        // Print cost summary
        print_cost_summary(&cost_tracker);
    } else if args.bare {
        // Bare mode: simple stdin/stdout loop, no TUI
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
    } else {
        // Interactive TUI mode
        info!("Starting interactive UI...");

        // Setup channels for UI <-> Engine
        let (tx_to_engine, mut rx_to_engine) = mpsc::channel::<String>(32);
        let (tx_to_ui, rx_to_ui) = mpsc::channel::<ui::app::UiEvent>(32);

        let cost_tracker = engine.cost_tracker.clone();
        let engine_clone = std::sync::Arc::new(engine);

        tokio::spawn(async move {
            while let Some(query) = rx_to_engine.recv().await {
                match engine_clone.query(&query, Some(tx_to_ui.clone())).await {
                    Ok(response) => {
                        let _ = tx_to_ui.send(ui::app::UiEvent::LLMResponse(response)).await;
                    }
                    Err(e) => {
                        let _ = tx_to_ui.send(ui::app::UiEvent::LLMError(e.to_string())).await;
                    }
                }
            }
        });

        // Enter interactive Ratatui mode
        let mut terminal = ui::setup_terminal()?;
        let mut app = ui::app::App::new(tx_to_engine, rx_to_ui);
        app.cost_tracker = Some(cost_tracker.clone());

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

fn print_cost_summary(cost_tracker: &std::sync::Arc<std::sync::Mutex<crate::engine::cost_tracker::CostTracker>>) {
    if let Ok(tracker) = cost_tracker.lock() {
        if tracker.total_cost_usd > 0.0 {
            eprintln!("\n{}", tracker.format_total_cost());
        }
    }
}

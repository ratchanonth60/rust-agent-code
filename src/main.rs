pub mod engine;
pub mod models;
pub mod tools;
pub mod ui;
pub mod mem;
pub mod keybindings;
pub mod output_styles;

use clap::Parser;
use tokio::sync::mpsc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use crate::engine::{ModelProvider, QueryEngine, EngineConfig};

/// Rust-based AI Agent CLI (ported from TypeScript)
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The input task or query for the agent
    #[arg(short, long)]
    query: Option<String>,

    /// Run in bare/simple mode (minimal UI chrome)
    #[arg(long, default_value_t = false)]
    bare: bool,

    /// Automatically run commands and bypass permission prompts [Y/n]
    #[arg(long, default_value_t = false)]
    auto: bool,

    /// API key for the selected provider
    #[arg(long, env = "API_KEY")]
    api_key: Option<String>,

    /// Model provider (gemini, openai, openrouter)
    #[arg(short, long, env = "PROVIDER", default_value = "gemini")]
    provider: String,

    /// Model to use (e.g. gemini-3-flash-preview, gpt-4o)
    #[arg(short, long, env = "MODEL", default_value = "gemini-3-flash-preview")]
    model: String,

    /// Override API base URL for custom OpenAI-compatible endpoints
    #[arg(long, env = "API_BASE")]
    api_base: Option<String>,
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
    if args.bare {
        info!("Running in bare mode.");
    }

    let api_key = args.api_key.unwrap_or_default();
    if api_key.is_empty() {
        eprintln!("Error: API_KEY is required. Set it via --api-key flag or API_KEY env var.");
        std::process::exit(1);
    }

    let provider = match args.provider.to_lowercase().as_str() {
        "openai" => ModelProvider::OpenAI,
        "openrouter" => ModelProvider::OpenRouter,
        "gemini" => ModelProvider::Gemini,
        other => {
            eprintln!("Unknown provider '{}'. Use: gemini, openai, openrouter", other);
            std::process::exit(1);
        }
    };

    // Initialize Query Engine with config
    let config = EngineConfig {
        auto_mode: args.auto,
        bare_mode: args.bare,
    };
    let engine = QueryEngine::new(&args.model, provider, config, api_key, args.api_base);

    if let Some(q) = args.query {
        info!("Received query: {}", q);
        let result = engine.query(&q, None).await;

        match result {
            Ok(res) => println!("\n🤖 Agent says:\n{}", res),
            Err(e) => eprintln!("Error querying AI: {:?}", e),
        }

        // Print cost summary
        if let Ok(tracker) = engine.cost_tracker.lock() {
            eprintln!("\n{}", tracker.format_total_cost());
        }
    } else {
        info!("No query provided. Starting interactive UI...");
        
        // Setup channels for UI <-> Engine
        let (tx_to_engine, mut rx_to_engine) = mpsc::channel::<String>(32);
        let (tx_to_ui, rx_to_ui) = mpsc::channel::<ui::app::UiEvent>(32);

        // Make engine Send + Sync by wrapping heavily if needed, 
        // but here engine is owned by the background task
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
        
        let app_result = app.run(&mut terminal).await;

        ui::restore_terminal()?;

        if let Err(err) = app_result {
            eprintln!("App error: {:?}", err);
        }
    }

    Ok(())
}

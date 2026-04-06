//! Agentic query engine with multi-provider LLM support.
//!
//! [`QueryEngine`] is the core execution loop.  It sends a user query to
//! an LLM, inspects the response for tool-use requests, dispatches them,
//! feeds the results back, and repeats until the model produces a final
//! text answer.
//!
//! # Supported providers
//!
//! | Provider             | Module                           | Transport          |
//! |----------------------|----------------------------------|--------------------|
//! | Claude               | [`providers::claude`]            | Native HTTP + SSE  |
//! | OpenAI / compatible  | [`providers::openai`]            | async-openai       |
//! | Gemini               | [`providers::gemini`]            | Raw HTTP + SSE     |
//!
//! Each provider lives in its own file under `engine/providers/` and
//! implements the agentic loop as `impl QueryEngine` methods.
//!
//! # Layout
//!
//! This file contains only the shared core:
//!
//! - [`ModelProvider`] enum
//! - [`QueryEngine`] struct + constructor
//! - Shared helpers: `find_tool`, `check_tool_permission`, `auto_save_session`
//! - The top-level `query()` dispatcher

use anyhow::{anyhow, Result};
use async_openai::{
    config::OpenAIConfig,
    Client,
};
use clap::ValueEnum;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tracing::info;

use crate::engine::config::EngineConfig;
use crate::engine::cost_tracker::CostTracker;
use crate::engine::session::Session;
use crate::permissions::{PermissionDecision, PermissionRule, check_permission};
use crate::tools::{Tool, ToolContext};

/// LLM provider selection.
///
/// Parsed from the `--provider` CLI flag via [`clap::ValueEnum`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ModelProvider {
    OpenAI,
    Gemini,
    Claude,
    OpenAICompatible,
}

/// Check whether an API key is available for the given provider.
///
/// Returns `Some(key)` if found via the auth chain (OAuth, env var),
/// or `None` if no key is configured. Used by the TUI to decide
/// whether to show the setup dialog before creating a [`QueryEngine`].
pub fn resolve_api_key(provider: ModelProvider, api_key_override: Option<&str>) -> Option<String> {
    if let Some(key) = api_key_override {
        return Some(key.to_string());
    }
    let key = match provider {
        ModelProvider::Claude => {
            std::env::var("ANTHROPIC_API_KEY")
                .or_else(|_| std::env::var("CLAUDE_API_KEY"))
                .unwrap_or_default()
        }
        ModelProvider::OpenAI => {
            std::env::var("OPENAI_API_KEY").unwrap_or_default()
        }
        ModelProvider::Gemini => {
            // OAuth first, then env vars
            if let Ok(Some(token)) = crate::auth::resolve_gemini_token() {
                return Some(token);
            }
            std::env::var("GEMINI_API_KEY")
                .or_else(|_| std::env::var("LLM_API_KEY"))
                .unwrap_or_default()
        }
        ModelProvider::OpenAICompatible => {
            std::env::var("OPENAI_COMPAT_API_KEY")
                .or_else(|_| std::env::var("OPENAI_API_KEY"))
                .or_else(|_| std::env::var("LLM_API_KEY"))
                .unwrap_or_default()
        }
    };
    if key.is_empty() { None } else { Some(key) }
}

/// The central agentic engine that drives the tool-use loop.
///
/// Holds the LLM client(s), registered tools, cost tracker, and
/// permission state.  Call [`QueryEngine::query`] to run a full
/// agent turn (potentially multiple LLM round-trips).
pub struct QueryEngine {
    pub(crate) provider: ModelProvider,
    pub(crate) openai_client: Option<Client<OpenAIConfig>>,
    pub(crate) http_client: reqwest::Client,
    pub(crate) model: String,
    pub tools: Vec<Box<dyn Tool + Send + Sync>>,
    pub config: EngineConfig,
    pub cost_tracker: Arc<Mutex<CostTracker>>,
    /// Session permission rules (e.g. "always allow" decisions).
    pub permission_rules: Arc<Mutex<Vec<PermissionRule>>>,
    /// Working directory for path safety checks.
    pub cwd: std::path::PathBuf,
    /// Shared todo list state.
    pub todo_list: crate::tools::todo::SharedTodoList,
    /// Unified task registry for background shell and agent tasks.
    pub task_registry: crate::tasks::SharedTaskRegistry,
    /// Active session for auto-save persistence.
    pub session: Arc<Mutex<Session>>,
}

impl QueryEngine {
    /// Create a new QueryEngine specifying the provider and optional API overrides.
    ///
    /// `question_tx` is an optional channel sender for forwarding
    /// [`AskUserQuestionTool`] questions to the TUI.  Pass `None` in
    /// headless / bare mode.
    pub fn new(
        model: impl Into<String>,
        provider: ModelProvider,
        api_key: Option<String>,
        api_base: Option<String>,
        config: EngineConfig,
        question_tx: Option<crate::tools::ask_user::QuestionSender>,
    ) -> Result<Self> {
        let openai_client = match provider {
            // Claude has its own native HTTP path; no async_openai client needed.
            ModelProvider::Claude => None,
            _ => {
                let mut oai_config = OpenAIConfig::default();

                let resolved_api_key = api_key.unwrap_or_else(|| match provider {
                    ModelProvider::OpenAI => std::env::var("OPENAI_API_KEY").unwrap_or_default(),
                    ModelProvider::OpenAICompatible => {
                        std::env::var("OPENAI_COMPAT_API_KEY")
                            .or_else(|_| std::env::var("OPENAI_API_KEY"))
                            .or_else(|_| std::env::var("LLM_API_KEY"))
                            .unwrap_or_default()
                    }
                    ModelProvider::Gemini => {
                        // Try OAuth token first, then env vars.
                        crate::auth::resolve_gemini_token()
                            .ok()
                            .flatten()
                            .unwrap_or_else(|| {
                                std::env::var("GEMINI_API_KEY")
                                    .or_else(|_| std::env::var("LLM_API_KEY"))
                                    .unwrap_or_default()
                            })
                    }
                    _ => String::new(),
                });

                if resolved_api_key.is_empty() {
                    let env_var_desc = match provider {
                        ModelProvider::OpenAI => "OPENAI_API_KEY",
                        ModelProvider::OpenAICompatible => "OPENAI_COMPAT_API_KEY, OPENAI_API_KEY, or LLM_API_KEY",
                        ModelProvider::Gemini => "GEMINI_API_KEY (or use /login gemini)",
                        _ => unreachable!(),
                    };
                    return Err(anyhow!(
                        "Environment variable(s) {} required for {:?} provider",
                        env_var_desc,
                        provider
                    ));
                }

                oai_config = oai_config.with_api_key(resolved_api_key);

                let resolved_api_base = match provider {
                    ModelProvider::OpenAI => api_base,
                    ModelProvider::OpenAICompatible => api_base
                        .or_else(|| std::env::var("OPENAI_COMPAT_API_BASE").ok())
                        .or_else(|| std::env::var("OPENAI_API_BASE").ok()),
                    ModelProvider::Gemini => {
                        let base = std::env::var("GEMINI_API_BASE")
                            .unwrap_or_else(|_| "https://generativelanguage.googleapis.com".to_string());
                        Some(format!("{}/v1beta/openai", base.trim_end_matches('/')))
                    }
                    _ => None,
                };

                if let Some(base) = resolved_api_base {
                    oai_config = oai_config.with_api_base(base);
                }

                Some(Client::with_config(oai_config))
            }
        };

        let todo_list = crate::tools::todo::new_shared_todo_list();
        let (tools, task_registry) = crate::tools::registry::default_tools(todo_list.clone(), question_tx);
        let model_str: String = model.into();

        // Create a session for auto-save persistence
        let session_id = uuid::Uuid::new_v4().to_string();
        let session = Session::new(
            session_id,
            model_str.clone(),
            format!("{:?}", provider),
        );

        Ok(Self {
            provider,
            openai_client,
            http_client: reqwest::Client::new(),
            model: model_str,
            tools,
            config,
            cost_tracker: Arc::new(Mutex::new(CostTracker::new())),
            permission_rules: Arc::new(Mutex::new(Vec::new())),
            cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            todo_list,
            task_registry,
            session: Arc::new(Mutex::new(session)),
        })
    }

    /// Register an [`AgentTool`] that can spawn sub-agents with this engine's
    /// model and configuration.  Sub-agents do **not** inherit the AgentTool,
    /// preventing infinite recursion.
    ///
    /// Follows a builder pattern so it can be chained after [`QueryEngine::new`]:
    ///
    /// ```ignore
    /// let engine = QueryEngine::new(model, provider, None, None, config)?
    ///     .with_agent_tool();
    /// ```
    pub fn with_agent_tool(mut self) -> Self {
        use crate::engine::agent_tool::AgentTool;
        self.tools.push(Box::new(AgentTool::new(
            self.model.clone(),
            self.provider,
            self.config.clone(),
            self.task_registry.clone(),
        )));
        self
    }

    /// Looks up a tool by name or alias.
    pub(crate) fn find_tool(&self, tool_name: &str) -> Option<&(dyn Tool + Send + Sync)> {
        self.tools.iter().find_map(|tool| {
            if tool.name() == tool_name || tool.aliases().into_iter().any(|alias| alias == tool_name) {
                Some(tool.as_ref())
            } else {
                None
            }
        })
    }

    /// Checks permission for a tool invocation.
    ///
    /// Returns `Ok(true)` if allowed, `Ok(false)` if denied.
    /// When the decision is [`PermissionDecision::Ask`] and a TUI channel
    /// is available, sends a [`UiEvent::PermissionRequest`] and awaits
    /// the user's interactive response.
    pub(crate) async fn check_tool_permission(
        &self,
        tool: &(dyn Tool + Send + Sync),
        input: &Value,
        tx_ui: &Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>,
    ) -> Result<bool> {
        let rules = self.permission_rules.lock()
            .map(|r| r.clone())
            .unwrap_or_default();

        let decision = check_permission(
            tool,
            input,
            self.config.permission_mode,
            &self.cwd,
            &rules,
        );

        match decision {
            PermissionDecision::Allow => Ok(true),
            PermissionDecision::Deny { reason } => {
                info!("Permission denied for '{}': {}", tool.name(), reason);
                Ok(false)
            }
            PermissionDecision::Ask { tool_name, description } => {
                // In auto mode or bypass mode, allow
                if self.config.auto_mode {
                    return Ok(true);
                }

                // If we have a TUI channel, ask the user
                if let Some(ref tx) = tx_ui {
                    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
                    let _ = tx.send(crate::ui::app::UiEvent::PermissionRequest {
                        tool_name: tool_name.clone(),
                        description,
                        response_tx: resp_tx,
                    }).await;

                    match resp_rx.await {
                        Ok(crate::ui::app::PermissionResponse::Allow) => Ok(true),
                        Ok(crate::ui::app::PermissionResponse::AlwaysAllow) => {
                            // Add a permanent allow rule for this tool
                            if let Ok(mut rules) = self.permission_rules.lock() {
                                rules.push(PermissionRule {
                                    tool_name,
                                    pattern: None,
                                    behavior: crate::permissions::RuleBehavior::Allow,
                                });
                            }
                            Ok(true)
                        }
                        Ok(crate::ui::app::PermissionResponse::Deny) => Ok(false),
                        Err(_) => Ok(false), // Channel closed = deny
                    }
                } else {
                    // No TUI (bare/one-shot mode): deny by default for non-auto
                    info!("No TUI available for permission prompt, denying '{}'", tool_name);
                    Ok(false)
                }
            }
        }
    }

    /// Runs the full agentic loop for a single user query.
    ///
    /// 1. Builds the system prompt (memory, output styles, context).
    /// 2. Dispatches to the appropriate provider path.
    /// 3. Loops: LLM call -> tool dispatch -> feed results -> repeat.
    /// 4. Returns the model's final text answer.
    pub async fn query(&self, input: &str, tx_ui: Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>) -> Result<String> {
        info!("Sending query to {:?} model: {}", self.provider, self.model);

        // 1. Build the system memory prompt that teaches the Agent how to remember.
        let mut system_prompt = crate::mem::build_memory_prompt();

        // 1.5 Inject Output Styles from user's Markdown definitions
        let output_styles = crate::output_styles::build_styles_prompt();
        system_prompt.push_str(&output_styles);

        // 1.6 Inject context (CLAUDE.md, git status, system info)
        let context_prompt = crate::context::build_context_prompt(&self.cwd);
        if !context_prompt.is_empty() {
            system_prompt.push_str("\n\n");
            system_prompt.push_str(&context_prompt);
        }

        // 3. Setup ToolContext driven by EngineConfig
        let tool_names = self.tools.iter().map(|t| t.name().to_string()).collect();
        let session_id = self.session.lock()
            .map(|s| s.id.clone())
            .ok();
        let ctx = ToolContext {
            auto_mode: self.config.auto_mode,
            debug: self.config.debug,
            tools_available: tool_names,
            max_budget_usd: self.config.max_budget_usd,
            cwd: self.cwd.clone(),
            permission_mode: self.config.permission_mode,
            session_id,
            is_agent: false,
        };

        // Pre-compute context window for compaction
        let context_window = crate::engine::tokens::get_context_window(&self.model);

        match self.provider {
            ModelProvider::Claude => self.query_claude(input, &system_prompt, &ctx, tx_ui, context_window).await,
            ModelProvider::Gemini => self.query_gemini_compat(input, &system_prompt, &ctx, tx_ui, context_window).await,
            _ => self.query_openai_compatible(input, &system_prompt, &ctx, tx_ui, context_window).await,
        }
    }

    /// Persist new conversation messages to disk (append-only JSONL).
    ///
    /// Only appends messages added since the last save — no full rewrite.
    /// Errors are logged but do not abort the agentic loop.
    pub(crate) fn auto_save_session(&self, messages: &[Value]) {
        if let Ok(mut session) = self.session.lock() {
            // Replace in-memory messages and let Session.save() append only new ones.
            session.messages = messages.to_vec();
            if let Err(e) = session.save() {
                info!("Auto-save failed: {}", e);
            }
        }
    }
}

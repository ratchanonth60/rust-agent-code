//! Agentic query engine with multi-provider LLM support.
//!
//! [`QueryEngine`] is the core execution loop.  It sends a user query to
//! an LLM, inspects the response for tool-use requests, dispatches them,
//! feeds the results back, and repeats until the model produces a final
//! text answer.
//!
//! Supported providers:
//! - **Claude** — native Anthropic Messages API with SSE streaming
//! - **OpenAI / Gemini / OpenAI-compatible** — via [`async_openai`]

use anyhow::{anyhow, Context, Result};
use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestToolMessageArgs, ChatCompletionRequestUserMessageArgs, ChatCompletionTool, ChatCompletionToolArgs, ChatCompletionToolType, CreateChatCompletionRequestArgs, FunctionObjectArgs
    },
    Client,
};
use clap::ValueEnum;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::info;

use crate::engine::config::EngineConfig;
use crate::engine::cost_tracker::CostTracker;
use crate::permissions::{PermissionDecision, PermissionRule, check_permission};
use crate::tools::{
    fs::ReadFileTool, fs::WriteFileTool, bash::BashTool,
    edit::FileEditTool, glob_tool::GlobTool, grep_tool::GrepTool,
    todo::TodoWriteTool, sleep::SleepTool, web_fetch::WebFetchTool,
    ask_user::AskUserQuestionTool,
    Tool, ToolContext,
};

/// LLM provider selection.
///
/// Parsed from the `--provider` CLI flag via [`clap::ValueEnum`].
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ModelProvider {
    OpenAI,
    Gemini,
    Claude,
    OpenAICompatible,
}

/// The central agentic engine that drives the tool-use loop.
///
/// Holds the LLM client(s), registered tools, cost tracker, and
/// permission state.  Call [`QueryEngine::query`] to run a full
/// agent turn (potentially multiple LLM round-trips).
pub struct QueryEngine {
    provider: ModelProvider,
    openai_client: Option<Client<OpenAIConfig>>,
    http_client: reqwest::Client,
    model: String,
    pub tools: Vec<Box<dyn Tool + Send + Sync>>,
    pub config: EngineConfig,
    pub cost_tracker: Arc<Mutex<CostTracker>>,
    /// Session permission rules (e.g. "always allow" decisions).
    pub permission_rules: Arc<Mutex<Vec<PermissionRule>>>,
    /// Working directory for path safety checks.
    pub cwd: std::path::PathBuf,
    /// Shared todo list state.
    pub todo_list: crate::tools::todo::SharedTodoList,
}

impl QueryEngine {
    /// Create a new QueryEngine specifying the provider and optional API overrides.
    pub fn new(
        model: impl Into<String>,
        provider: ModelProvider,
        api_key: Option<String>,
        api_base: Option<String>,
        config: EngineConfig,
    ) -> Result<Self> {
        let openai_client = match provider {
            ModelProvider::Claude => None,
            _ => {
                let mut config = OpenAIConfig::default();

                let resolved_api_key = api_key.unwrap_or_else(|| match provider {
                    ModelProvider::OpenAI => std::env::var("OPENAI_API_KEY").unwrap_or_default(),
                    ModelProvider::Gemini => std::env::var("GEMINI_API_KEY").unwrap_or_default(),
                    ModelProvider::OpenAICompatible => {
                        std::env::var("OPENAI_COMPAT_API_KEY")
                            .or_else(|_| std::env::var("OPENAI_API_KEY"))
                            .or_else(|_| std::env::var("LLM_API_KEY"))
                            .unwrap_or_default()
                    }
                    ModelProvider::Claude => String::new(),
                });

                config = config.with_api_key(resolved_api_key);

                let resolved_api_base = match provider {
                    ModelProvider::OpenAI => api_base,
                    ModelProvider::Gemini => Some(
                        api_base.unwrap_or_else(|| {
                            "https://generativelanguage.googleapis.com/v1beta/openai/".to_string()
                        }),
                    ),
                    ModelProvider::OpenAICompatible => api_base
                        .or_else(|| std::env::var("OPENAI_COMPAT_API_BASE").ok())
                        .or_else(|| std::env::var("OPENAI_API_BASE").ok()),
                    ModelProvider::Claude => None,
                };

                if let Some(base) = resolved_api_base {
                    config = config.with_api_base(base);
                }

                Some(Client::with_config(config))
            }
        };

        let todo_list = crate::tools::todo::new_shared_todo_list();

        let tools: Vec<Box<dyn Tool + Send + Sync>> = vec![
            Box::new(ReadFileTool),
            Box::new(WriteFileTool),
            Box::new(BashTool),
            Box::new(FileEditTool),
            Box::new(GlobTool),
            Box::new(GrepTool),
            Box::new(TodoWriteTool { todos: todo_list.clone() }),
            Box::new(SleepTool),
            Box::new(WebFetchTool),
            Box::new(AskUserQuestionTool::new(None)), // TUI channel wired later if needed
        ];

        Ok(Self {
            provider,
            openai_client,
            http_client: reqwest::Client::new(),
            model: model.into(),
            tools,
            config,
            cost_tracker: Arc::new(Mutex::new(CostTracker::new())),
            permission_rules: Arc::new(Mutex::new(Vec::new())),
            cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            todo_list,
        })
    }

    /// Converts registered tools into the OpenAI function-calling schema.
    fn get_openai_tools(&self) -> Result<Vec<ChatCompletionTool>> {
        let mut ret = Vec::new();
        for tool in &self.tools {
            let func = FunctionObjectArgs::default()
                .name(tool.name())
                .description(tool.description())
                .parameters(tool.input_schema())
                .build()?;
            
            ret.push(
                ChatCompletionToolArgs::default()
                    .r#type(ChatCompletionToolType::Function)
                    .function(func)
                    .build()?
            );
        }
        Ok(ret)
    }

    /// Returns a reference to the OpenAI-compatible client, or an error
    /// if the engine was configured for a non-OpenAI provider.
    fn get_openai_client(&self) -> Result<&Client<OpenAIConfig>> {
        self.openai_client
            .as_ref()
            .ok_or_else(|| anyhow!("OpenAI-compatible client is not configured for this provider"))
    }

    /// Looks up a tool by name or alias.
    fn find_tool(&self, tool_name: &str) -> Option<&(dyn Tool + Send + Sync)> {
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
    async fn check_tool_permission(
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
    /// 3. Loops: LLM call → tool dispatch → feed results → repeat.
    /// 4. Returns the model's final text answer.
    pub async fn query(&self, input: &str, tx_ui: Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>) -> Result<String> {
        info!("Sending query to {:?} model: {}", self.provider, self.model);
        
        // 1. Build the system memory prompt that teaches the Agent how to remember.
        // This is equivalent to TS `buildMemoryLines()`.
        let mut system_prompt = crate::mem::build_memory_prompt();

        // 1.5 Inject Output Styles from user's Markdown definitions
        // This maps to TS `loadOutputStylesDir.ts`.
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
        let ctx = ToolContext {
            auto_mode: self.config.auto_mode,
            debug: self.config.debug,
            tools_available: tool_names,
            max_budget_usd: self.config.max_budget_usd,
        };

        // Pre-compute context window for compaction
        let context_window = crate::engine::tokens::get_context_window(&self.model);

        match self.provider {
            ModelProvider::Claude => self.query_claude(input, &system_prompt, &ctx, tx_ui, context_window).await,
            _ => self.query_openai_compatible(input, &system_prompt, &ctx, tx_ui, context_window).await,
        }
    }

    /// OpenAI-compatible agentic loop (OpenAI, Gemini, and compatible providers).
    async fn query_openai_compatible(
        &self,
        input: &str,
        system_prompt: &str,
        ctx: &ToolContext,
        tx_ui: Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>,
        context_window: u64,
    ) -> Result<String> {
        let mut messages: Vec<ChatCompletionRequestMessage> = vec![
            ChatCompletionRequestSystemMessageArgs::default()
                .content(system_prompt)
                .build()?
                .into(),
            ChatCompletionRequestUserMessageArgs::default()
                .content(input)
                .build()?
                .into(),
        ];

        let openai_tools = self.get_openai_tools()?;
        let client = self.get_openai_client()?;

        // ============================================
        // 4. THE AGENTIC TOOL EVALUATION LOOP
        // ============================================
        loop {
            // Microcompact: clear old tool results if approaching context limit
            {
                let est_tokens = messages.iter()
                    .map(|m| {
                        let s = serde_json::to_string(m).unwrap_or_default();
                        crate::engine::tokens::estimate_tokens(&s)
                    })
                    .sum::<u64>();
                if crate::engine::tokens::should_compact(est_tokens, context_window, 0.8) {
                    info!("Approaching context limit ({}/{} est. tokens), clearing old tool results", est_tokens, context_window);
                    // For OpenAI, we serialize to JSON, microcompact, then deserialize back
                    let mut json_msgs: Vec<Value> = messages.iter()
                        .map(|m| serde_json::to_value(m).unwrap_or_default())
                        .collect();
                    crate::engine::compaction::microcompact_openai(&mut json_msgs, 6);
                    // Re-serialize back (best-effort; if it fails, keep originals)
                    let compacted: Vec<ChatCompletionRequestMessage> = json_msgs.iter()
                        .filter_map(|v| serde_json::from_value(v.clone()).ok())
                        .collect();
                    if compacted.len() == messages.len() {
                        messages = compacted;
                    }
                }
            }

            let req = CreateChatCompletionRequestArgs::default()
                .max_tokens(self.config.max_tokens as u16)
                .model(&self.model)
                .messages(messages.clone())
                .tools(openai_tools.clone())
                .build()
                .context("Failed to construct Chat Request")?;

            let api_start = Instant::now();
            let response = client.chat().create(req).await?;
            let api_duration = api_start.elapsed().as_millis() as u64;

            // Track usage/cost from OpenAI response
            if let Some(ref usage) = response.usage {
                let input_tok = usage.prompt_tokens as u64;
                let output_tok = usage.completion_tokens as u64;
                let cost = calculate_cost(&self.model, input_tok, output_tok);
                if let Ok(mut tracker) = self.cost_tracker.lock() {
                    tracker.add_usage(&self.model, input_tok, output_tok, cost);
                    tracker.total_api_duration_ms += api_duration;
                }
            }

            let choice = response.choices.first().ok_or_else(|| anyhow!("No choices returned"))?;
            let message = &choice.message;

            // Append assistant's response to the conversation
            let mut asst_msg = ChatCompletionRequestAssistantMessageArgs::default();
            if let Some(ref content) = message.content {
                asst_msg.content(content.clone());
            }
            if let Some(ref tool_calls) = message.tool_calls {
                asst_msg.tool_calls(tool_calls.clone());
            }
            messages.push(asst_msg.build()?.into());

            // Check if there are tool calls to execute
            if let Some(ref tool_calls) = message.tool_calls {
                for call in tool_calls {
                    let func_name = &call.function.name;
                    let func_args = &call.function.arguments;
                    if let Some(tool) = self.find_tool(func_name) {
                        info!("Executing tool: {} with args: {}", func_name, func_args);

                        let args_val: Value = serde_json::from_str(func_args)?;

                        // Permission check
                        let allowed = self.check_tool_permission(tool, &args_val, &tx_ui).await?;
                        if !allowed {
                            messages.push(
                                ChatCompletionRequestToolMessageArgs::default()
                                    .tool_call_id(call.id.clone())
                                    .content(format!("Permission denied for tool '{}'.", func_name))
                                    .build()?
                                    .into()
                            );
                            continue;
                        }

                        if let Some(ref tx) = tx_ui {
                            let _ = tx
                                .send(crate::ui::app::UiEvent::ToolStarted(func_name.to_string()))
                                .await;
                        }

                        let exec_result = tool.call(args_val, ctx).await;

                        if let Some(ref tx) = tx_ui {
                            let _ = tx
                                .send(crate::ui::app::UiEvent::ToolFinished(func_name.to_string()))
                                .await;
                        }

                        let content = match exec_result {
                            Ok(res) => {
                                serde_json::to_string(&res.output).unwrap_or_else(|_| "success".to_string())
                            }
                            Err(e) => format!("Error executing tool: {}", e),
                        };

                        messages.push(
                            ChatCompletionRequestToolMessageArgs::default()
                                .tool_call_id(call.id.clone())
                                .content(content)
                                .build()?
                                .into()
                        );
                    } else {
                        messages.push(
                            ChatCompletionRequestToolMessageArgs::default()
                                .tool_call_id(call.id.clone())
                                .content(format!("Error: Tool '{}' not found.", func_name))
                                .build()?
                                .into(),
                        );
                    }
                }
            } else {
                // No tool calls, return purely text content
                return Ok(message.content.clone().unwrap_or_default());
            }
        }
    }

    /// Resolves the Anthropic API key from environment variables.
    fn get_claude_key(&self) -> String {
        std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("CLAUDE_API_KEY"))
            .or_else(|_| std::env::var("LLM_API_KEY"))
            .unwrap_or_default()
    }

    /// Returns the API base URL, defaulting to `https://api.anthropic.com`.
    fn get_claude_base(&self) -> String {
        std::env::var("ANTHROPIC_BASE_URL").unwrap_or_else(|_| "https://api.anthropic.com".to_string())
    }

    /// Converts registered tools into the Claude tool definition format.
    fn get_claude_tools(&self) -> Vec<ClaudeToolDefinition> {
        self.tools
            .iter()
            .map(|tool| ClaudeToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                input_schema: tool.input_schema(),
            })
            .collect()
    }

    /// Claude-native agentic loop (Anthropic Messages API).
    ///
    /// Supports both streaming (SSE) and non-streaming paths.
    async fn query_claude(
        &self,
        input: &str,
        system_prompt: &str,
        ctx: &ToolContext,
        tx_ui: Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>,
        context_window: u64,
    ) -> Result<String> {
        use crate::engine::streaming::{parse_claude_sse, parse_tool_input, StreamEvent};

        let api_key = self.get_claude_key();
        if api_key.is_empty() {
            return Err(anyhow!(
                "ANTHROPIC_API_KEY (or CLAUDE_API_KEY) is required for Claude provider"
            ));
        }

        let api_base = self.get_claude_base();
        let mut messages = vec![ClaudeMessage {
            role: "user".to_string(),
            content: vec![ClaudeContentBlock::Text {
                text: input.to_string(),
            }],
        }];

        let tools = self.get_claude_tools();
        let use_streaming = tx_ui.is_some(); // Stream when TUI is active

        loop {
            // Microcompact: clear old tool results if approaching context limit
            {
                let mut json_msgs: Vec<Value> = messages.iter()
                    .map(|m| serde_json::to_value(m).unwrap_or_default())
                    .collect();
                let est_tokens: u64 = json_msgs.iter()
                    .map(|v| crate::engine::tokens::estimate_tokens(&v.to_string()))
                    .sum();
                if crate::engine::tokens::should_compact(est_tokens, context_window, 0.8) {
                    info!("Approaching context limit ({}/{} est. tokens), clearing old tool results", est_tokens, context_window);
                    crate::engine::compaction::microcompact(&mut json_msgs, 6);
                    // Re-serialize back
                    let compacted: Vec<ClaudeMessage> = json_msgs.iter()
                        .filter_map(|v| serde_json::from_value(v.clone()).ok())
                        .collect();
                    if compacted.len() == messages.len() {
                        messages = compacted;
                    }
                }
            }

            let request_body = ClaudeMessagesRequest {
                model: self.model.clone(),
                max_tokens: self.config.max_tokens,
                system: Some(system_prompt.to_string()),
                messages: messages.clone(),
                tools: if tools.is_empty() { None } else { Some(tools.clone()) },
                tool_choice: if tools.is_empty() {
                    None
                } else {
                    Some(ClaudeToolChoice {
                        r#type: "auto".to_string(),
                    })
                },
                stream: if use_streaming { Some(true) } else { None },
            };

            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            headers.insert("x-api-key", HeaderValue::from_str(&api_key)?);
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));

            let endpoint = format!("{}/v1/messages", api_base.trim_end_matches('/'));
            let api_start = Instant::now();
            let response = self
                .http_client
                .post(&endpoint)
                .headers(headers)
                .json(&request_body)
                .send()
                .await?;
            let status = response.status();

            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(anyhow!("Claude API error {}: {}", status, body));
            }

            // ---- Streaming path ----
            if use_streaming {
                // Notify TUI that streaming started
                if let Some(ref tx) = tx_ui {
                    let _ = tx.send(crate::ui::app::UiEvent::StreamStart).await;
                }

                // Create a channel to forward stream events to the TUI
                let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);

                // Forward StreamEvent::TextDelta → UiEvent::StreamDelta in background
                let tx_ui_clone = tx_ui.clone();
                let forward_handle = tokio::spawn(async move {
                    while let Some(event) = stream_rx.recv().await {
                        if let Some(ref tx) = tx_ui_clone {
                            match &event {
                                StreamEvent::TextDelta(text) => {
                                    let _ = tx.send(crate::ui::app::UiEvent::StreamDelta(text.clone())).await;
                                }
                                StreamEvent::ToolUseStart { name, .. } => {
                                    let _ = tx.send(crate::ui::app::UiEvent::ToolStarted(name.clone())).await;
                                }
                                _ => {}
                            }
                        }
                    }
                });

                let streamed = parse_claude_sse(response, Some(&stream_tx)).await;
                drop(stream_tx); // Close channel so forward_handle finishes
                let _ = forward_handle.await;

                // Notify TUI that streaming ended
                if let Some(ref tx) = tx_ui {
                    let _ = tx.send(crate::ui::app::UiEvent::StreamEnd).await;
                }

                let streamed = streamed?;
                let api_duration = api_start.elapsed().as_millis() as u64;

                // Track cost
                let cost = calculate_cost(&self.model, streamed.input_tokens, streamed.output_tokens);
                if let Ok(mut tracker) = self.cost_tracker.lock() {
                    tracker.add_usage(&self.model, streamed.input_tokens, streamed.output_tokens, cost);
                    tracker.total_api_duration_ms += api_duration;
                }

                // Reconstruct assistant message content blocks
                let mut assistant_content = Vec::new();
                if !streamed.text.is_empty() {
                    assistant_content.push(ClaudeContentBlock::Text {
                        text: streamed.text.clone(),
                    });
                }
                for tu in &streamed.tool_uses {
                    assistant_content.push(ClaudeContentBlock::ToolUse {
                        id: tu.id.clone(),
                        name: tu.name.clone(),
                        input: parse_tool_input(&tu.input_json),
                    });
                }

                messages.push(ClaudeMessage {
                    role: "assistant".to_string(),
                    content: assistant_content,
                });

                // Execute tool calls if any
                if streamed.tool_uses.is_empty() {
                    return Ok(streamed.text);
                }

                let mut tool_result_blocks = Vec::new();
                for tu in &streamed.tool_uses {
                    let tool_input = parse_tool_input(&tu.input_json);
                    if let Some(tool) = self.find_tool(&tu.name) {
                        // Permission check
                        let allowed = self.check_tool_permission(tool, &tool_input, &tx_ui).await?;
                        if !allowed {
                            tool_result_blocks.push(ClaudeContentBlock::ToolResult {
                                tool_use_id: tu.id.clone(),
                                content: format!("Permission denied for tool '{}'.", tu.name),
                                is_error: Some(true),
                            });
                            continue;
                        }

                        let exec_result = tool.call(tool_input, ctx).await;

                        if let Some(ref tx) = tx_ui {
                            let _ = tx.send(crate::ui::app::UiEvent::ToolFinished(tu.name.clone())).await;
                        }

                        match exec_result {
                            Ok(res) => {
                                tool_result_blocks.push(ClaudeContentBlock::ToolResult {
                                    tool_use_id: tu.id.clone(),
                                    content: serde_json::to_string(&res.output)
                                        .unwrap_or_else(|_| "{}".to_string()),
                                    is_error: if res.is_error { Some(true) } else { None },
                                });
                            }
                            Err(e) => {
                                tool_result_blocks.push(ClaudeContentBlock::ToolResult {
                                    tool_use_id: tu.id.clone(),
                                    content: format!("Error executing tool: {}", e),
                                    is_error: Some(true),
                                });
                            }
                        }
                    } else {
                        tool_result_blocks.push(ClaudeContentBlock::ToolResult {
                            tool_use_id: tu.id.clone(),
                            content: format!("Error: Tool '{}' not found.", tu.name),
                            is_error: Some(true),
                        });
                    }
                }

                messages.push(ClaudeMessage {
                    role: "user".to_string(),
                    content: tool_result_blocks,
                });

            // ---- Non-streaming path (one-shot / bare mode) ----
            } else {
                let api_response: ClaudeMessagesResponse = response.json().await?;
                let api_duration = api_start.elapsed().as_millis() as u64;

                // Track cost
                if let Some(ref usage) = api_response.usage {
                    let cost = calculate_cost(&self.model, usage.input_tokens, usage.output_tokens);
                    if let Ok(mut tracker) = self.cost_tracker.lock() {
                        tracker.add_usage(&self.model, usage.input_tokens, usage.output_tokens, cost);
                        tracker.total_api_duration_ms += api_duration;
                    }
                }

                messages.push(ClaudeMessage {
                    role: "assistant".to_string(),
                    content: api_response.content.clone(),
                });

                let mut tool_result_blocks = Vec::new();
                for block in &api_response.content {
                    if let ClaudeContentBlock::ToolUse { id, name, input } = block {
                        if let Some(tool) = self.find_tool(name) {
                            // Permission check
                            let allowed = self.check_tool_permission(tool, input, &tx_ui).await?;
                            if !allowed {
                                tool_result_blocks.push(ClaudeContentBlock::ToolResult {
                                    tool_use_id: id.clone(),
                                    content: format!("Permission denied for tool '{}'.", name),
                                    is_error: Some(true),
                                });
                                continue;
                            }

                            let exec_result = tool.call(input.clone(), ctx).await;
                            match exec_result {
                                Ok(res) => {
                                    tool_result_blocks.push(ClaudeContentBlock::ToolResult {
                                        tool_use_id: id.clone(),
                                        content: serde_json::to_string(&res.output)
                                            .unwrap_or_else(|_| "{}".to_string()),
                                        is_error: if res.is_error { Some(true) } else { None },
                                    });
                                }
                                Err(e) => {
                                    tool_result_blocks.push(ClaudeContentBlock::ToolResult {
                                        tool_use_id: id.clone(),
                                        content: format!("Error executing tool: {}", e),
                                        is_error: Some(true),
                                    });
                                }
                            }
                        } else {
                            tool_result_blocks.push(ClaudeContentBlock::ToolResult {
                                tool_use_id: id.clone(),
                                content: format!("Error: Tool '{}' not found.", name),
                                is_error: Some(true),
                            });
                        }
                    }
                }

                if tool_result_blocks.is_empty() {
                    let final_text = api_response
                        .content
                        .into_iter()
                        .filter_map(|block| match block {
                            ClaudeContentBlock::Text { text } => Some(text),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    return Ok(final_text);
                }

                messages.push(ClaudeMessage {
                    role: "user".to_string(),
                    content: tool_result_blocks,
                });
            }
        }
    }
}

// ── Claude API wire types ──────────────────────────────────────────

/// Tool definition in the Claude Messages API format.
#[derive(Debug, Clone, Serialize)]
struct ClaudeToolDefinition {
    name: String,
    description: String,
    input_schema: Value,
}

/// Tool-choice selector (`"auto"` lets the model decide).
#[derive(Debug, Clone, Serialize)]
struct ClaudeToolChoice {
    r#type: String,
}

/// A single message in the Claude conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClaudeMessage {
    role: String,
    content: Vec<ClaudeContentBlock>,
}

/// Content block variants used in Claude messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClaudeContentBlock {
    /// Plain text output from the model or user.
    Text {
        text: String,
    },
    /// A tool invocation requested by the model.
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    /// The result of executing a tool, sent back to the model.
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

/// Request body for the Claude Messages API.
#[derive(Debug, Clone, Serialize)]
struct ClaudeMessagesRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<ClaudeMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ClaudeToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<ClaudeToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

/// Response body from the Claude Messages API.
#[derive(Debug, Clone, Deserialize)]
struct ClaudeMessagesResponse {
    content: Vec<ClaudeContentBlock>,
    #[serde(default)]
    usage: Option<ClaudeUsage>,
}

/// Token usage reported by the Claude API.
#[derive(Debug, Clone, Deserialize)]
struct ClaudeUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[allow(dead_code)]
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    #[allow(dead_code)]
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
}

// ── Cost calculation ───────────────────────────────────────────────

/// Per-model pricing in USD per million tokens.
struct ModelPricing {
    input_per_mtok: f64,
    output_per_mtok: f64,
}

/// Returns pricing for known model families.
///
/// Falls back to Claude Sonnet-tier pricing for unrecognised models.
fn get_model_pricing(model: &str) -> ModelPricing {
    let m = model.to_lowercase();
    if m.contains("opus") {
        // Claude Opus 4/4.1: $15/$75
        ModelPricing { input_per_mtok: 15.0, output_per_mtok: 75.0 }
    } else if m.contains("haiku") {
        // Claude Haiku: $1/$5
        ModelPricing { input_per_mtok: 1.0, output_per_mtok: 5.0 }
    } else if m.contains("sonnet") || m.contains("claude") {
        // Claude Sonnet: $3/$15
        ModelPricing { input_per_mtok: 3.0, output_per_mtok: 15.0 }
    } else if m.contains("gpt-4o-mini") {
        ModelPricing { input_per_mtok: 0.15, output_per_mtok: 0.60 }
    } else if m.contains("gpt-4o") || m.contains("gpt-4") {
        ModelPricing { input_per_mtok: 2.50, output_per_mtok: 10.0 }
    } else if m.contains("gemini") {
        // Gemini 2.5 Pro: free tier / $1.25/$10 for paid
        ModelPricing { input_per_mtok: 1.25, output_per_mtok: 10.0 }
    } else {
        // Default: moderate pricing
        ModelPricing { input_per_mtok: 3.0, output_per_mtok: 15.0 }
    }
}

/// Calculates the USD cost for a single API call.
fn calculate_cost(model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
    let pricing = get_model_pricing(model);
    (input_tokens as f64 / 1_000_000.0) * pricing.input_per_mtok
        + (output_tokens as f64 / 1_000_000.0) * pricing.output_per_mtok
}

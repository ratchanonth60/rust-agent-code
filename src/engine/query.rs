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
use futures_util::future::join_all;
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
            // Claude and Gemini have their own HTTP paths; no async_openai client needed.
            ModelProvider::Claude | ModelProvider::Gemini => None,
            _ => {
                let mut config = OpenAIConfig::default();

                let resolved_api_key = api_key.unwrap_or_else(|| match provider {
                    ModelProvider::OpenAI => std::env::var("OPENAI_API_KEY").unwrap_or_default(),
                    ModelProvider::OpenAICompatible => {
                        std::env::var("OPENAI_COMPAT_API_KEY")
                            .or_else(|_| std::env::var("OPENAI_API_KEY"))
                            .or_else(|_| std::env::var("LLM_API_KEY"))
                            .unwrap_or_default()
                    }
                    _ => String::new(),
                });

                if resolved_api_key.is_empty() {
                    let env_var_desc = match provider {
                        ModelProvider::OpenAI => "OPENAI_API_KEY",
                        ModelProvider::OpenAICompatible => "OPENAI_COMPAT_API_KEY, OPENAI_API_KEY, or LLM_API_KEY",
                        _ => unreachable!(),
                    };
                    return Err(anyhow!(
                        "Environment variable(s) {} required for {:?} provider",
                        env_var_desc,
                        provider
                    ));
                }

                config = config.with_api_key(resolved_api_key);

                let resolved_api_base = match provider {
                    ModelProvider::OpenAI => api_base,
                    ModelProvider::OpenAICompatible => api_base
                        .or_else(|| std::env::var("OPENAI_COMPAT_API_BASE").ok())
                        .or_else(|| std::env::var("OPENAI_API_BASE").ok()),
                    _ => None,
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
            ModelProvider::Gemini => self.query_gemini(input, &system_prompt, &ctx, tx_ui).await,
            _ => self.query_openai_compatible(input, &system_prompt, &ctx, tx_ui, context_window).await,
        }
    }

    /// Resolves the Gemini API key from environment variables.
    fn get_gemini_key(&self) -> String {
        std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("LLM_API_KEY"))
            .unwrap_or_default()
    }

    /// Gemini-native agentic loop using direct HTTP (not async_openai).
    ///
    /// Gemini's thinking models include a `thought_signature` in tool-use
    /// responses that must be echoed back in the following assistant message.
    /// `async_openai` has no awareness of this field, so we manage the
    /// conversation as raw JSON via our `http_client`.
    async fn query_gemini(
        &self,
        input: &str,
        system_prompt: &str,
        ctx: &ToolContext,
        tx_ui: Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>,
    ) -> Result<String> {
        let api_key = self.get_gemini_key();
        if api_key.is_empty() {
            return Err(anyhow!("GEMINI_API_KEY is required for Gemini provider"));
        }

        let gemini_base = std::env::var("GEMINI_API_BASE")
            .unwrap_or_else(|_| "https://generativelanguage.googleapis.com".to_string());
        let url = format!(
            "{}/v1beta/openai/chat/completions",
            gemini_base.trim_end_matches('/')
        );

        // Build tools in OpenAI function-calling format
        let tools: Vec<Value> = self.tools.iter().map(|t| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.input_schema()
                }
            })
        }).collect();

        let mut messages: Vec<Value> = vec![
            serde_json::json!({"role": "system", "content": system_prompt}),
            serde_json::json!({"role": "user",   "content": input}),
        ];

        loop {
            let request_body = serde_json::json!({
                "model":      &self.model,
                "max_tokens": self.config.max_tokens,
                "messages":   &messages,
                "tools":      &tools,
            });

            let api_start = Instant::now();
            let response = self.http_client
                .post(url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .json(&request_body)
                .send().await?;
            let api_duration = api_start.elapsed().as_millis() as u64;

            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(anyhow!("Gemini API error {}: {}", status, body));
            }

            let resp: Value = response.json().await?;

            // Track cost
            if let Some(usage) = resp.get("usage") {
                let input_tok  = usage["prompt_tokens"].as_u64().unwrap_or(0);
                let output_tok = usage["completion_tokens"].as_u64().unwrap_or(0);
                let cost = calculate_cost(&self.model, input_tok, output_tok);
                if let Ok(mut tracker) = self.cost_tracker.lock() {
                    tracker.add_usage(&self.model, input_tok, output_tok, cost);
                    tracker.total_api_duration_ms += api_duration;
                }
            }

            let msg = &resp["choices"][0]["message"];
            let content     = msg["content"].as_str().unwrap_or("").to_string();
            let thought_sig = msg["extra_content"]["google"]["thought_signature"]
                .as_str().map(String::from);
            let tool_calls  = msg["tool_calls"].as_array().cloned().unwrap_or_default();

            // Build assistant message, preserving thought_signature for thinking models
            let mut asst_msg = serde_json::json!({
                "role": "assistant",
                "content": if content.is_empty() { Value::Null } else { Value::String(content.clone()) },
                "tool_calls": &tool_calls,
            });
            if let Some(ts) = thought_sig {
                asst_msg["extra_content"] = serde_json::json!({"google": {"thought_signature": ts}});
            }
            messages.push(asst_msg);

            if tool_calls.is_empty() {
                return Ok(content);
            }

            // Execute all tool calls and collect results
            for tc in &tool_calls {
                let tc_id     = tc["id"].as_str().unwrap_or("").to_string();
                let tool_name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                let args_str  = tc["function"]["arguments"].as_str().unwrap_or("{}");
                let tool_input: Value = serde_json::from_str(args_str)
                    .unwrap_or_else(|_| Value::Object(serde_json::Map::new()));

                let result_content = if let Some(tool) = self.find_tool(&tool_name) {
                    if let Some(ref tx) = tx_ui {
                        let _ = tx.send(crate::ui::app::UiEvent::ToolStarted(tool_name.clone())).await;
                    }
                    let allowed = self.check_tool_permission(tool, &tool_input, &tx_ui).await?;
                    if !allowed {
                        format!("Permission denied for tool '{}'.", tool_name)
                    } else {
                        let exec = tool.call(tool_input, ctx).await;
                        if let Some(ref tx) = tx_ui {
                            let _ = tx.send(crate::ui::app::UiEvent::ToolFinished(tool_name.clone())).await;
                        }
                        match exec {
                            Ok(res) => serde_json::to_string(&res.output).unwrap_or_else(|_| "{}".to_string()),
                            Err(e)  => format!("Error executing tool: {}", e),
                        }
                    }
                } else {
                    format!("Error: Tool '{}' not found.", tool_name)
                };

                messages.push(serde_json::json!({
                    "role":         "tool",
                    "tool_call_id": tc_id,
                    "content":      result_content,
                }));
            }
        }
    }

    /// OpenAI-compatible agentic loop (OpenAI and compatible providers).
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

                let calls: Vec<ToolCall> = streamed.tool_uses.iter()
                    .map(|tu| ToolCall {
                        id: tu.id.clone(),
                        name: tu.name.clone(),
                        input: parse_tool_input(&tu.input_json),
                    })
                    .collect();
                let tool_result_blocks = self.execute_tools_parallel(&calls, ctx, &tx_ui).await?;

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

                let calls: Vec<ToolCall> = api_response.content.iter()
                    .filter_map(|block| {
                        if let ClaudeContentBlock::ToolUse { id, name, input } = block {
                            Some(ToolCall { id: id.clone(), name: name.clone(), input: input.clone() })
                        } else {
                            None
                        }
                    })
                    .collect();
                let tool_result_blocks = self.execute_tools_parallel(&calls, ctx, &tx_ui).await?;

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

    /// Execute a batch of tool calls: permission checks run sequentially (one TUI dialog at a
    /// time), then all approved tools execute concurrently.  Results are returned in the same
    /// order as `calls`.
    async fn execute_tools_parallel(
        &self,
        calls: &[ToolCall],
        ctx: &ToolContext,
        tx_ui: &Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>,
    ) -> Result<Vec<ClaudeContentBlock>> {
        // Phase 1: sequential permission checks (user prompts must not overlap).
        let mut permitted = Vec::with_capacity(calls.len());
        for call in calls {
            let allowed = match self.find_tool(&call.name) {
                None => false,
                Some(tool) => self.check_tool_permission(tool, &call.input, tx_ui).await?,
            };
            permitted.push(allowed);
        }

        // Phase 2: parallel execution — all approved tools run concurrently.
        let futs = calls.iter().zip(permitted.into_iter()).map(|(call, allowed)| {
            self.run_tool(allowed, &call.id, &call.name, call.input.clone(), ctx, tx_ui)
        });
        join_all(futs).await.into_iter().collect()
    }

    /// Execute a single tool whose permission has already been resolved.
    async fn run_tool(
        &self,
        allowed: bool,
        tool_use_id: &str,
        tool_name: &str,
        tool_input: Value,
        ctx: &ToolContext,
        tx_ui: &Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>,
    ) -> Result<ClaudeContentBlock> {
        if !allowed {
            return Ok(ClaudeContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: format!("Permission denied for tool '{}'.", tool_name),
                is_error: Some(true),
            });
        }

        let Some(tool) = self.find_tool(tool_name) else {
            return Ok(ClaudeContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: format!("Error: Tool '{}' not found.", tool_name),
                is_error: Some(true),
            });
        };

        let exec_result = tool.call(tool_input, ctx).await;
        if let Some(ref tx) = tx_ui {
            let _ = tx.send(crate::ui::app::UiEvent::ToolFinished(tool_name.to_string())).await;
        }

        match exec_result {
            Ok(res) => Ok(ClaudeContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: serde_json::to_string(&res.output).unwrap_or_else(|_| "{}".to_string()),
                is_error: if res.is_error { Some(true) } else { None },
            }),
            Err(e) => Ok(ClaudeContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: format!("Error executing tool: {}", e),
                is_error: Some(true),
            }),
        }
    }
}

// ── Claude API wire types ──────────────────────────────────────────

/// A pending tool invocation extracted from an LLM response.
struct ToolCall {
    id: String,
    name: String,
    input: Value,
}

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
#[derive(Clone, Copy)]
struct ModelPricing {
    input_per_mtok: f64,
    output_per_mtok: f64,
}

/// A row in the static pricing lookup table.
struct PricingEntry {
    /// Required substring (lowercased) in the model name.
    primary: &'static str,
    /// Optional second substring; both must match when set.
    secondary: Option<&'static str>,
    price: ModelPricing,
}

/// Static pricing table (USD / million tokens).
///
/// Scanned top-to-bottom; the first row whose pattern(s) all appear
/// in the lowercased model name wins.  Keep **more-specific** entries
/// above broader family catch-alls for the same provider.
///
/// # Adding a new model
///
/// Insert one row before the catch-all for its provider family:
/// ```text
/// PricingEntry { primary: "new-model-v3", secondary: None,
///     price: ModelPricing { input_per_mtok: 1.0, output_per_mtok: 4.0 } },
/// ```
///
/// Sources: Anthropic docs · Google AI pricing (2026-04).
static PRICING_TABLE: &[PricingEntry] = &[
    // ── Claude ─────────────────────────────────────────────────────
    PricingEntry { primary: "opus-4-6",    secondary: None,               price: ModelPricing { input_per_mtok:  5.00, output_per_mtok: 25.00 } },
    PricingEntry { primary: "opus-4-5",    secondary: None,               price: ModelPricing { input_per_mtok:  5.00, output_per_mtok: 25.00 } },
    PricingEntry { primary: "opus",        secondary: None,               price: ModelPricing { input_per_mtok: 15.00, output_per_mtok: 75.00 } },
    PricingEntry { primary: "haiku-4",     secondary: None,               price: ModelPricing { input_per_mtok:  1.00, output_per_mtok:  5.00 } },
    PricingEntry { primary: "haiku",       secondary: None,               price: ModelPricing { input_per_mtok:  0.25, output_per_mtok:  1.25 } },
    PricingEntry { primary: "sonnet",      secondary: None,               price: ModelPricing { input_per_mtok:  3.00, output_per_mtok: 15.00 } },
    PricingEntry { primary: "claude",      secondary: None,               price: ModelPricing { input_per_mtok:  3.00, output_per_mtok: 15.00 } },
    // ── Gemini ─────────────────────────────────────────────────────
    PricingEntry { primary: "gemini-3",    secondary: Some("pro"),        price: ModelPricing { input_per_mtok:  2.00, output_per_mtok: 12.00 } },
    PricingEntry { primary: "gemini-3",    secondary: Some("flash-lite"), price: ModelPricing { input_per_mtok:  0.25, output_per_mtok:  1.50 } },
    PricingEntry { primary: "gemini-3",    secondary: Some("flash"),      price: ModelPricing { input_per_mtok:  0.50, output_per_mtok:  3.00 } },
    PricingEntry { primary: "gemini-2.5",  secondary: Some("flash-lite"), price: ModelPricing { input_per_mtok:  0.10, output_per_mtok:  0.40 } },
    PricingEntry { primary: "gemini-2.5",  secondary: Some("flash"),      price: ModelPricing { input_per_mtok:  0.30, output_per_mtok:  2.50 } },
    PricingEntry { primary: "gemini-2.5",  secondary: None,               price: ModelPricing { input_per_mtok:  1.25, output_per_mtok: 10.00 } },
    PricingEntry { primary: "gemini",      secondary: Some("flash"),      price: ModelPricing { input_per_mtok:  0.10, output_per_mtok:  0.40 } },
    PricingEntry { primary: "gemini",      secondary: None,               price: ModelPricing { input_per_mtok:  1.25, output_per_mtok: 10.00 } },
    // ── OpenAI ─────────────────────────────────────────────────────
    PricingEntry { primary: "gpt-4o-mini", secondary: None,               price: ModelPricing { input_per_mtok:  0.15, output_per_mtok:  0.60 } },
    PricingEntry { primary: "gpt-4o",      secondary: None,               price: ModelPricing { input_per_mtok:  2.50, output_per_mtok: 10.00 } },
    PricingEntry { primary: "gpt-4",       secondary: None,               price: ModelPricing { input_per_mtok:  2.50, output_per_mtok: 10.00 } },
    PricingEntry { primary: "gpt-3.5",     secondary: None,               price: ModelPricing { input_per_mtok:  0.50, output_per_mtok:  1.50 } },
];

/// Looks up the price for `model` by scanning [`PRICING_TABLE`].
///
/// Falls back to Sonnet-tier ($3 / $15) for unrecognised models.
fn get_model_pricing(model: &str) -> ModelPricing {
    let m = model.to_lowercase();
    PRICING_TABLE
        .iter()
        .find(|e| m.contains(e.primary) && e.secondary.map_or(true, |s| m.contains(s)))
        .map_or(ModelPricing { input_per_mtok: 3.0, output_per_mtok: 15.0 }, |e| e.price)
}

/// Calculates the USD cost for a single API call.
fn calculate_cost(model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
    let p = get_model_pricing(model);
    (input_tokens as f64 / 1_000_000.0) * p.input_per_mtok
        + (output_tokens as f64 / 1_000_000.0) * p.output_per_mtok
}

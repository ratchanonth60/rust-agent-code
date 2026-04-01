use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;
use std::time::Instant;
use tracing::info;

use crate::engine::cost_tracker::CostTracker;
use crate::tools::{
    bash::BashTool, edit::FileEditTool, fs::ReadFileTool, fs::WriteFileTool,
    glob_tool::GlobTool, grep_tool::GrepTool, Tool, ToolContext,
};

/// Configuration flags that control [`QueryEngine`] behavior.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Skip interactive tool-use confirmation prompts.
    pub auto_mode: bool,
    /// Minimal UI chrome (no spinners, no color).
    pub bare_mode: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            auto_mode: false,
            bare_mode: false,
        }
    }
}

/// Supported LLM API providers.
///
/// All providers are accessed through the OpenAI-compatible chat completions
/// protocol. Each variant maps to a known API base URL.
pub enum ModelProvider {
    /// OpenAI — `https://api.openai.com/v1`
    OpenAI,
    /// Google Gemini — `https://generativelanguage.googleapis.com/v1beta/openai/`
    Gemini,
    /// OpenRouter — `https://openrouter.ai/api/v1`
    OpenRouter,
}

// ── Request / response types for the OpenAI-compatible API ──────────────

/// A single message in the chat conversation.
///
/// Uses `#[serde(flatten)]` to capture and re-emit provider-specific
/// fields (e.g. Gemini's `extra_content` containing `thought_signature`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    /// Captures unknown fields (e.g. `extra_content`) so they round-trip
    /// correctly when the assistant message is sent back to the API.
    #[serde(flatten)]
    extra: std::collections::HashMap<String, Value>,
}

/// A tool-call request from the assistant.
///
/// Uses `#[serde(flatten)]` to preserve Gemini's `extra_content`
/// (containing `thought_signature`) for round-tripping.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolCall {
    id: String,
    r#type: String,
    function: FunctionCall,
    /// Captures provider-specific fields (e.g. `extra_content`).
    #[serde(flatten)]
    extra: std::collections::HashMap<String, Value>,
}

/// Function name + serialised arguments inside a [`ToolCall`].
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FunctionCall {
    name: String,
    arguments: String,
}

/// Tool definition sent to the API.
#[derive(Debug, Clone, Serialize)]
struct ToolDef {
    r#type: String,
    function: FunctionDef,
}

/// Function schema inside a [`ToolDef`].
#[derive(Debug, Clone, Serialize)]
struct FunctionDef {
    name: String,
    description: String,
    parameters: Value,
}

/// The top-level chat completion request body.
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    tools: Vec<ToolDef>,
    max_tokens: u32,
}

/// Parsed chat completion response (ignores unknown fields like Gemini's `extra_content`).
#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<UsageInfo>,
}

/// A single choice in the response.
#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

/// Token usage counters.
#[derive(Debug, Deserialize)]
struct UsageInfo {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

/// The core agentic engine.
///
/// Owns an HTTP client, a set of [`Tool`] implementations, and a
/// [`CostTracker`]. The main entry point is [`QueryEngine::query`],
/// which drives the tool-use loop until the LLM produces a final text
/// response.
///
/// Uses raw `reqwest` + `serde_json` instead of `async-openai` to
/// tolerate non-standard fields (e.g. Gemini's `extra_content`).
///
/// # Architecture
///
/// ```text
/// User input
///   → build system prompt (memory + output styles)
///   → loop {
///       send messages to LLM
///       if tool_calls → dispatch each tool → append results → continue
///       else          → return text content
///     }
/// ```
pub struct QueryEngine {
    http: reqwest::Client,
    api_url: String,
    api_key: String,
    model: String,
    pub tools: Vec<Box<dyn Tool + Send + Sync>>,
    config: EngineConfig,
    pub cost_tracker: Mutex<CostTracker>,
}

impl QueryEngine {
    /// Creates a new engine for the given model and provider.
    ///
    /// The `api_key` is used for all providers. If `api_base` is `Some`,
    /// it overrides the provider's default base URL (useful for proxies
    /// or custom OpenAI-compatible endpoints).
    ///
    /// Automatically registers all built-in tools:
    /// [`ReadFileTool`], [`WriteFileTool`], [`FileEditTool`],
    /// [`BashTool`], [`GlobTool`], [`GrepTool`].
    pub fn new(
        model: impl Into<String>,
        provider: ModelProvider,
        config: EngineConfig,
        api_key: String,
        api_base: Option<String>,
    ) -> Self {
        let base = api_base.unwrap_or_else(|| match provider {
            ModelProvider::OpenAI => "https://api.openai.com/v1".to_string(),
            ModelProvider::Gemini => {
                "https://generativelanguage.googleapis.com/v1beta/openai".to_string()
            }
            ModelProvider::OpenRouter => "https://openrouter.ai/api/v1".to_string(),
        });

        let api_url = format!("{}/chat/completions", base.trim_end_matches('/'));

        let tools: Vec<Box<dyn Tool + Send + Sync>> = vec![
            Box::new(ReadFileTool),
            Box::new(WriteFileTool),
            Box::new(FileEditTool),
            Box::new(BashTool),
            Box::new(GlobTool),
            Box::new(GrepTool),
        ];

        Self {
            http: reqwest::Client::new(),
            api_url,
            api_key,
            model: model.into(),
            tools,
            config,
            cost_tracker: Mutex::new(CostTracker::new()),
        }
    }

    /// Builds the tool definitions array for the API request.
    fn build_tool_defs(&self) -> Vec<ToolDef> {
        self.tools
            .iter()
            .map(|t| ToolDef {
                r#type: "function".to_string(),
                function: FunctionDef {
                    name: t.name().to_string(),
                    description: t.description().to_string(),
                    parameters: t.input_schema(),
                },
            })
            .collect()
    }

    /// Runs the agentic tool-use loop until the LLM produces a final text answer.
    ///
    /// 1. Builds the system prompt from memory + output styles.
    /// 2. Sends the conversation to the LLM.
    /// 3. If the response contains `tool_calls`, dispatches each to the
    ///    matching [`Tool`], appends tool results, and re-sends.
    /// 4. Repeats until the LLM returns plain text (no tool calls).
    ///
    /// When `tx_ui` is `Some`, emits [`UiEvent`](crate::ui::app::UiEvent)
    /// messages so the TUI can show tool execution progress.
    ///
    /// # Errors
    ///
    /// Returns `Err` on network failures, malformed API responses,
    /// or if the LLM returns zero choices.
    pub async fn query(
        &self,
        input: &str,
        tx_ui: Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>,
    ) -> Result<String> {
        info!("Sending query to model: {} at {}", self.model, self.api_url);

        // Build system prompt: memory instructions + output styles
        let mut system_prompt = crate::mem::build_memory_prompt();
        let output_styles = crate::output_styles::build_styles_prompt();
        system_prompt.push_str(&output_styles);

        // Initial conversation: system + user messages
        let mut messages: Vec<ChatMessage> = vec![
            ChatMessage {
                role: "system".into(),
                content: Some(system_prompt),
                tool_calls: None,
                tool_call_id: None,
                extra: Default::default(),
            },
            ChatMessage {
                role: "user".into(),
                content: Some(input.to_string()),
                tool_calls: None,
                tool_call_id: None,
                extra: Default::default(),
            },
        ];

        let tool_defs = self.build_tool_defs();

        let ctx = ToolContext {
            auto_mode: self.config.auto_mode,
            debug: false,
            tools_available: self.tools.iter().map(|t| t.name().to_string()).collect(),
            max_budget_usd: None,
        };

        // === Agentic tool-use loop ===
        // Send → inspect → dispatch tools → append results → repeat
        loop {
            let body = ChatRequest {
                model: self.model.clone(),
                messages: messages.clone(),
                tools: tool_defs.clone(),
                max_tokens: 8192,
            };

            let api_start = Instant::now();

            let resp = self
                .http
                .post(&self.api_url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
                .context("API request failed — check your network connection")?;

            let status = resp.status();
            let resp_text = resp
                .text()
                .await
                .context("Failed to read API response body")?;

            if !status.is_success() {
                return Err(anyhow!(
                    "API returned HTTP {} — check your API_KEY and PROVIDER.\nResponse: {}",
                    status,
                    resp_text
                ));
            }

            let response: ChatResponse = serde_json::from_str(&resp_text).context(format!(
                "Failed to parse API response. Raw response:\n{}",
                &resp_text[..resp_text.len().min(500)]
            ))?;

            let api_duration = api_start.elapsed().as_millis() as u64;

            // Track token usage
            if let Some(ref usage) = response.usage {
                if let Ok(mut tracker) = self.cost_tracker.lock() {
                    tracker.total_api_duration_ms += api_duration;
                    tracker.add_usage(
                        &self.model,
                        usage.prompt_tokens,
                        usage.completion_tokens,
                        0.0,
                    );
                }
            }

            let choice = response
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("No choices returned"))?;

            let assistant_msg = choice.message;

            // Append assistant turn to history
            messages.push(assistant_msg.clone());

            // Dispatch tool calls, or return final text
            if let Some(ref tool_calls) = assistant_msg.tool_calls {
                if tool_calls.is_empty() {
                    return Ok(assistant_msg.content.unwrap_or_default());
                }

                for call in tool_calls {
                    let func_name = &call.function.name;
                    let func_args = &call.function.arguments;

                    let mut handled = false;
                    for tool in &self.tools {
                        if tool.name() == func_name {
                            info!("Executing tool: {} with args: {}", func_name, func_args);

                            if let Some(ref tx) = tx_ui {
                                let _ = tx
                                    .send(crate::ui::app::UiEvent::ToolStarted(
                                        func_name.to_string(),
                                    ))
                                    .await;
                            }

                            let args_val: Value = serde_json::from_str(func_args)?;

                            let tool_start = Instant::now();
                            let exec_result = tool.call(args_val, &ctx).await;
                            let tool_duration = tool_start.elapsed().as_millis() as u64;

                            if let Ok(mut tracker) = self.cost_tracker.lock() {
                                tracker.total_tool_duration_ms += tool_duration;
                            }

                            if let Some(ref tx) = tx_ui {
                                let _ = tx
                                    .send(crate::ui::app::UiEvent::ToolFinished(
                                        func_name.to_string(),
                                    ))
                                    .await;
                            }

                            let content = match exec_result {
                                Ok(res) => serde_json::to_string(&res.output)
                                    .unwrap_or_else(|_| "success".to_string()),
                                Err(e) => format!("Error executing tool: {}", e),
                            };

                            messages.push(ChatMessage {
                                role: "tool".into(),
                                content: Some(content),
                                tool_calls: None,
                                tool_call_id: Some(call.id.clone()),
                                extra: Default::default(),
                            });
                            handled = true;
                            break;
                        }
                    }

                    if !handled {
                        messages.push(ChatMessage {
                            role: "tool".into(),
                            content: Some(format!("Error: Tool '{}' not found.", func_name)),
                            tool_calls: None,
                            tool_call_id: Some(call.id.clone()),
                            extra: Default::default(),
                        });
                    }
                }
            } else {
                // No tool calls → final answer
                return Ok(assistant_msg.content.unwrap_or_default());
            }
        }
    }
}

use anyhow::{anyhow, Context, Result};
use async_openai::{
    types::{
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestToolMessageArgs, ChatCompletionRequestUserMessageArgs, ChatCompletionTool, ChatCompletionToolArgs, ChatCompletionToolType, CreateChatCompletionRequestArgs, FunctionObjectArgs
    },
    Client,
};
use serde_json::Value;
use std::sync::Mutex;
use std::time::Instant;
use tracing::info;

use crate::engine::cost_tracker::CostTracker;

use crate::tools::{fs::ReadFileTool, fs::WriteFileTool, bash::BashTool, edit::FileEditTool, glob_tool::GlobTool, grep_tool::GrepTool, Tool, ToolContext};

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
/// Both are accessed through the OpenAI-compatible chat completions
/// protocol — Gemini uses its `/v1beta/openai/` gateway.
pub enum ModelProvider {
    OpenAI,
    Gemini,
}

/// The core agentic engine.
///
/// Owns an LLM client, a set of [`Tool`] implementations, and a
/// [`CostTracker`]. The main entry point is [`QueryEngine::query`],
/// which drives the tool-use loop until the LLM produces a final text
/// response.
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
    client: Client<async_openai::config::OpenAIConfig>,
    model: String,
    pub tools: Vec<Box<dyn Tool + Send + Sync>>,
    config: EngineConfig,
    pub cost_tracker: Mutex<CostTracker>,
}

impl QueryEngine {
    /// Creates a new engine for the given model and provider.
    ///
    /// Automatically registers all built-in tools:
    /// [`ReadFileTool`], [`WriteFileTool`], [`FileEditTool`],
    /// [`BashTool`], [`GlobTool`], [`GrepTool`].
    ///
    /// # Panics
    ///
    /// Does **not** panic. Missing `GEMINI_API_KEY` env var silently
    /// defaults to an empty string (API calls will fail at runtime).
    pub fn new(model: impl Into<String>, provider: ModelProvider, config: EngineConfig) -> Self {
        let api_config = match provider {
            ModelProvider::OpenAI => {
                async_openai::config::OpenAIConfig::default()
            }
            ModelProvider::Gemini => {
                let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_else(|_| "".to_string());
                async_openai::config::OpenAIConfig::default()
                    .with_api_key(api_key)
                    .with_api_base("https://generativelanguage.googleapis.com/v1beta/openai/")
            }
        };

        let tools: Vec<Box<dyn Tool + Send + Sync>> = vec![
            Box::new(ReadFileTool),
            Box::new(WriteFileTool),
            Box::new(FileEditTool),
            Box::new(BashTool),
            Box::new(GlobTool),
            Box::new(GrepTool),
        ];

        Self {
            client: Client::with_config(api_config),
            model: model.into(),
            tools,
            config,
            cost_tracker: Mutex::new(CostTracker::new()),
        }
    }

    /// Converts registered [`Tool`]s into the OpenAI function-calling schema.
    ///
    /// # Errors
    ///
    /// Returns `Err` if any tool's schema fails to build via the
    /// `async_openai` builder API.
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
    pub async fn query(&self, input: &str, tx_ui: Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>) -> Result<String> {
        info!("Sending query to OpenAI model: {}", self.model);

        // Build system prompt: memory instructions + output styles
        let mut system_prompt = crate::mem::build_memory_prompt();
        let output_styles = crate::output_styles::build_styles_prompt();
        system_prompt.push_str(&output_styles);

        // Initial conversation: system + user messages
        let mut messages: Vec<ChatCompletionRequestMessage> = vec![
            ChatCompletionRequestSystemMessageArgs::default()
                .content(system_prompt)
                .build()?
                .into(),
            ChatCompletionRequestUserMessageArgs::default()
                .content(input)
                .build()?
                .into()
        ];

        let openai_tools = self.get_openai_tools()?;

        let ctx = ToolContext {
            auto_mode: self.config.auto_mode,
            debug: false,
            tools_available: self.tools.iter().map(|t| t.name().to_string()).collect(),
            max_budget_usd: None
        };

        // === Agentic tool-use loop ===
        // Send → inspect → dispatch tools → append results → repeat
        loop {
            let req = CreateChatCompletionRequestArgs::default()
                .max_tokens(8192u16)
                .model(&self.model)
                .messages(messages.clone())
                .tools(openai_tools.clone())
                .build()
                .context("Failed to construct Chat Request")?;

            let api_start = Instant::now();
            let response = self.client.chat().create(req).await?;
            let api_duration = api_start.elapsed().as_millis() as u64;

            // Track token usage
            if let Some(ref usage) = response.usage {
                let input_tokens = usage.prompt_tokens as u64;
                let output_tokens = usage.completion_tokens as u64;
                if let Ok(mut tracker) = self.cost_tracker.lock() {
                    tracker.total_api_duration_ms += api_duration;
                    tracker.add_usage(&self.model, input_tokens, output_tokens, 0.0);
                }
            }

            let choice = response.choices.first().ok_or_else(|| anyhow!("No choices returned"))?;
            let message = &choice.message;

            // Append assistant turn to history
            let mut asst_msg = ChatCompletionRequestAssistantMessageArgs::default();
            if let Some(ref content) = message.content {
                asst_msg.content(content.clone());
            }
            if let Some(ref tool_calls) = message.tool_calls {
                asst_msg.tool_calls(tool_calls.clone());
            }
            messages.push(asst_msg.build()?.into());

            // Dispatch tool calls, or return final text
            if let Some(ref tool_calls) = message.tool_calls {
                for call in tool_calls {
                    let func_name = &call.function.name;
                    let func_args = &call.function.arguments;

                    let mut handled = false;
                    for tool in &self.tools {
                        if tool.name() == func_name {
                            info!("Executing tool: {} with args: {}", func_name, func_args);

                            if let Some(ref tx) = tx_ui {
                                let _ = tx.send(crate::ui::app::UiEvent::ToolStarted(func_name.to_string())).await;
                            }

                            let args_val: Value = serde_json::from_str(func_args)?;

                            let tool_start = Instant::now();
                            let exec_result = tool.call(args_val, &ctx).await;
                            let tool_duration = tool_start.elapsed().as_millis() as u64;

                            if let Ok(mut tracker) = self.cost_tracker.lock() {
                                tracker.total_tool_duration_ms += tool_duration;
                            }

                            if let Some(ref tx) = tx_ui {
                                let _ = tx.send(crate::ui::app::UiEvent::ToolFinished(func_name.to_string())).await;
                            }

                            let content = match exec_result {
                                Ok(res) => serde_json::to_string(&res.output).unwrap_or_else(|_| "success".to_string()),
                                Err(e) => format!("Error executing tool: {}", e)
                            };

                            messages.push(
                                ChatCompletionRequestToolMessageArgs::default()
                                    .tool_call_id(call.id.clone())
                                    .content(content)
                                    .build()?
                                    .into()
                            );
                            handled = true;
                            break;
                        }
                    }

                    if !handled {
                        messages.push(
                            ChatCompletionRequestToolMessageArgs::default()
                                .tool_call_id(call.id.clone())
                                .content(format!("Error: Tool '{}' not found.", func_name))
                                .build()?
                                .into()
                        );
                    }
                }
            } else {
                // No tool calls → final answer
                return Ok(message.content.clone().unwrap_or_default());
            }
        }
    }
}

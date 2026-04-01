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
use tracing::info;

use crate::tools::{fs::ReadFileTool, fs::WriteFileTool, bash::BashTool, Tool, ToolContext};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ModelProvider {
    OpenAI,
    Gemini,
    Claude,
    OpenAICompatible,
}

pub struct QueryEngine {
    provider: ModelProvider,
    openai_client: Option<Client<OpenAIConfig>>,
    http_client: reqwest::Client,
    model: String,
    pub tools: Vec<Box<dyn Tool + Send + Sync>>,
}

impl QueryEngine {
    /// Create a new QueryEngine specifying the provider and optional API overrides.
    pub fn new(
        model: impl Into<String>,
        provider: ModelProvider,
        api_key: Option<String>,
        api_base: Option<String>,
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

        let tools: Vec<Box<dyn Tool + Send + Sync>> = vec![
            Box::new(ReadFileTool),
            Box::new(WriteFileTool),
            Box::new(BashTool),
        ];

        Ok(Self {
            provider,
            openai_client,
            http_client: reqwest::Client::new(),
            model: model.into(),
            tools,
        })
    }

    /// Helper to convert our Rust tools into OpenAI ChatCompletionTool format
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

    fn get_openai_client(&self) -> Result<&Client<OpenAIConfig>> {
        self.openai_client
            .as_ref()
            .ok_or_else(|| anyhow!("OpenAI-compatible client is not configured for this provider"))
    }

    fn find_tool(&self, tool_name: &str) -> Option<&(dyn Tool + Send + Sync)> {
        self.tools.iter().find_map(|tool| {
            if tool.name() == tool_name || tool.aliases().into_iter().any(|alias| alias == tool_name) {
                Some(tool.as_ref())
            } else {
                None
            }
        })
    }

    /// Executes the agent loop. It will call LLM, if tools are requested, it executes them and loops.
    pub async fn query(&self, input: &str, tx_ui: Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>) -> Result<String> {
        info!("Sending query to {:?} model: {}", self.provider, self.model);
        
        // 1. Build the system memory prompt that teaches the Agent how to remember.
        // This is equivalent to TS `buildMemoryLines()`.
        let mut system_prompt = crate::mem::build_memory_prompt();
        
        // 1.5 Inject Output Styles from user's Markdown definitions
        // This maps to TS `loadOutputStylesDir.ts`.
        let output_styles = crate::output_styles::build_styles_prompt();
        system_prompt.push_str(&output_styles);
        
        // 3. Setup ToolContext - later this will be driven by CLI args like --auto
        let ctx = ToolContext { 
            auto_mode: true, 
            debug: false, 
            tools_available: vec![], 
            max_budget_usd: None 
        }; // Hardcoded auto-mode for Phase 5 MVP

        match self.provider {
            ModelProvider::Claude => self.query_claude(input, &system_prompt, &ctx, tx_ui).await,
            _ => self.query_openai_compatible(input, &system_prompt, &ctx, tx_ui).await,
        }
    }

    async fn query_openai_compatible(
        &self,
        input: &str,
        system_prompt: &str,
        ctx: &ToolContext,
        tx_ui: Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>,
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
        // We will constantly ask the Model for a response.
        // If it asks to run Tools, we run them in Rust, append the results to the message list,
        // and loop back to the LLM to get an analysis of the Tool's output!
        loop {
            let req = CreateChatCompletionRequestArgs::default()
                .max_tokens(1024u16)
                .model(&self.model)
                .messages(messages.clone())
                .tools(openai_tools.clone())
                .build()
                .context("Failed to construct Chat Request")?;

            let response = client.chat().create(req).await?;
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

                        if let Some(ref tx) = tx_ui {
                            let _ = tx
                                .send(crate::ui::app::UiEvent::ToolStarted(func_name.to_string()))
                                .await;
                        }

                        let args_val: Value = serde_json::from_str(func_args)?;
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

    fn get_claude_key(&self) -> String {
        std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("CLAUDE_API_KEY"))
            .or_else(|_| std::env::var("LLM_API_KEY"))
            .unwrap_or_default()
    }

    fn get_claude_base(&self) -> String {
        std::env::var("ANTHROPIC_BASE_URL").unwrap_or_else(|_| "https://api.anthropic.com".to_string())
    }

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

    async fn query_claude(
        &self,
        input: &str,
        system_prompt: &str,
        ctx: &ToolContext,
        tx_ui: Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>,
    ) -> Result<String> {
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

        loop {
            let request_body = ClaudeMessagesRequest {
                model: self.model.clone(),
                max_tokens: 1024,
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
            };

            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            headers.insert("x-api-key", HeaderValue::from_str(&api_key)?);
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));

            let endpoint = format!("{}/v1/messages", api_base.trim_end_matches('/'));
            let response = self
                .http_client
                .post(endpoint)
                .headers(headers)
                .json(&request_body)
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(anyhow!("Claude API error {}: {}", status, body));
            }

            let api_response: ClaudeMessagesResponse = response.json().await?;
            messages.push(ClaudeMessage {
                role: "assistant".to_string(),
                content: api_response.content.clone(),
            });

            let mut tool_result_blocks = Vec::new();
            for block in &api_response.content {
                if let ClaudeContentBlock::ToolUse { id, name, input } = block {
                    if let Some(tool) = self.find_tool(name) {
                        if let Some(ref tx) = tx_ui {
                            let _ = tx
                                .send(crate::ui::app::UiEvent::ToolStarted(name.clone()))
                                .await;
                        }

                        let exec_result = tool.call(input.clone(), ctx).await;

                        if let Some(ref tx) = tx_ui {
                            let _ = tx
                                .send(crate::ui::app::UiEvent::ToolFinished(name.clone()))
                                .await;
                        }

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

#[derive(Debug, Clone, Serialize)]
struct ClaudeToolDefinition {
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Debug, Clone, Serialize)]
struct ClaudeToolChoice {
    r#type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClaudeMessage {
    role: String,
    content: Vec<ClaudeContentBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClaudeContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

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
}

#[derive(Debug, Clone, Deserialize)]
struct ClaudeMessagesResponse {
    content: Vec<ClaudeContentBlock>,
}

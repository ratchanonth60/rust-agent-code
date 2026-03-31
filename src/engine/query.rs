use anyhow::{anyhow, Context, Result};
use async_openai::{
    types::{
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestToolMessageArgs, ChatCompletionRequestUserMessageArgs, ChatCompletionTool, ChatCompletionToolArgs, ChatCompletionToolType, CreateChatCompletionRequestArgs, FunctionObjectArgs
    },
    Client,
};
use serde_json::Value;
use tracing::info;

use crate::tools::{fs::ReadFileTool, fs::WriteFileTool, bash::BashTool, Tool, ToolContext};

pub enum ModelProvider {
    OpenAI,
    Gemini,
}

pub struct QueryEngine {
    client: Client<async_openai::config::OpenAIConfig>,
    model: String,
    pub tools: Vec<Box<dyn Tool + Send + Sync>>,
}

impl QueryEngine {
    /// Create a new QueryEngine specifying the provider (OpenAI or Gemini)
    pub fn new(model: impl Into<String>, provider: ModelProvider) -> Self {
        let config = match provider {
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
            Box::new(BashTool),
        ];

        Self {
            client: Client::with_config(config),
            model: model.into(),
            tools,
        }
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

    /// Executes the agent loop. It will call LLM, if tools are requested, it executes them and loops.
    pub async fn query(&self, input: &str, tx_ui: Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>) -> Result<String> {
        info!("Sending query to OpenAI model: {}", self.model);
        
        // 1. Build the system memory prompt that teaches the Agent how to remember.
        // This is equivalent to TS `buildMemoryLines()`.
        let mut system_prompt = crate::mem::build_memory_prompt();
        
        // 1.5 Inject Output Styles from user's Markdown definitions
        // This maps to TS `loadOutputStylesDir.ts`.
        let output_styles = crate::output_styles::build_styles_prompt();
        system_prompt.push_str(&output_styles);
        
        // 2. Setup the initial Conversation History with System & User Instructions.
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
        
        // 3. Setup ToolContext - later this will be driven by CLI args like --auto
        let ctx = ToolContext { 
            auto_mode: true, 
            debug: false, 
            tools_available: vec![], 
            max_budget_usd: None 
        }; // Hardcoded auto-mode for Phase 5 MVP

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

            let response = self.client.chat().create(req).await?;
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
                    
                    let mut handled = false;
                    for tool in &self.tools {
                        if tool.name() == func_name {
                            info!("Executing tool: {} with args: {}", func_name, func_args);
                            
                            if let Some(ref tx) = tx_ui {
                                let _ = tx.send(crate::ui::app::UiEvent::ToolStarted(func_name.to_string())).await;
                            }

                            let args_val: Value = serde_json::from_str(func_args)?;
                            
                            let exec_result = tool.call(args_val, &ctx).await;

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
                // No tool calls, return purely text content
                return Ok(message.content.clone().unwrap_or_default());
            }
        }
    }
}

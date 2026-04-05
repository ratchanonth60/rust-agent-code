//! OpenAI / OpenAI-compatible provider agentic loop.
//!
//! Uses the [`async_openai`] crate for both streaming and non-streaming
//! chat completion requests. This provider path is also used for any
//! OpenAI-compatible endpoint (e.g. local models, Groq, Together AI).
//!
//! # Key functions
//!
//! - [`QueryEngine::query_openai_compatible`] — Full agentic loop with
//!   tool-use, streaming, microcompaction, and cost tracking.
//! - [`QueryEngine::get_openai_tools`] — Converts registered tools to
//!   the OpenAI function-calling schema.
//! - [`QueryEngine::get_openai_client`] — Returns the configured
//!   [`async_openai::Client`] reference.

use anyhow::{anyhow, Context, Result};
use async_openai::types::{
    ChatCompletionMessageToolCall, ChatCompletionRequestAssistantMessageArgs,
    ChatCompletionRequestMessage, ChatCompletionRequestToolMessageArgs,
    ChatCompletionStreamOptions, ChatCompletionTool, ChatCompletionToolArgs,
    ChatCompletionToolType, CreateChatCompletionRequestArgs, FunctionCall, FunctionObjectArgs,
};
use async_openai::{config::OpenAIConfig, Client};
use futures_util::StreamExt;
use serde_json::Value;
use std::time::Instant;
use tracing::info;

use crate::engine::pricing::calculate_cost;
use crate::engine::query::QueryEngine;
use crate::tools::ToolContext;

impl QueryEngine {
    /// Converts registered tools into the OpenAI function-calling schema.
    pub(crate) fn get_openai_tools(&self) -> Result<Vec<ChatCompletionTool>> {
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
    pub(crate) fn get_openai_client(&self) -> Result<&Client<OpenAIConfig>> {
        self.openai_client
            .as_ref()
            .ok_or_else(|| anyhow!("OpenAI-compatible client is not configured for this provider"))
    }

    /// OpenAI-compatible agentic loop (OpenAI and compatible providers).
    ///
    /// Handles both streaming (when TUI is active) and non-streaming paths.
    /// Accumulates tool calls from streamed chunks, executes tools sequentially,
    /// and loops until the model produces a final text answer.
    pub(crate) async fn query_openai_compatible(
        &self,
        input: &str,
        system_prompt: &str,
        ctx: &ToolContext,
        tx_ui: Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>,
        context_window: u64,
    ) -> Result<String> {
        let mut messages: Vec<ChatCompletionRequestMessage> = vec![
            async_openai::types::ChatCompletionRequestSystemMessageArgs::default()
                .content(system_prompt)
                .build()?
                .into(),
            async_openai::types::ChatCompletionRequestUserMessageArgs::default()
                .content(input)
                .build()?
                .into(),
        ];

        let openai_tools = self.get_openai_tools()?;
        let client = self.get_openai_client()?;

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
                    let mut json_msgs: Vec<Value> = messages.iter()
                        .map(|m| serde_json::to_value(m).unwrap_or_default())
                        .collect();
                    crate::engine::compaction::microcompact_openai(&mut json_msgs, 6);
                    let compacted: Vec<ChatCompletionRequestMessage> = json_msgs.iter()
                        .filter_map(|v| serde_json::from_value(v.clone()).ok())
                        .collect();
                    if compacted.len() == messages.len() {
                        messages = compacted;
                    }
                }
            }

            if tx_ui.is_some() {
                // ── Streaming path ───────────────────────────────────────────
                let req = CreateChatCompletionRequestArgs::default()
                    .max_tokens(self.config.max_tokens as u16)
                    .model(&self.model)
                    .messages(messages.clone())
                    .tools(openai_tools.clone())
                    .stream_options(ChatCompletionStreamOptions { include_usage: true })
                    .build()
                    .context("Failed to construct Chat Request")?;

                let api_start = Instant::now();
                let mut stream = client.chat().create_stream(req).await?;

                if let Some(ref tx) = tx_ui {
                    let _ = tx.send(crate::ui::app::UiEvent::StreamStart).await;
                }

                let mut accumulated_text = String::new();
                let mut tool_acc: std::collections::HashMap<i32, (String, String, String)> =
                    std::collections::HashMap::new();
                let mut usage_data: Option<(u64, u64)> = None;

                while let Some(chunk_result) = stream.next().await {
                    let chunk = chunk_result?;
                    if let Some(usage) = chunk.usage {
                        usage_data = Some((usage.prompt_tokens as u64, usage.completion_tokens as u64));
                    }
                    if let Some(choice) = chunk.choices.first() {
                        if let Some(ref content) = choice.delta.content {
                            accumulated_text.push_str(content);
                            if let Some(ref tx) = tx_ui {
                                let _ = tx.send(crate::ui::app::UiEvent::StreamDelta(content.clone())).await;
                            }
                        }
                        if let Some(ref delta_tcs) = choice.delta.tool_calls {
                            for dtc in delta_tcs {
                                let entry = tool_acc
                                    .entry(dtc.index)
                                    .or_insert_with(|| (String::new(), String::new(), String::new()));
                                if let Some(ref id) = dtc.id {
                                    entry.0 = id.clone();
                                }
                                if let Some(ref func) = dtc.function {
                                    if let Some(ref name) = func.name {
                                        if entry.1.is_empty() {
                                            if let Some(ref tx) = tx_ui {
                                                let _ = tx.send(crate::ui::app::UiEvent::ToolStarted(name.clone())).await;
                                            }
                                        }
                                        entry.1.push_str(name);
                                    }
                                    if let Some(ref args) = func.arguments {
                                        entry.2.push_str(args);
                                    }
                                }
                            }
                        }
                    }
                }
                drop(stream);
                let api_duration = api_start.elapsed().as_millis() as u64;

                if let Some(ref tx) = tx_ui {
                    let _ = tx.send(crate::ui::app::UiEvent::StreamEnd).await;
                }

                if let Some((input_tok, output_tok)) = usage_data {
                    let cost = calculate_cost(&self.model, input_tok, output_tok);
                    if let Ok(mut tracker) = self.cost_tracker.lock() {
                        tracker.add_usage(&self.model, input_tok, output_tok, cost);
                        tracker.total_api_duration_ms += api_duration;
                    }
                }

                let mut sorted_tcs: Vec<(i32, String, String, String)> = tool_acc
                    .into_iter()
                    .map(|(k, v)| (k, v.0, v.1, v.2))
                    .collect();
                sorted_tcs.sort_by_key(|e| e.0);

                if sorted_tcs.is_empty() {
                    return Ok(accumulated_text);
                }

                let tool_calls_for_msg: Vec<ChatCompletionMessageToolCall> = sorted_tcs
                    .iter()
                    .map(|(_, id, name, args)| ChatCompletionMessageToolCall {
                        id: id.clone(),
                        r#type: ChatCompletionToolType::Function,
                        function: FunctionCall { name: name.clone(), arguments: args.clone() },
                    })
                    .collect();
                let mut asst_builder = ChatCompletionRequestAssistantMessageArgs::default();
                if !accumulated_text.is_empty() {
                    asst_builder.content(accumulated_text);
                }
                asst_builder.tool_calls(tool_calls_for_msg);
                messages.push(asst_builder.build()?.into());

                for (_, id, name, args) in &sorted_tcs {
                    let tool_input: Value = serde_json::from_str(args)
                        .unwrap_or(Value::Object(serde_json::Map::new()));
                    let result_content = if let Some(tool) = self.find_tool(name) {
                        let allowed = self.check_tool_permission(tool, &tool_input, &tx_ui).await?;
                        if !allowed {
                            format!("Permission denied for tool '{}'.", name)
                        } else {
                            let exec = tool.call(tool_input, ctx).await;
                            if let Some(ref tx) = tx_ui {
                                let _ = tx.send(crate::ui::app::UiEvent::ToolFinished(name.clone())).await;
                            }
                            match exec {
                                Ok(res) => serde_json::to_string(&res.output)
                                    .unwrap_or_else(|_| "success".to_string()),
                                Err(e) => format!("Error executing tool: {}", e),
                            }
                        }
                    } else {
                        format!("Error: Tool '{}' not found.", name)
                    };
                    messages.push(
                        ChatCompletionRequestToolMessageArgs::default()
                            .tool_call_id(id.clone())
                            .content(result_content)
                            .build()?
                            .into(),
                    );
                }

                let json_msgs: Vec<Value> = messages.iter()
                    .filter_map(|m| serde_json::to_value(m).ok())
                    .collect();
                self.auto_save_session(&json_msgs);

            } else {
                // ── Non-streaming path ───────────────────────────────────────
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

                let mut asst_msg = ChatCompletionRequestAssistantMessageArgs::default();
                if let Some(ref content) = message.content {
                    asst_msg.content(content.clone());
                }
                if let Some(ref tool_calls) = message.tool_calls {
                    asst_msg.tool_calls(tool_calls.clone());
                }
                messages.push(asst_msg.build()?.into());

                if let Some(ref tool_calls) = message.tool_calls {
                    for call in tool_calls {
                        let func_name = &call.function.name;
                        let func_args = &call.function.arguments;
                        if let Some(tool) = self.find_tool(func_name) {
                            info!("Executing tool: {} with args: {}", func_name, func_args);
                            let args_val: Value = serde_json::from_str(func_args)?;
                            let allowed = self.check_tool_permission(tool, &args_val, &tx_ui).await?;
                            if !allowed {
                                messages.push(
                                    ChatCompletionRequestToolMessageArgs::default()
                                        .tool_call_id(call.id.clone())
                                        .content(format!("Permission denied for tool '{}'.", func_name))
                                        .build()?
                                        .into(),
                                );
                                continue;
                            }
                            let exec_result = tool.call(args_val, ctx).await;
                            let content = match exec_result {
                                Ok(res) => serde_json::to_string(&res.output)
                                    .unwrap_or_else(|_| "success".to_string()),
                                Err(e) => format!("Error executing tool: {}", e),
                            };
                            messages.push(
                                ChatCompletionRequestToolMessageArgs::default()
                                    .tool_call_id(call.id.clone())
                                    .content(content)
                                    .build()?
                                    .into(),
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
                    let json_msgs: Vec<Value> = messages.iter()
                        .filter_map(|m| serde_json::to_value(m).ok())
                        .collect();
                    self.auto_save_session(&json_msgs);
                } else {
                    return Ok(message.content.clone().unwrap_or_default());
                }
            }
        }
    }
}

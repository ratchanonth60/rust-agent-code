//! Claude provider — native Anthropic Messages API with SSE streaming.
//!
//! This module contains the full Claude agentic loop, wire types for the
//! Anthropic Messages API, and the parallel tool execution pipeline.
//!
//! # Key functions
//!
//! - [`QueryEngine::query_claude`] — Main agentic loop (streaming + non-streaming).
//! - [`QueryEngine::execute_tools_parallel`] — Permission-first, then concurrent execution.
//! - [`QueryEngine::run_tool`] — Single tool dispatch with error wrapping.
//!
//! # Wire types
//!
//! The Claude API uses a unique message format with content blocks (text,
//! tool_use, tool_result). These are defined here rather than in a shared
//! types module because they are specific to the Anthropic protocol.

use anyhow::{anyhow, Result};
use futures_util::future::join_all;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Instant;
use tracing::info;

use crate::engine::pricing::calculate_cost;
use crate::engine::query::QueryEngine;
use crate::engine::streaming::{parse_claude_sse, parse_tool_input, StreamEvent};
use crate::tools::ToolContext;

// ── Claude API wire types ──────────────────────────────────────────

/// A pending tool invocation extracted from an LLM response.
pub(crate) struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

/// Tool definition in the Claude Messages API format.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ClaudeToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Tool-choice selector (`"auto"` lets the model decide).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ClaudeToolChoice {
    pub r#type: String,
}

/// A single message in the Claude conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ClaudeMessage {
    pub role: String,
    pub content: Vec<ClaudeContentBlock>,
}

/// Content block variants used in Claude messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ClaudeContentBlock {
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

// ── QueryEngine methods for Claude ─────────────────────────────────

impl QueryEngine {
    /// Resolves authentication for the Anthropic API.
    ///
    /// Returns `(token_or_key, is_oauth)`. When `is_oauth` is true, the caller
    /// should use `Authorization: Bearer` + `anthropic-beta` header instead of
    /// the `x-api-key` header.
    pub(crate) fn get_claude_auth(&self) -> (String, bool) {
        // 1. OAuth token (from credentials.json) takes priority.
        if let Ok(Some(token)) = crate::auth::resolve_claude_token() {
            return (token, true);
        }
        // 2. Fall back to API key env vars.
        let key = std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("CLAUDE_API_KEY"))
            .unwrap_or_default();
        (key, false)
    }

    /// Returns the Anthropic API base URL.
    ///
    /// Respects `ANTHROPIC_API_BASE` for proxy / self-hosted setups;
    /// defaults to `https://api.anthropic.com`.
    pub(crate) fn get_claude_base(&self) -> String {
        std::env::var("ANTHROPIC_API_BASE")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string())
    }

    /// Converts registered tools into the Claude Messages API tool schema.
    pub(crate) fn get_claude_tools(&self) -> Vec<ClaudeToolDefinition> {
        self.tools
            .iter()
            .map(|t| ClaudeToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.input_schema(),
            })
            .collect()
    }

    /// Claude-native agentic loop with SSE streaming support.
    ///
    /// # Behaviour
    ///
    /// 1. Sends the initial user message to the Anthropic Messages API.
    /// 2. If streaming is enabled (TUI active), parses SSE events and
    ///    forwards text deltas / tool-start events to the UI channel.
    /// 3. When tool-use blocks are returned, executes them in parallel
    ///    via [`execute_tools_parallel`] and appends results.
    /// 4. Loops until the model produces a text-only response.
    pub(crate) async fn query_claude(
        &self,
        input: &str,
        system_prompt: &str,
        ctx: &ToolContext,
        tx_ui: Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>,
        context_window: u64,
    ) -> Result<String> {
        let (auth_value, is_oauth) = self.get_claude_auth();
        if auth_value.is_empty() {
            return Err(anyhow!(
                "No Claude authentication available.\n  \
                 Set ANTHROPIC_API_KEY or run /login claude for OAuth."
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
        let use_streaming = tx_ui.is_some();

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
            if is_oauth {
                headers.insert(
                    "Authorization",
                    HeaderValue::from_str(&format!("Bearer {}", auth_value))?,
                );
                headers.insert(
                    "anthropic-beta",
                    HeaderValue::from_static(crate::auth::claude_oauth_beta_header()),
                );
            } else {
                headers.insert("x-api-key", HeaderValue::from_str(&auth_value)?);
            }
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
                if let Some(ref tx) = tx_ui {
                    let _ = tx.send(crate::ui::app::UiEvent::StreamStart).await;
                }

                let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);

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
                drop(stream_tx);
                let _ = forward_handle.await;

                if let Some(ref tx) = tx_ui {
                    let _ = tx.send(crate::ui::app::UiEvent::StreamEnd).await;
                }

                let streamed = streamed?;
                let api_duration = api_start.elapsed().as_millis() as u64;

                let cost = calculate_cost(&self.model, streamed.input_tokens, streamed.output_tokens);
                if let Ok(mut tracker) = self.cost_tracker.lock() {
                    tracker.add_usage(&self.model, streamed.input_tokens, streamed.output_tokens, cost);
                    tracker.total_api_duration_ms += api_duration;
                }

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

                let json_msgs: Vec<Value> = messages.iter()
                    .filter_map(|m| serde_json::to_value(m).ok())
                    .collect();
                self.auto_save_session(&json_msgs);

            // ---- Non-streaming path (one-shot / bare mode) ----
            } else {
                let api_response: ClaudeMessagesResponse = response.json().await?;
                let api_duration = api_start.elapsed().as_millis() as u64;

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

                let json_msgs: Vec<Value> = messages.iter()
                    .filter_map(|m| serde_json::to_value(m).ok())
                    .collect();
                self.auto_save_session(&json_msgs);
            }
        }
    }

    /// Execute a batch of tool calls: permission checks run sequentially (one
    /// TUI dialog at a time), then all approved tools execute concurrently.
    ///
    /// Results are returned in the same order as `calls`.
    pub(crate) async fn execute_tools_parallel(
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

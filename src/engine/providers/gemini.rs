//! Gemini provider — raw HTTP to the OpenAI-compatible endpoint.
//!
//! Uses direct HTTP requests (not [`async_openai`]) so that opaque JSON
//! fields like `thought_signature` survive round-trips without being
//! stripped by typed deserialization.
//!
//! # Why raw HTTP?
//!
//! Gemini's thinking models (gemini-2.5-pro, gemini-2.5-flash) return
//! `extra_content.google.thought_signature` inside `tool_calls`. This
//! field must be echoed back on subsequent turns for multi-turn tool-use
//! correctness. Using `Vec<Value>` messages preserves these fields
//! transparently.
//!
//! # Key functions
//!
//! - [`QueryEngine::query_gemini_compat`] — Full agentic loop (streaming + non-streaming).
//! - [`QueryEngine::get_gemini_key`] — Resolves API key from env.
//! - [`QueryEngine::get_gemini_endpoint`] — Builds the chat completions URL.
//! - [`QueryEngine::get_tools_json`] — Converts tools to raw JSON schema.

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use serde_json::Value;
use std::time::Instant;
use tracing::info;

use crate::engine::pricing::calculate_cost;
use crate::engine::query::QueryEngine;
use crate::tools::ToolContext;

impl QueryEngine {
    /// Resolves the Gemini API key / OAuth token.
    ///
    /// Priority: OAuth credentials → `GEMINI_API_KEY` → `LLM_API_KEY`.
    pub(crate) fn get_gemini_key(&self) -> String {
        // 1. Try OAuth token from ~/.rust-agent/credentials.json.
        if let Ok(Some(token)) = crate::auth::resolve_gemini_token() {
            return token;
        }
        // 2. Fallback to environment variables (existing behaviour).
        std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("LLM_API_KEY"))
            .unwrap_or_default()
    }

    /// Returns the Gemini OpenAI-compat chat completions endpoint.
    pub(crate) fn get_gemini_endpoint(&self) -> String {
        let base = std::env::var("GEMINI_API_BASE")
            .unwrap_or_else(|_| "https://generativelanguage.googleapis.com".to_string());
        format!("{}/v1beta/openai/chat/completions", base.trim_end_matches('/'))
    }

    /// Converts registered tools to raw JSON (for providers using direct HTTP).
    pub(crate) fn get_tools_json(&self) -> Vec<Value> {
        self.tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name(),
                        "description": t.description(),
                        "parameters": t.input_schema(),
                    }
                })
            })
            .collect()
    }

    /// Gemini-native agentic loop using raw HTTP to the OpenAI-compat endpoint.
    ///
    /// Stores messages as [`Value`] so that the `thought_signature` fields
    /// returned by thinking models (gemini-2.5-pro, gemini-2.5-flash) are
    /// preserved and echoed back on subsequent turns, which is required for
    /// multi-turn tool-use correctness.
    pub(crate) async fn query_gemini_compat(
        &self,
        input: &str,
        system_prompt: &str,
        ctx: &ToolContext,
        tx_ui: Option<tokio::sync::mpsc::Sender<crate::ui::app::UiEvent>>,
        context_window: u64,
    ) -> Result<String> {
        let api_key = self.get_gemini_key();
        if api_key.is_empty() {
            return Err(anyhow!(
                "GEMINI_API_KEY (or LLM_API_KEY) is required for Gemini provider"
            ));
        }

        let endpoint = self.get_gemini_endpoint();
        let tools_json = self.get_tools_json();
        let use_streaming = tx_ui.is_some();

        // Messages stored as Value so thought_signature fields survive round-trips.
        let mut messages: Vec<Value> = vec![
            serde_json::json!({"role": "system", "content": system_prompt}),
            serde_json::json!({"role": "user", "content": input}),
        ];

        loop {
            // Microcompact when approaching context limit.
            {
                let est_tokens: u64 = messages
                    .iter()
                    .map(|v| crate::engine::tokens::estimate_tokens(&v.to_string()))
                    .sum();
                if crate::engine::tokens::should_compact(est_tokens, context_window, 0.8) {
                    info!(
                        "Approaching context limit ({}/{} est. tokens), clearing old tool results",
                        est_tokens, context_window
                    );
                    crate::engine::compaction::microcompact_openai(&mut messages, 6);
                }
            }

            let mut request_body = serde_json::json!({
                "model": self.model,
                "max_tokens": self.config.max_tokens,
                "messages": messages,
                "tools": tools_json,
                "stream": use_streaming,
            });
            if use_streaming {
                request_body["stream_options"] = serde_json::json!({"include_usage": true});
            }

            let api_start = Instant::now();
            let response = self
                .http_client
                .post(&endpoint)
                .bearer_auth(&api_key)
                .json(&request_body)
                .send()
                .await?;

            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(anyhow!("Gemini API error {}: {}", status, body));
            }

            if use_streaming {
                // ── Streaming path ───────────────────────────────────────────
                if let Some(ref tx) = tx_ui {
                    let _ = tx.send(crate::ui::app::UiEvent::StreamStart).await;
                }

                let mut accumulated_text = String::new();
                // index -> (id, name, args, thought_signature)
                let mut tool_acc: std::collections::HashMap<
                    i32,
                    (String, String, String, String),
                > = std::collections::HashMap::new();
                let mut usage_data: Option<(u64, u64)> = None;
                let mut byte_stream = response.bytes_stream();
                let mut sse_buf = String::new();

                while let Some(chunk_result) = byte_stream.next().await {
                    let chunk = chunk_result.context("Gemini stream read error")?;
                    sse_buf.push_str(&String::from_utf8_lossy(&chunk));

                    // Process complete SSE events (delimited by "\n\n").
                    while let Some(pos) = sse_buf.find("\n\n") {
                        let event = sse_buf[..pos].to_string();
                        sse_buf = sse_buf[pos + 2..].to_string();

                        for line in event.lines() {
                            let Some(json_str) = line.strip_prefix("data: ") else {
                                continue;
                            };
                            if json_str == "[DONE]" {
                                continue;
                            }

                            let Ok(chunk_json) = serde_json::from_str::<Value>(json_str) else {
                                continue;
                            };

                            // Track usage (arrives on the last chunk).
                            if let Some(usage) = chunk_json.get("usage") {
                                let inp = usage
                                    .get("prompt_tokens")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0);
                                let out = usage
                                    .get("completion_tokens")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0);
                                if inp > 0 || out > 0 {
                                    usage_data = Some((inp, out));
                                }
                            }

                            if let Some(choices) =
                                chunk_json.get("choices").and_then(|v| v.as_array())
                            {
                                if let Some(choice) = choices.first() {
                                    let delta = &choice["delta"];

                                    // Accumulate assistant text.
                                    if let Some(content) =
                                        delta.get("content").and_then(|v| v.as_str())
                                    {
                                        if !content.is_empty() {
                                            accumulated_text.push_str(content);
                                            if let Some(ref tx) = tx_ui {
                                                let _ = tx
                                                    .send(crate::ui::app::UiEvent::StreamDelta(
                                                        content.to_string(),
                                                    ))
                                                    .await;
                                            }
                                        }
                                    }

                                    // Accumulate tool calls + thought_signatures.
                                    if let Some(tcs) =
                                        delta.get("tool_calls").and_then(|v| v.as_array())
                                    {
                                        for tc in tcs {
                                            let idx = tc
                                                .get("index")
                                                .and_then(|v| v.as_i64())
                                                .unwrap_or(0)
                                                as i32;
                                            let entry = tool_acc.entry(idx).or_insert_with(|| {
                                                (
                                                    String::new(),
                                                    String::new(),
                                                    String::new(),
                                                    String::new(),
                                                )
                                            });
                                            if let Some(id) =
                                                tc.get("id").and_then(|v| v.as_str())
                                            {
                                                entry.0 = id.to_string();
                                            }
                                            if let Some(func) = tc.get("function") {
                                                if let Some(name) =
                                                    func.get("name").and_then(|v| v.as_str())
                                                {
                                                    if entry.1.is_empty() {
                                                        if let Some(ref tx) = tx_ui {
                                                            let _ = tx
                                                                .send(crate::ui::app::UiEvent::ToolStarted(
                                                                    name.to_string(),
                                                                ))
                                                                .await;
                                                        }
                                                    }
                                                    entry.1.push_str(name);
                                                }
                                                if let Some(args) = func
                                                    .get("arguments")
                                                    .and_then(|v| v.as_str())
                                                {
                                                    entry.2.push_str(args);
                                                }
                                            }
                                            // Accumulate thought_signature across chunks.
                                            if let Some(sig) = tc
                                                .get("extra_content")
                                                .and_then(|ec| ec.get("google"))
                                                .and_then(|g| g.get("thought_signature"))
                                                .and_then(|v| v.as_str())
                                            {
                                                entry.3.push_str(sig);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let api_duration = api_start.elapsed().as_millis() as u64;

                if let Some(ref tx) = tx_ui {
                    let _ = tx.send(crate::ui::app::UiEvent::StreamEnd).await;
                }

                if let Some((inp, out)) = usage_data {
                    let cost = calculate_cost(&self.model, inp, out);
                    if let Ok(mut tracker) = self.cost_tracker.lock() {
                        tracker.add_usage(&self.model, inp, out, cost);
                        tracker.total_api_duration_ms += api_duration;
                    }
                }

                // Build sorted list of accumulated tool calls.
                let mut sorted_tcs: Vec<(i32, String, String, String, String)> = tool_acc
                    .into_iter()
                    .map(|(k, v)| (k, v.0, v.1, v.2, v.3))
                    .collect();
                sorted_tcs.sort_by_key(|e| e.0);

                if sorted_tcs.is_empty() {
                    return Ok(accumulated_text);
                }

                // Reconstruct assistant message with thought_signatures preserved.
                let tool_calls_json: Vec<Value> = sorted_tcs
                    .iter()
                    .map(|(_, id, name, args, sig)| {
                        let mut tc = serde_json::json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": args,
                            }
                        });
                        if !sig.is_empty() {
                            tc["extra_content"] =
                                serde_json::json!({"google": {"thought_signature": sig}});
                        }
                        tc
                    })
                    .collect();

                let mut asst_msg = serde_json::json!({
                    "role": "assistant",
                    "tool_calls": tool_calls_json,
                });
                if !accumulated_text.is_empty() {
                    asst_msg["content"] = serde_json::json!(accumulated_text);
                }
                messages.push(asst_msg);

                for (_, id, name, args, _) in &sorted_tcs {
                    let tool_input: Value = serde_json::from_str(args)
                        .unwrap_or(Value::Object(serde_json::Map::new()));
                    let result_content = if let Some(tool) = self.find_tool(name) {
                        let allowed =
                            self.check_tool_permission(tool, &tool_input, &tx_ui).await?;
                        if !allowed {
                            format!("Permission denied for tool '{}'.", name)
                        } else {
                            let exec = tool.call(tool_input, ctx).await;
                            if let Some(ref tx) = tx_ui {
                                let _ = tx
                                    .send(crate::ui::app::UiEvent::ToolFinished(name.clone()))
                                    .await;
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
                    messages.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": id,
                        "content": result_content,
                    }));
                }

                self.auto_save_session(&messages);
            } else {
                // ── Non-streaming path ───────────────────────────────────────
                // Parse response as raw JSON to preserve thought_signature fields.
                let resp_json: Value = response.json().await?;
                let api_duration = api_start.elapsed().as_millis() as u64;

                if let Some(usage) = resp_json.get("usage") {
                    let inp = usage
                        .get("prompt_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let out = usage
                        .get("completion_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let cost = calculate_cost(&self.model, inp, out);
                    if let Ok(mut tracker) = self.cost_tracker.lock() {
                        tracker.add_usage(&self.model, inp, out, cost);
                        tracker.total_api_duration_ms += api_duration;
                    }
                }

                let message = resp_json
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"))
                    .ok_or_else(|| anyhow!("No choices in Gemini response"))?
                    .clone();

                // Push raw message as assistant turn (preserves thought_signature).
                let mut asst_msg = message.clone();
                asst_msg["role"] = serde_json::json!("assistant");
                messages.push(asst_msg);

                let tool_calls = message
                    .get("tool_calls")
                    .and_then(|v| v.as_array())
                    .cloned();

                if let Some(tool_calls_arr) = tool_calls {
                    for tc in &tool_calls_arr {
                        let call_id = tc
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let func_name = tc["function"]["name"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        let func_args = tc["function"]["arguments"]
                            .as_str()
                            .unwrap_or("{}")
                            .to_string();

                        let result_content = if let Some(tool) = self.find_tool(&func_name) {
                            let args_val: Value = serde_json::from_str(&func_args)
                                .unwrap_or(Value::Object(serde_json::Map::new()));
                            let allowed =
                                self.check_tool_permission(tool, &args_val, &tx_ui).await?;
                            if !allowed {
                                format!("Permission denied for tool '{}'.", func_name)
                            } else {
                                match tool.call(args_val, ctx).await {
                                    Ok(res) => serde_json::to_string(&res.output)
                                        .unwrap_or_else(|_| "success".to_string()),
                                    Err(e) => format!("Error executing tool: {}", e),
                                }
                            }
                        } else {
                            format!("Error: Tool '{}' not found.", func_name)
                        };

                        messages.push(serde_json::json!({
                            "role": "tool",
                            "tool_call_id": call_id,
                            "content": result_content,
                        }));
                    }
                    self.auto_save_session(&messages);
                } else {
                    let text = message
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    return Ok(text);
                }
            }
        }
    }
}

use anyhow::Result;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;

/// Events emitted during streaming response parsing.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A chunk of text content from the model.
    TextDelta(String),
    /// A tool_use block has started.
    ToolUseStart { index: usize, id: String, name: String },
    /// A chunk of JSON input for the current tool_use block.
    ToolUseInputDelta(String),
    /// The response is complete with optional usage info.
    MessageStop { input_tokens: u64, output_tokens: u64 },
    /// An error occurred during streaming.
    Error(String),
}

/// Accumulated tool use from streaming.
#[derive(Debug, Clone)]
pub struct StreamedToolUse {
    pub id: String,
    pub name: String,
    pub input_json: String,
}

/// Accumulated response from streaming.
#[derive(Debug, Clone)]
pub struct StreamedResponse {
    pub text: String,
    pub tool_uses: Vec<StreamedToolUse>,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// SSE event types from Claude API.
#[derive(Debug, Deserialize)]
struct SseData {
    r#type: String,
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    content_block: Option<ContentBlockStart>,
    #[serde(default)]
    delta: Option<DeltaBlock>,
    #[serde(default)]
    usage: Option<UsageBlock>,
    #[serde(default)]
    message: Option<MessageBlock>,
}

#[derive(Debug, Deserialize)]
struct ContentBlockStart {
    r#type: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeltaBlock {
    r#type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageBlock {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct MessageBlock {
    #[serde(default)]
    usage: Option<UsageBlock>,
}

/// Parse a streaming SSE response from the Claude Messages API.
///
/// Reads the response body as a byte stream, parses SSE events, accumulates
/// the full response, and sends `StreamEvent`s to the provided channel.
///
/// Returns the accumulated `StreamedResponse` with full text and tool uses.
pub async fn parse_claude_sse(
    response: reqwest::Response,
    tx: Option<&mpsc::Sender<StreamEvent>>,
) -> Result<StreamedResponse> {
    let mut result = StreamedResponse {
        text: String::new(),
        tool_uses: Vec::new(),
        input_tokens: 0,
        output_tokens: 0,
    };

    // Current tool use being accumulated
    let mut current_tool: Option<StreamedToolUse> = None;

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Process complete SSE events from the buffer
        while let Some(event_end) = buffer.find("\n\n") {
            let event_block = buffer[..event_end].to_string();
            buffer = buffer[event_end + 2..].to_string();

            // Parse the SSE event
            let mut data_str = String::new();
            for line in event_block.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    data_str.push_str(data);
                }
            }

            if data_str.is_empty() || data_str == "[DONE]" {
                continue;
            }

            let sse_data: SseData = match serde_json::from_str(&data_str) {
                Ok(d) => d,
                Err(_) => continue,
            };

            match sse_data.r#type.as_str() {
                "message_start" => {
                    if let Some(msg) = &sse_data.message {
                        if let Some(usage) = &msg.usage {
                            result.input_tokens = usage.input_tokens;
                        }
                    }
                }

                "content_block_start" => {
                    if let Some(block) = &sse_data.content_block {
                        match block.r#type.as_str() {
                            "tool_use" => {
                                let id = block.id.clone().unwrap_or_default();
                                let name = block.name.clone().unwrap_or_default();
                                let index = sse_data.index.unwrap_or(0);
                                current_tool = Some(StreamedToolUse {
                                    id: id.clone(),
                                    name: name.clone(),
                                    input_json: String::new(),
                                });
                                if let Some(tx) = tx {
                                    let _ = tx.send(StreamEvent::ToolUseStart { index, id, name }).await;
                                }
                            }
                            _ => {} // text blocks start empty
                        }
                    }
                }

                "content_block_delta" => {
                    if let Some(delta) = &sse_data.delta {
                        match delta.r#type.as_str() {
                            "text_delta" => {
                                if let Some(text) = &delta.text {
                                    result.text.push_str(text);
                                    if let Some(tx) = tx {
                                        let _ = tx.send(StreamEvent::TextDelta(text.clone())).await;
                                    }
                                }
                            }
                            "input_json_delta" => {
                                if let Some(json) = &delta.partial_json {
                                    if let Some(ref mut tool) = current_tool {
                                        tool.input_json.push_str(json);
                                    }
                                    if let Some(tx) = tx {
                                        let _ = tx.send(StreamEvent::ToolUseInputDelta(json.clone())).await;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }

                "content_block_stop" => {
                    if let Some(tool) = current_tool.take() {
                        result.tool_uses.push(tool);
                    }
                }

                "message_delta" => {
                    if let Some(usage) = &sse_data.usage {
                        result.output_tokens = usage.output_tokens;
                    }
                }

                "message_stop" => {
                    if let Some(tx) = tx {
                        let _ = tx.send(StreamEvent::MessageStop {
                            input_tokens: result.input_tokens,
                            output_tokens: result.output_tokens,
                        }).await;
                    }
                }

                "error" => {
                    let err_msg = data_str.clone();
                    if let Some(tx) = tx {
                        let _ = tx.send(StreamEvent::Error(err_msg.clone())).await;
                    }
                    return Err(anyhow::anyhow!("Claude streaming error: {}", err_msg));
                }

                _ => {} // ping, etc.
            }
        }
    }

    Ok(result)
}

/// Parse a tool use's accumulated JSON input into a serde_json::Value.
pub fn parse_tool_input(json_str: &str) -> Value {
    serde_json::from_str(json_str).unwrap_or(Value::Object(serde_json::Map::new()))
}

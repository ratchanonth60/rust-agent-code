use serde_json::Value;

/// Tool names whose results can be cleared during microcompact.
const CLEARABLE_TOOLS: &[&str] = &["Read", "Bash", "Grep", "Glob", "Edit", "Write"];

/// Microcompact: replace old tool result content with a short summary.
///
/// Keeps the most recent `keep_recent` turns intact. Older tool_result
/// blocks are replaced with `[Cleared: <tool_name> result]`.
///
/// Works on Claude-format messages (Vec of {role, content: [blocks]}).
pub fn microcompact(messages: &mut Vec<Value>, keep_recent: usize) {
    if messages.len() <= keep_recent {
        return;
    }

    let cutoff = messages.len().saturating_sub(keep_recent);

    for msg in messages[..cutoff].iter_mut() {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if role != "user" {
            continue;
        }

        if let Some(content) = msg.get_mut("content") {
            if let Some(blocks) = content.as_array_mut() {
                for block in blocks.iter_mut() {
                    let block_type = block
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("");

                    if block_type == "tool_result" {
                        // Check if the tool_use_id hints at a clearable tool
                        // or just clear all old tool results
                        let current_content = block
                            .get("content")
                            .and_then(|c| c.as_str())
                            .unwrap_or("");

                        // Only clear if content is large enough to be worth it
                        if current_content.len() > 500 {
                            block.as_object_mut().map(|obj| {
                                obj.insert(
                                    "content".to_string(),
                                    Value::String("[Cleared: tool result]".to_string()),
                                );
                            });
                        }
                    }
                }
            }
        }
    }
}

/// Build the summarization prompt for auto-compact.
pub fn build_compact_prompt(messages_json: &str) -> String {
    format!(
        "You are summarizing a conversation between a user and an AI coding assistant. \
        Preserve the following in your summary:\n\
        - Key decisions and conclusions reached\n\
        - Important file paths and code changes made\n\
        - Current task and what remains to be done\n\
        - Any errors encountered and their resolution\n\
        - User preferences expressed during the conversation\n\n\
        Be concise but thorough. Output only the summary, no preamble.\n\n\
        Conversation to summarize:\n{}\n",
        messages_json
    )
}

/// Compact OpenAI-format messages (Vec<ChatCompletionRequestMessage> serialized as JSON).
///
/// For OpenAI format, we clear old assistant tool_calls arguments and old
/// tool response content when they exceed a length threshold.
pub fn microcompact_openai(messages: &mut Vec<Value>, keep_recent: usize) {
    if messages.len() <= keep_recent {
        return;
    }

    let cutoff = messages.len().saturating_sub(keep_recent);

    for msg in messages[..cutoff].iter_mut() {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");

        match role {
            "tool" => {
                // Clear old tool response content if large
                if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                    if content.len() > 500 {
                        msg.as_object_mut().map(|obj| {
                            obj.insert(
                                "content".to_string(),
                                Value::String("[Cleared: tool result]".to_string()),
                            );
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_microcompact_clears_old_large_results() {
        let large_content = "x".repeat(1000);
        let mut messages = vec![
            json!({
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "abc",
                        "content": large_content
                    }
                ]
            }),
            json!({
                "role": "assistant",
                "content": [{"type": "text", "text": "ok"}]
            }),
            json!({
                "role": "user",
                "content": [{"type": "text", "text": "next question"}]
            }),
        ];

        microcompact(&mut messages, 2);

        let cleared = messages[0]["content"][0]["content"].as_str().unwrap();
        assert_eq!(cleared, "[Cleared: tool result]");
    }

    #[test]
    fn test_microcompact_keeps_small_results() {
        let mut messages = vec![
            json!({
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "abc",
                        "content": "small result"
                    }
                ]
            }),
            json!({
                "role": "assistant",
                "content": [{"type": "text", "text": "ok"}]
            }),
            json!({
                "role": "user",
                "content": [{"type": "text", "text": "next"}]
            }),
        ];

        microcompact(&mut messages, 2);

        let content = messages[0]["content"][0]["content"].as_str().unwrap();
        assert_eq!(content, "small result"); // unchanged
    }
}

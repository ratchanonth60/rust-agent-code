//! Context compaction for managing conversation length.
//!
//! When the conversation approaches the model's context window limit,
//! old tool results are replaced with short placeholders to reclaim
//! tokens without losing the conversation structure.

use serde_json::Value;

/// Byte-length threshold for clearing a tool result block.
///
/// Results shorter than this are kept intact — the compaction overhead
/// would not be worth the token savings.
const CLEAR_THRESHOLD: usize = 500;

/// Placeholder text inserted in place of cleared tool results.
const CLEARED_MARKER: &str = "[Cleared: tool result]";

/// Replaces large tool-result blocks in **Claude-format** messages.
///
/// Messages older than the most recent `keep_recent` turns have their
/// `tool_result` content replaced with [`CLEARED_MARKER`] when the
/// original content exceeds [`CLEAR_THRESHOLD`] bytes.
///
/// # Arguments
///
/// * `messages` — mutable slice of Claude-format JSON messages
/// * `keep_recent` — number of recent messages to leave untouched
pub fn microcompact(messages: &mut [Value], keep_recent: usize) {
    let len = messages.len();
    if len <= keep_recent {
        return;
    }
    let cutoff = len - keep_recent;

    for msg in &mut messages[..cutoff] {
        if msg.get("role").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let Some(blocks) = msg.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        for block in blocks.iter_mut() {
            let is_tool_result = block
                .get("type")
                .and_then(Value::as_str)
                == Some("tool_result");
            if !is_tool_result {
                continue;
            }
            let large = block
                .get("content")
                .and_then(Value::as_str)
                .map_or(false, |c| c.len() > CLEAR_THRESHOLD);
            if large {
                if let Some(obj) = block.as_object_mut() {
                    obj.insert("content".into(), Value::String(CLEARED_MARKER.into()));
                }
            }
        }
    }
}

/// Replaces large tool responses in **OpenAI-format** messages.
///
/// Works identically to [`microcompact`] but targets `role: "tool"`
/// messages used by the OpenAI chat-completion API.
pub fn microcompact_openai(messages: &mut [Value], keep_recent: usize) {
    let len = messages.len();
    if len <= keep_recent {
        return;
    }
    let cutoff = len - keep_recent;

    for msg in &mut messages[..cutoff] {
        if msg.get("role").and_then(Value::as_str) != Some("tool") {
            continue;
        }
        let large = msg
            .get("content")
            .and_then(Value::as_str)
            .map_or(false, |c| c.len() > CLEAR_THRESHOLD);
        if large {
            if let Some(obj) = msg.as_object_mut() {
                obj.insert("content".into(), Value::String(CLEARED_MARKER.into()));
            }
        }
    }
}

/// Builds a summarisation prompt for LLM-based auto-compaction.
///
/// The returned string can be sent to the LLM to produce a concise
/// summary of the conversation so far, preserving key decisions,
/// file paths, errors, and remaining tasks.
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
         Conversation to summarize:\n{messages_json}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn clears_old_large_tool_results() {
        let large = "x".repeat(1000);
        let mut msgs = vec![
            json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "a", "content": large}]}),
            json!({"role": "assistant", "content": [{"type": "text", "text": "ok"}]}),
            json!({"role": "user", "content": [{"type": "text", "text": "next"}]}),
        ];
        microcompact(&mut msgs, 2);
        assert_eq!(msgs[0]["content"][0]["content"].as_str().unwrap(), CLEARED_MARKER);
    }

    #[test]
    fn keeps_small_tool_results() {
        let mut msgs = vec![
            json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "a", "content": "small"}]}),
            json!({"role": "assistant", "content": [{"type": "text", "text": "ok"}]}),
            json!({"role": "user", "content": [{"type": "text", "text": "next"}]}),
        ];
        microcompact(&mut msgs, 2);
        assert_eq!(msgs[0]["content"][0]["content"].as_str().unwrap(), "small");
    }

    #[test]
    fn openai_clears_large_tool_messages() {
        let large = "y".repeat(1000);
        let mut msgs = vec![
            json!({"role": "tool", "content": large, "tool_call_id": "t1"}),
            json!({"role": "assistant", "content": "noted"}),
            json!({"role": "user", "content": "what next?"}),
        ];
        microcompact_openai(&mut msgs, 2);
        assert_eq!(msgs[0]["content"].as_str().unwrap(), CLEARED_MARKER);
    }
}

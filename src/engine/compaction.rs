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
                .is_some_and(|c| c.len() > CLEAR_THRESHOLD);
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
            .is_some_and(|c| c.len() > CLEAR_THRESHOLD);
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

// ── Auto-compact service ────────────────────────────────────────────────

/// Tracks auto-compaction state across the session.
///
/// Prevents runaway compaction attempts when the LLM summarizer
/// repeatedly fails, using a circuit breaker pattern (max 3 failures
/// before disabling).
#[derive(Debug, Clone)]
pub struct AutoCompactState {
    /// Number of consecutive compaction failures.
    failures: u32,
    /// Maximum consecutive failures before disabling auto-compact.
    max_failures: u32,
    /// `true` when micro-compaction was already applied this turn.
    pub micro_compacted: bool,
}

impl Default for AutoCompactState {
    fn default() -> Self {
        Self {
            failures: 0,
            max_failures: 3,
            micro_compacted: false,
        }
    }
}

impl AutoCompactState {
    /// Create a new auto-compact state with the default circuit breaker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if the circuit breaker has not tripped.
    pub fn is_available(&self) -> bool {
        self.failures < self.max_failures
    }

    /// Record a successful compaction, resetting the failure counter.
    pub fn record_success(&mut self) {
        self.failures = 0;
    }

    /// Record a failed compaction attempt.
    pub fn record_failure(&mut self) {
        self.failures += 1;
        if self.failures >= self.max_failures {
            tracing::warn!(
                "Auto-compact circuit breaker tripped after {} failures",
                self.failures
            );
        }
    }
}

/// Checks if auto-compaction should run based on estimated token usage.
///
/// Returns `true` when estimated tokens exceed `threshold` fraction of
/// the context window **and** the circuit breaker has not tripped.
pub fn should_auto_compact(
    estimated_tokens: u64,
    context_window: u64,
    threshold: f64,
    state: &AutoCompactState,
) -> bool {
    state.is_available()
        && super::tokens::should_compact(estimated_tokens, context_window, threshold)
}

/// Builds a compact system message containing the LLM-generated summary.
///
/// After the LLM produces a summary from [`build_compact_prompt`], this
/// wraps it in a standard role+content JSON value suitable for insertion
/// at the beginning of the conversation (after the system prompt).
pub fn build_summary_message(summary: &str) -> Value {
    serde_json::json!({
        "role": "system",
        "content": format!(
            "[Conversation Summary]\n{}\n[End Summary — the above is a compressed summary of earlier conversation]",
            summary
        )
    })
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

    #[test]
    fn auto_compact_state_circuit_breaker() {
        let mut state = AutoCompactState::new();
        assert!(state.is_available());

        state.record_failure();
        assert!(state.is_available());

        state.record_failure();
        assert!(state.is_available());

        state.record_failure();
        assert!(!state.is_available()); // tripped after 3

        state.record_success(); // resets
        assert!(state.is_available());
    }

    #[test]
    fn should_auto_compact_respects_circuit_breaker() {
        let available_state = AutoCompactState::new();
        // Over threshold — should compact
        assert!(should_auto_compact(170_000, 200_000, 0.8, &available_state));
        // Under threshold — should not
        assert!(!should_auto_compact(100_000, 200_000, 0.8, &available_state));

        let mut tripped_state = AutoCompactState::new();
        tripped_state.record_failure();
        tripped_state.record_failure();
        tripped_state.record_failure();
        // Over threshold but circuit breaker tripped
        assert!(!should_auto_compact(170_000, 200_000, 0.8, &tripped_state));
    }

    #[test]
    fn build_summary_message_format() {
        let msg = build_summary_message("Test summary");
        assert_eq!(msg["role"], "system");
        let content = msg["content"].as_str().unwrap();
        assert!(content.contains("Test summary"));
        assert!(content.contains("[Conversation Summary]"));
    }
}

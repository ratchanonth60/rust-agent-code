/// Approximate token estimator (~4 chars per token, matching tiktoken averages).
pub fn estimate_tokens(text: &str) -> u64 {
    // Simple heuristic: ~4 characters per token for English text.
    // More conservative (lower chars/token) for code.
    let chars = text.len() as u64;
    (chars + 3) / 4 // ceil division
}

/// Estimate total tokens for a conversation (list of serializable messages).
pub fn estimate_conversation_tokens(messages: &[serde_json::Value]) -> u64 {
    messages
        .iter()
        .map(|m| {
            let s = m.to_string();
            estimate_tokens(&s)
        })
        .sum()
}

/// Context window sizes keyed by model name patterns.
pub fn get_context_window(model: &str) -> u64 {
    let model_lower = model.to_lowercase();
    if model_lower.contains("claude") {
        200_000
    } else if model_lower.contains("gpt-4o") || model_lower.contains("gpt-4") {
        128_000
    } else if model_lower.contains("gemini") {
        1_000_000
    } else {
        128_000 // reasonable default
    }
}

/// Returns true if the conversation is using more than `threshold` fraction of the context window.
pub fn should_compact(estimated_tokens: u64, context_window: u64, threshold: f64) -> bool {
    estimated_tokens as f64 > context_window as f64 * threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("hi"), 1); // 2 chars → 1 token
        assert_eq!(estimate_tokens("hello world"), 3); // 11 chars → ~3 tokens
        assert!(estimate_tokens("a".repeat(1000).as_str()) >= 250);
    }

    #[test]
    fn test_context_windows() {
        assert_eq!(get_context_window("claude-sonnet-4-20250514"), 200_000);
        assert_eq!(get_context_window("gpt-4o"), 128_000);
        assert_eq!(get_context_window("gemini-2.5-pro"), 1_000_000);
    }

    #[test]
    fn test_should_compact() {
        assert!(!should_compact(100_000, 200_000, 0.8));
        assert!(should_compact(170_000, 200_000, 0.8));
    }
}

//! Token estimation and context window utilities.
//!
//! Provides lightweight heuristics for estimating token counts without
//! depending on a full tokenizer. Used by the compaction system to decide
//! when to clear old tool results.

/// Approximates the token count for a UTF-8 string.
///
/// Uses the common heuristic of ~4 characters per token, which aligns
/// with tiktoken averages for English text and code.
///
/// # Examples
///
/// ```
/// # use rust_agent::engine::tokens::estimate_tokens;
/// assert_eq!(estimate_tokens("hello world"), 3);
/// assert_eq!(estimate_tokens(""), 0);
/// ```
pub fn estimate_tokens(text: &str) -> u64 {
    let len = text.len() as u64;
    (len + 3) / 4 // ceiling division
}

/// Estimates total tokens for a serialised conversation.
///
/// Each message is converted to its JSON string representation before
/// counting, which includes structural overhead (keys, braces, etc.).
pub fn estimate_conversation_tokens(messages: &[serde_json::Value]) -> u64 {
    messages.iter().map(|m| estimate_tokens(&m.to_string())).sum()
}

/// Returns the context window size (in tokens) for a given model name.
///
/// Uses substring matching on the lowercased model identifier.
/// Falls back to 128 000 tokens for unrecognised models.
///
/// Sources:
/// - Anthropic docs (2026-04): Opus 4.6, Sonnet 4.6 = 1M; others = 200k
/// - Google AI pricing (2026-04): Gemini 2.5+ = 1M
/// - OpenAI: GPT-4o = 128k
pub fn get_context_window(model: &str) -> u64 {
    let m = model.to_lowercase();
    // Claude Opus 4.6 and Sonnet 4.6 have a 1M context window.
    if m.contains("opus-4-6") || m.contains("sonnet-4-6") {
        1_000_000
    } else if m.contains("claude") {
        200_000
    } else if m.contains("gpt-4o") || m.contains("gpt-4") {
        128_000
    } else if m.contains("gemini") {
        1_000_000
    } else {
        128_000
    }
}

/// Returns `true` when estimated usage exceeds `threshold` of the window.
///
/// A typical threshold is `0.8` (80 %), triggering compaction before the
/// context is completely full.
pub fn should_compact(estimated_tokens: u64, context_window: u64, threshold: f64) -> bool {
    (estimated_tokens as f64) > (context_window as f64) * threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_empty_string() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_short_string() {
        assert_eq!(estimate_tokens("hi"), 1);
        assert_eq!(estimate_tokens("hello world"), 3);
    }

    #[test]
    fn estimate_long_string() {
        assert!(estimate_tokens(&"a".repeat(1000)) >= 250);
    }

    #[test]
    fn context_windows() {
        assert_eq!(get_context_window("claude-opus-4-6"), 1_000_000);
        assert_eq!(get_context_window("claude-sonnet-4-6"), 1_000_000);
        assert_eq!(get_context_window("claude-sonnet-4-20250514"), 200_000);
        assert_eq!(get_context_window("gpt-4o"), 128_000);
        assert_eq!(get_context_window("gemini-2.5-pro"), 1_000_000);
        assert_eq!(get_context_window("unknown-model"), 128_000);
    }

    #[test]
    fn compact_threshold() {
        assert!(!should_compact(100_000, 200_000, 0.8));
        assert!(should_compact(170_000, 200_000, 0.8));
    }
}

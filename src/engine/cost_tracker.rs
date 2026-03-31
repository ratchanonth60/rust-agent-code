use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub web_search_requests: u64,
    pub cost_usd: f64,
    pub context_window: u64,
    pub max_output_tokens: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostTracker {
    pub total_cost_usd: f64,
    pub total_api_duration_ms: u64,
    pub total_tool_duration_ms: u64,
    pub total_lines_added: u64,
    pub total_lines_removed: u64,
    
    // Usage mapped by Model shortname
    pub model_usage: HashMap<String, ModelUsage>,
}

impl CostTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add exact usage from an API call
    pub fn add_usage(&mut self, model: &str, input_tokens: u64, output_tokens: u64, cost: f64) {
        let usage = self.model_usage.entry(model.to_string()).or_default();
        usage.input_tokens += input_tokens;
        usage.output_tokens += output_tokens;
        usage.cost_usd += cost;
        self.total_cost_usd += cost;
    }

    /// Renders a terminal-friendly summary of the cost exactly like the TS `formatTotalCost`
    pub fn format_total_cost(&self) -> String {
        let mut result = format!(
            "Total cost:            ${:.4}\n\
             Total duration (API):  {}ms\n\
             Total code changes:    {} lines added, {} lines removed\n\
             Usage by model:\n",
            self.total_cost_usd,
            self.total_api_duration_ms,
            self.total_lines_added,
            self.total_lines_removed,
        );

        if self.model_usage.is_empty() {
            result.push_str("  Usage: 0 input, 0 output, 0 cache read, 0 cache write");
            return result;
        }

        for (model, usage) in &self.model_usage {
            let usage_str = format!(
                "{:>21}:  {} input, {} output, {} cache read, {} cache write (${:.4})\n",
                model,
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_input_tokens,
                usage.cache_creation_input_tokens,
                usage.cost_usd
            );
            result.push_str(&usage_str);
        }

        result
    }
}

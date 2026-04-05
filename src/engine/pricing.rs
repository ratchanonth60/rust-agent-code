//! Per-model pricing and cost calculation.
//!
//! Provides a static lookup table mapping known model names to their
//! per-million-token pricing, plus a [`calculate_cost`] helper used by
//! every provider loop to record USD spend in the cost tracker.
//!
//! # Adding a new model
//!
//! Insert a row in [`PRICING_TABLE`] *before* the catch-all for its provider
//! family so that the more-specific pattern matches first.
//!
//! # Fallback
//!
//! Unrecognised model names fall back to Sonnet-tier pricing ($3 / $15).

/// Per-model pricing in USD per million tokens.
#[derive(Clone, Copy)]
pub(crate) struct ModelPricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
}

/// A row in the static pricing lookup table.
struct PricingEntry {
    /// Required substring (lowercased) in the model name.
    primary: &'static str,
    /// Optional second substring; both must match when set.
    secondary: Option<&'static str>,
    price: ModelPricing,
}

/// Static pricing table (USD / million tokens).
///
/// Scanned top-to-bottom; the first row whose pattern(s) all appear
/// in the lowercased model name wins.  Keep **more-specific** entries
/// above broader family catch-alls for the same provider.
///
/// Sources: Anthropic docs · Google AI pricing · OpenAI pricing (2026-04).
static PRICING_TABLE: &[PricingEntry] = &[
    // ── Claude ─────────────────────────────────────────────────────
    PricingEntry { primary: "opus-4-6",    secondary: None,               price: ModelPricing { input_per_mtok:  5.00, output_per_mtok: 25.00 } },
    PricingEntry { primary: "opus-4-5",    secondary: None,               price: ModelPricing { input_per_mtok:  5.00, output_per_mtok: 25.00 } },
    PricingEntry { primary: "opus",        secondary: None,               price: ModelPricing { input_per_mtok: 15.00, output_per_mtok: 75.00 } },
    PricingEntry { primary: "haiku-4",     secondary: None,               price: ModelPricing { input_per_mtok:  1.00, output_per_mtok:  5.00 } },
    PricingEntry { primary: "haiku",       secondary: None,               price: ModelPricing { input_per_mtok:  0.25, output_per_mtok:  1.25 } },
    PricingEntry { primary: "sonnet",      secondary: None,               price: ModelPricing { input_per_mtok:  3.00, output_per_mtok: 15.00 } },
    PricingEntry { primary: "claude",      secondary: None,               price: ModelPricing { input_per_mtok:  3.00, output_per_mtok: 15.00 } },
    // ── Gemini ─────────────────────────────────────────────────────
    PricingEntry { primary: "gemini-3",    secondary: Some("pro"),        price: ModelPricing { input_per_mtok:  2.00, output_per_mtok: 12.00 } },
    PricingEntry { primary: "gemini-3",    secondary: Some("flash-lite"), price: ModelPricing { input_per_mtok:  0.25, output_per_mtok:  1.50 } },
    PricingEntry { primary: "gemini-3",    secondary: Some("flash"),      price: ModelPricing { input_per_mtok:  0.50, output_per_mtok:  3.00 } },
    PricingEntry { primary: "gemini-2.5",  secondary: Some("flash-lite"), price: ModelPricing { input_per_mtok:  0.10, output_per_mtok:  0.40 } },
    PricingEntry { primary: "gemini-2.5",  secondary: Some("flash"),      price: ModelPricing { input_per_mtok:  0.30, output_per_mtok:  2.50 } },
    PricingEntry { primary: "gemini-2.5",  secondary: None,               price: ModelPricing { input_per_mtok:  1.25, output_per_mtok: 10.00 } },
    PricingEntry { primary: "gemini",      secondary: Some("flash"),      price: ModelPricing { input_per_mtok:  0.10, output_per_mtok:  0.40 } },
    PricingEntry { primary: "gemini",      secondary: None,               price: ModelPricing { input_per_mtok:  1.25, output_per_mtok: 10.00 } },
    // ── OpenAI ─────────────────────────────────────────────────────
    PricingEntry { primary: "gpt-4.1-mini",  secondary: None, price: ModelPricing { input_per_mtok:  0.40, output_per_mtok:  1.60 } },
    PricingEntry { primary: "gpt-4.1-nano",  secondary: None, price: ModelPricing { input_per_mtok:  0.10, output_per_mtok:  0.40 } },
    PricingEntry { primary: "gpt-4.1",       secondary: None, price: ModelPricing { input_per_mtok:  2.00, output_per_mtok:  8.00 } },
    PricingEntry { primary: "gpt-4o-mini",   secondary: None, price: ModelPricing { input_per_mtok:  0.15, output_per_mtok:  0.60 } },
    PricingEntry { primary: "gpt-4o",        secondary: None, price: ModelPricing { input_per_mtok:  2.50, output_per_mtok: 10.00 } },
    PricingEntry { primary: "gpt-4",         secondary: None, price: ModelPricing { input_per_mtok:  2.50, output_per_mtok: 10.00 } },
    PricingEntry { primary: "gpt-3.5",       secondary: None, price: ModelPricing { input_per_mtok:  0.50, output_per_mtok:  1.50 } },
    PricingEntry { primary: "o4-mini",       secondary: None, price: ModelPricing { input_per_mtok:  1.10, output_per_mtok:  4.40 } },
    PricingEntry { primary: "o3-mini",       secondary: None, price: ModelPricing { input_per_mtok:  1.10, output_per_mtok:  4.40 } },
    PricingEntry { primary: "o3",            secondary: None, price: ModelPricing { input_per_mtok: 10.00, output_per_mtok: 40.00 } },
    PricingEntry { primary: "o1-mini",       secondary: None, price: ModelPricing { input_per_mtok:  1.10, output_per_mtok:  4.40 } },
    PricingEntry { primary: "o1",            secondary: None, price: ModelPricing { input_per_mtok: 15.00, output_per_mtok: 60.00 } },
];

/// Looks up the price for `model` by scanning [`PRICING_TABLE`].
///
/// Falls back to Sonnet-tier ($3 / $15) for unrecognised models.
pub(crate) fn get_model_pricing(model: &str) -> ModelPricing {
    let m = model.to_lowercase();
    PRICING_TABLE
        .iter()
        .find(|e| m.contains(e.primary) && e.secondary.is_none_or(|s| m.contains(s)))
        .map_or(ModelPricing { input_per_mtok: 3.0, output_per_mtok: 15.0 }, |e| e.price)
}

/// Calculates the USD cost for a single API call.
pub(crate) fn calculate_cost(model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
    let p = get_model_pricing(model);
    (input_tokens as f64 / 1_000_000.0) * p.input_per_mtok
        + (output_tokens as f64 / 1_000_000.0) * p.output_per_mtok
}

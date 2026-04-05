//! Engine module re-exports.
//!
//! Aggregates the query engine, configuration, cost tracking, streaming,
//! token estimation, context compaction, shared state, session, pricing,
//! and provider-specific agentic loops.
//!
//! # Module layout
//!
//! | Module       | Responsibility                                      |
//! |--------------|-----------------------------------------------------|
//! | `query`      | `QueryEngine` struct, constructor, shared helpers    |
//! | `providers/` | Per-provider agentic loops (Claude, OpenAI, Gemini) |
//! | `pricing`    | Per-model pricing table + cost calculation           |
//! | `streaming`  | Claude SSE parser                                   |
//! | `tokens`     | Context window sizes + token estimation              |
//! | `compaction` | Microcompact logic for long conversations            |
//! | `config`     | `EngineConfig` with CLI-driven settings              |
//! | `cost_tracker`| Per-session token + USD tracking                   |
//! | `session`    | JSONL session persistence                            |
//! | `state`      | Shared mutable state (engine-level)                  |
//! | `agent_tool` | Sub-agent spawning tool                              |

pub mod agent_tool;
pub mod compaction;
pub mod config;
pub mod cost_tracker;
pub mod pricing;
pub mod providers;
pub mod query;
pub mod session;
pub mod state;
pub mod streaming;
pub mod tokens;

pub use config::*;
pub use query::*;

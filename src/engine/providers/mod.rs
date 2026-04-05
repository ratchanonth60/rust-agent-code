//! Provider-specific agentic loop implementations.
//!
//! Each sub-module contains the full agentic loop for one LLM provider,
//! implemented as `impl QueryEngine` methods in separate files.
//!
//! | Module   | Provider                     | Key method                |
//! |----------|------------------------------|---------------------------|
//! | claude   | Anthropic Messages API (SSE) | `query_claude()`          |
//! | openai   | OpenAI / compatible          | `query_openai_compatible()`|
//! | gemini   | Google Gemini (raw HTTP SSE) | `query_gemini_compat()`   |

pub mod claude;
pub mod gemini;
pub mod openai;

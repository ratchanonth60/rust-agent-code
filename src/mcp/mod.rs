//! MCP (Model Context Protocol) integration.
//!
//! Implements a JSON-RPC 2.0 client that connects to external MCP servers
//! over stdio, discovers their tools and resources, and exposes them as
//! dynamically-registered [`Tool`](crate::tools::Tool) instances.
//!
//! # Architecture
//!
//! ```text
//!  ┌──────────┐   stdio    ┌────────────┐
//!  │ MCP      │ ◄────────► │ External   │
//!  │ Client   │  JSON-RPC  │ MCP Server │
//!  └────┬─────┘            └────────────┘
//!       │
//!  ┌────┴─────┐
//!  │ McpProxy │  ← one per server tool
//!  │ Tool     │
//!  └──────────┘
//! ```

pub mod types;
pub mod transport;
pub mod client;
pub mod manager;
pub mod tools;

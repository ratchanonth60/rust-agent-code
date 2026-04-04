//! Plugin system — discovery, loading, and lifecycle hooks.
//!
//! Plugins live at `~/.rust-agent/plugins/{name}/` and contain a
//! `plugin.json` manifest describing hooks and optional tool definitions.
//!
//! # Plugin structure
//!
//! ```text
//! ~/.rust-agent/plugins/my-plugin/
//!   ├── plugin.json   ← manifest (required)
//!   ├── on_start.sh   ← optional hook script
//!   └── tools/
//!       └── my_tool.sh
//! ```
//!
//! # Usage
//!
//! ```ignore
//! let plugins = crate::plugins::load_plugins();
//! for plugin in &plugins {
//!     println!("{}: {}", plugin.manifest.name, plugin.manifest.description);
//! }
//! ```

pub mod loader;
pub mod types;

pub use loader::{load_plugins, load_plugins_from, run_hook};
pub use types::{LoadedPlugin, PluginHooks, PluginManifest, PluginToolDef};

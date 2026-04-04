//! Plugin type definitions — the interface contracts for runtime plugins.
//!
//! A plugin is a directory at `~/.rust-agent/plugins/{name}/` containing a
//! `plugin.json` manifest and optional hook scripts.

use serde::{Deserialize, Serialize};

/// Plugin manifest loaded from `plugin.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Unique plugin name.
    pub name: String,
    /// Semver version string.
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// Author name or handle.
    pub author: Option<String>,
    /// Hook definitions (lifecycle entry points).
    #[serde(default)]
    pub hooks: PluginHooks,
    /// Additional tool definitions provided by this plugin.
    #[serde(default)]
    pub tools: Vec<PluginToolDef>,
    /// Whether this plugin is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Lifecycle hooks that a plugin can define.
///
/// Each hook is an optional shell command executed at the corresponding
/// event in the engine lifecycle.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginHooks {
    /// Runs once when the engine starts.
    pub on_start: Option<String>,
    /// Runs once when the engine shuts down.
    pub on_exit: Option<String>,
    /// Runs before each LLM query.
    pub pre_query: Option<String>,
    /// Runs after each LLM query completes.
    pub post_query: Option<String>,
    /// Runs before a tool is executed.
    pub pre_tool: Option<String>,
    /// Runs after a tool finishes executing.
    pub post_tool: Option<String>,
}

/// A tool definition contributed by a plugin.
///
/// Plugin tools are shell-script based: the engine calls the `command`
/// with JSON input on stdin and reads JSON output on stdout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginToolDef {
    /// Tool name as seen by the LLM.
    pub name: String,
    /// Tool description.
    pub description: String,
    /// Shell command to invoke (receives JSON on stdin).
    pub command: String,
    /// JSON Schema for the tool's input.
    #[serde(default)]
    pub input_schema: serde_json::Value,
}

/// A loaded plugin with its manifest and resolved directory path.
#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    /// Resolved path to the plugin directory.
    pub path: std::path::PathBuf,
    /// The parsed manifest.
    pub manifest: PluginManifest,
}

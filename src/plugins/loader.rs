//! Plugin loader — discovers and loads plugins from the filesystem.
//!
//! Scans `~/.rust-agent/plugins/` for directories containing a
//! `plugin.json` manifest.  Each valid manifest is parsed into a
//! [`LoadedPlugin`] and returned for registration.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

use super::types::{LoadedPlugin, PluginManifest};

// ── Plugin discovery ────────────────────────────────────────────────

/// Returns the plugins directory.
fn plugins_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rust-agent")
        .join("plugins")
}

/// Discover and load all plugins from the default plugins directory.
///
/// Skips plugins whose manifest cannot be parsed and logs a warning
/// via `tracing`.
pub fn load_plugins() -> Vec<LoadedPlugin> {
    let dir = plugins_dir();
    if !dir.exists() {
        return Vec::new();
    }

    load_plugins_from(&dir)
}

/// Load plugins from a specific directory (useful for testing).
pub fn load_plugins_from(dir: &std::path::Path) -> Vec<LoadedPlugin> {
    let mut plugins = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return plugins,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let manifest_path = path.join("plugin.json");
        if !manifest_path.exists() {
            continue;
        }

        match load_manifest(&manifest_path) {
            Ok(manifest) => {
                if manifest.enabled {
                    plugins.push(LoadedPlugin {
                        path: path.clone(),
                        manifest,
                    });
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to load plugin from {}: {}",
                    path.display(),
                    e
                );
            }
        }
    }

    // Sort by name for deterministic ordering
    plugins.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
    plugins
}

/// Parse a single plugin manifest file.
fn load_manifest(path: &std::path::Path) -> Result<PluginManifest> {
    let json = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let manifest: PluginManifest = serde_json::from_str(&json)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(manifest)
}

/// Execute a hook command (fire-and-forget, logs errors).
///
/// Runs the command in the plugin's directory via `sh -c`.
pub fn run_hook(plugin: &LoadedPlugin, hook_cmd: &str) -> Result<String> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(hook_cmd)
        .current_dir(&plugin.path)
        .output()
        .with_context(|| format!(
            "Failed to run hook for plugin '{}'",
            plugin.manifest.name
        ))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        anyhow::bail!(
            "Hook failed for plugin '{}': {}",
            plugin.manifest.name,
            stderr
        );
    }

    Ok(if stdout.is_empty() { stderr } else { stdout })
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_plugin(dir: &std::path::Path, name: &str, enabled: bool) {
        let plugin_dir = dir.join(name);
        fs::create_dir_all(&plugin_dir).unwrap();
        let manifest = serde_json::json!({
            "name": name,
            "version": "1.0.0",
            "description": format!("Test plugin {}", name),
            "enabled": enabled,
            "hooks": {},
            "tools": []
        });
        fs::write(
            plugin_dir.join("plugin.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        ).unwrap();
    }

    #[test]
    fn load_plugins_from_empty_dir() {
        let dir = std::env::temp_dir().join(format!("test-plugins-empty-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let plugins = load_plugins_from(&dir);
        assert!(plugins.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_enabled_plugin() {
        let dir = std::env::temp_dir().join(format!("test-plugins-enabled-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        setup_test_plugin(&dir, "my-plugin", true);

        let plugins = load_plugins_from(&dir);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].manifest.name, "my-plugin");
        assert_eq!(plugins[0].manifest.version, "1.0.0");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn skip_disabled_plugin() {
        let dir = std::env::temp_dir().join(format!("test-plugins-disabled-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        setup_test_plugin(&dir, "disabled-plugin", false);

        let plugins = load_plugins_from(&dir);
        assert!(plugins.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_plugin_with_tools() {
        let dir = std::env::temp_dir().join(format!("test-plugins-tools-{}", std::process::id()));
        let plugin_dir = dir.join("tool-plugin");
        fs::create_dir_all(&plugin_dir).unwrap();

        let manifest = serde_json::json!({
            "name": "tool-plugin",
            "version": "0.1.0",
            "description": "Plugin with tools",
            "tools": [{
                "name": "MyCustomTool",
                "description": "A custom tool from a plugin",
                "command": "echo '{\"status\": \"ok\"}'",
                "input_schema": {"type": "object", "properties": {}}
            }]
        });
        fs::write(
            plugin_dir.join("plugin.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        ).unwrap();

        let plugins = load_plugins_from(&dir);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].manifest.tools.len(), 1);
        assert_eq!(plugins[0].manifest.tools[0].name, "MyCustomTool");

        let _ = fs::remove_dir_all(&dir);
    }
}

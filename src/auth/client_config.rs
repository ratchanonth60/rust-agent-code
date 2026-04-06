//! OAuth2 client configuration for Google / Gemini.
//!
//! Ships with bundled defaults (standard for desktop CLI apps).
//! Users can override by placing their own `gemini.json` at
//! `~/.rust-agent/oauth-clients/gemini.json`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ── Types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClientConfig {
    pub client_id: String,
    pub client_secret: String,
    pub auth_uri: String,
    pub token_uri: String,
    pub revoke_uri: String,
    pub scopes: Vec<String>,
}

// ── Bundled defaults ─────────────────────────────────────────────────

/// Returns the bundled Google OAuth client config for Gemini.
///
/// These are "desktop application" credentials which, per Google's
/// documentation, are not considered confidential.  The same pattern
/// is used by `gcloud`, `firebase-tools`, and similar CLI tools.
pub fn default_gemini_config() -> OAuthClientConfig {
    OAuthClientConfig {
        // Default credentials — users should create their own via
        // Google Cloud Console and place them at the override path.
        client_id: "YOUR_CLIENT_ID.apps.googleusercontent.com".into(),
        client_secret: "YOUR_CLIENT_SECRET".into(),
        auth_uri: "https://accounts.google.com/o/oauth2/v2/auth".into(),
        token_uri: "https://oauth2.googleapis.com/token".into(),
        revoke_uri: "https://oauth2.googleapis.com/revoke".into(),
        scopes: vec![
            "https://www.googleapis.com/auth/generative-language".into(),
            "https://www.googleapis.com/auth/cloud-platform".into(),
        ],
    }
}

// ── Override loading ─────────────────────────────────────────────────

/// Returns `~/.rust-agent/oauth-clients/gemini.json`.
fn override_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rust-agent")
        .join("oauth-clients")
        .join("gemini.json")
}

/// Load the Gemini OAuth config.
///
/// Checks `~/.rust-agent/oauth-clients/gemini.json` first;
/// falls back to bundled defaults if the file doesn't exist or fails to parse.
pub fn load_gemini_config() -> OAuthClientConfig {
    let path = override_path();
    if let Ok(json) = fs::read_to_string(&path) {
        if let Ok(cfg) = serde_json::from_str::<OAuthClientConfig>(&json) {
            return cfg;
        }
    }
    default_gemini_config()
}

/// Returns `true` if the config still has placeholder credentials.
pub fn is_placeholder_config(config: &OAuthClientConfig) -> bool {
    config.client_id.starts_with("YOUR_CLIENT_ID")
        || config.client_secret.starts_with("YOUR_CLIENT_SECRET")
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_correct_endpoints() {
        let cfg = default_gemini_config();
        assert!(cfg.auth_uri.contains("accounts.google.com"));
        assert!(cfg.token_uri.contains("oauth2.googleapis.com"));
        assert!(!cfg.scopes.is_empty());
    }

    #[test]
    fn placeholder_detection() {
        let cfg = default_gemini_config();
        assert!(is_placeholder_config(&cfg));

        let real = OAuthClientConfig {
            client_id: "123.apps.googleusercontent.com".into(),
            client_secret: "GOCSPX-abc".into(),
            ..cfg
        };
        assert!(!is_placeholder_config(&real));
    }

    #[test]
    fn serialize_roundtrip() {
        let cfg = default_gemini_config();
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let loaded: OAuthClientConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.client_id, cfg.client_id);
    }
}

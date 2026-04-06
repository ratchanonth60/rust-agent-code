//! OAuth2 credential storage — load, save, and manage tokens.
//!
//! Tokens are persisted at `~/.rust-agent/credentials.json` with
//! `0o600` permissions on Unix.  The [`CredentialStore`] supports
//! multiple providers via a `HashMap<String, TokenCredential>`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

// ── Constants ────────────────────────────────────────────────────────

/// Seconds before actual expiry at which we consider the token "needs refresh".
const REFRESH_BUFFER_SECS: u64 = 300; // 5 minutes

// ── Types ────────────────────────────────────────────────────────────

/// Stored OAuth2 token set for a single provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenCredential {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix timestamp (seconds) when the access token expires.
    pub expires_at: u64,
    pub provider: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl TokenCredential {
    /// Returns `true` if the access token has already expired.
    pub fn is_expired(&self) -> bool {
        now_secs() >= self.expires_at
    }

    /// Returns `true` if the access token will expire within the refresh buffer.
    pub fn needs_refresh(&self) -> bool {
        now_secs() + REFRESH_BUFFER_SECS >= self.expires_at
    }
}

/// Multi-provider credential store, persisted to disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CredentialStore {
    #[serde(default)]
    pub tokens: HashMap<String, TokenCredential>,
}

// ── Paths ────────────────────────────────────────────────────────────

/// Returns `~/.rust-agent/credentials.json`.
pub fn credentials_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rust-agent")
        .join("credentials.json")
}

// ── CredentialStore implementation ───────────────────────────────────

impl CredentialStore {
    /// Load from disk. Returns an empty store if the file doesn't exist.
    pub fn load() -> Result<Self> {
        let path = credentials_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let json = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read credentials: {}", path.display()))?;
        let store: Self = serde_json::from_str(&json)
            .with_context(|| "Failed to parse credentials.json")?;
        Ok(store)
    }

    /// Persist to disk with restrictive permissions.
    pub fn save(&self) -> Result<()> {
        let path = credentials_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create dir: {}", parent.display()))?;
        }

        let json = serde_json::to_string_pretty(self)
            .with_context(|| "Failed to serialize credentials")?;
        fs::write(&path, &json)
            .with_context(|| format!("Failed to write credentials: {}", path.display()))?;

        // Restrict to owner-only on Unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o600);
            fs::set_permissions(&path, perms)?;
        }

        Ok(())
    }

    pub fn get_token(&self, provider: &str) -> Option<&TokenCredential> {
        self.tokens.get(provider)
    }

    pub fn set_token(&mut self, provider: &str, cred: TokenCredential) {
        self.tokens.insert(provider.to_string(), cred);
    }

    pub fn remove_token(&mut self, provider: &str) {
        self.tokens.remove(provider);
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_expiry_logic() {
        let cred = TokenCredential {
            access_token: "test".into(),
            refresh_token: "ref".into(),
            expires_at: now_secs() + 10, // expires in 10s
            provider: "gemini".into(),
            scopes: vec![],
        };
        assert!(!cred.is_expired());
        assert!(cred.needs_refresh()); // within 5-min buffer

        let fresh = TokenCredential {
            expires_at: now_secs() + 3600, // 1 hour
            ..cred.clone()
        };
        assert!(!fresh.is_expired());
        assert!(!fresh.needs_refresh());

        let expired = TokenCredential {
            expires_at: now_secs().saturating_sub(10),
            ..cred
        };
        assert!(expired.is_expired());
        assert!(expired.needs_refresh());
    }

    #[test]
    fn store_roundtrip() {
        let mut store = CredentialStore::default();
        assert!(store.get_token("gemini").is_none());

        store.set_token(
            "gemini",
            TokenCredential {
                access_token: "ya29.abc".into(),
                refresh_token: "1//xyz".into(),
                expires_at: 1_700_000_000,
                provider: "gemini".into(),
                scopes: vec!["scope1".into()],
            },
        );
        assert!(store.get_token("gemini").is_some());

        store.remove_token("gemini");
        assert!(store.get_token("gemini").is_none());
    }

    #[test]
    fn serialize_deserialize() {
        let mut store = CredentialStore::default();
        store.set_token(
            "gemini",
            TokenCredential {
                access_token: "tok".into(),
                refresh_token: "ref".into(),
                expires_at: 123,
                provider: "gemini".into(),
                scopes: vec![],
            },
        );
        let json = serde_json::to_string(&store).unwrap();
        let loaded: CredentialStore = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.get_token("gemini").unwrap().access_token, "tok");
    }
}

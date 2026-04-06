//! Authentication module — OAuth2 login, token storage, and resolution.
//!
//! Provides a fallback chain for API key resolution:
//! 1. OAuth token from `~/.rust-agent/credentials.json` (auto-refresh)
//! 2. Environment variable (`GEMINI_API_KEY`, `LLM_API_KEY`)
//!
//! # Usage
//!
//! ```ignore
//! // In provider code:
//! if let Ok(Some(token)) = auth::resolve_gemini_token() {
//!     // use token
//! }
//! ```

pub mod client_config;
pub mod credentials;
pub mod oauth;

use anyhow::Result;
use tracing::info;

use client_config::{load_claude_config, load_gemini_config};
use credentials::CredentialStore;

/// The `anthropic-beta` header value required when using Claude OAuth tokens.
pub fn claude_oauth_beta_header() -> &'static str {
    "oauth-2025-04-20"
}

/// Resolve a valid Gemini OAuth token, refreshing if needed.
///
/// Returns `Ok(Some(access_token))` if a valid/refreshed token is available,
/// `Ok(None)` if no OAuth credentials exist or refresh failed (caller should
/// fall back to env vars).
pub fn resolve_gemini_token() -> Result<Option<String>> {
    let store = CredentialStore::load()?;
    let cred = match store.get_token("gemini") {
        Some(c) => c,
        None => return Ok(None),
    };

    // Token is still fresh — use it directly.
    if !cred.needs_refresh() {
        return Ok(Some(cred.access_token.clone()));
    }

    // Token needs refresh.
    if cred.refresh_token.is_empty() {
        info!("Gemini OAuth token expired and no refresh token available");
        return Ok(None);
    }

    // Try async refresh via block_in_place (safe under tokio multi-thread).
    let config = load_gemini_config();
    let refresh_result = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(oauth::refresh_access_token(
            &config,
            &cred.refresh_token,
        ))
    });

    match refresh_result {
        Ok(new_cred) => {
            let token = new_cred.access_token.clone();
            // Persist the refreshed token.
            let mut store = CredentialStore::load().unwrap_or_default();
            store.set_token("gemini", new_cred);
            if let Err(e) = store.save() {
                info!("Failed to save refreshed token: {e}");
            }
            Ok(Some(token))
        }
        Err(e) => {
            info!("Gemini OAuth token refresh failed: {e}");
            Ok(None) // Graceful fallback to env var.
        }
    }
}

/// Resolve a valid Claude (Anthropic) OAuth token, refreshing if needed.
///
/// Returns `Ok(Some(access_token))` if a valid/refreshed token is available,
/// `Ok(None)` if no OAuth credentials exist or refresh failed.
pub fn resolve_claude_token() -> Result<Option<String>> {
    let store = CredentialStore::load()?;
    let cred = match store.get_token("claude") {
        Some(c) => c,
        None => return Ok(None),
    };

    if !cred.needs_refresh() {
        return Ok(Some(cred.access_token.clone()));
    }

    if cred.refresh_token.is_empty() {
        info!("Claude OAuth token expired and no refresh token available");
        return Ok(None);
    }

    let config = load_claude_config();
    let refresh_result = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(oauth::refresh_claude_token(&config, &cred.refresh_token))
    });

    match refresh_result {
        Ok(new_cred) => {
            let token = new_cred.access_token.clone();
            let mut store = CredentialStore::load().unwrap_or_default();
            store.set_token("claude", new_cred);
            if let Err(e) = store.save() {
                info!("Failed to save refreshed Claude token: {e}");
            }
            Ok(Some(token))
        }
        Err(e) => {
            info!("Claude OAuth token refresh failed: {e}");
            Ok(None)
        }
    }
}

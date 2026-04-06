//! OAuth2 Authorization Code + PKCE flow for Google / Gemini and Anthropic / Claude.
//!
//! Implements the full browser-based login flow:
//! 1. Generate PKCE challenge
//! 2. Open browser to consent screen
//! 3. Listen on localhost for the redirect callback
//! 4. Exchange authorization code for tokens
//! 5. Save tokens to credential store
//!
//! Also provides token refresh and revocation.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::info;

use crate::auth::client_config::{self, AnthropicOAuthConfig, OAuthClientConfig};
use crate::auth::credentials::{CredentialStore, TokenCredential};

// ── PKCE ─────────────────────────────────────────────────────────────

pub struct PkceChallenge {
    pub code_verifier: String,
    pub code_challenge: String,
}

/// Generate a PKCE code verifier (43-128 chars) and S256 challenge.
pub fn generate_pkce() -> PkceChallenge {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 64];
    rng.fill(&mut bytes);

    let code_verifier = base64_url_encode(&bytes);

    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let digest = hasher.finalize();
    let code_challenge = base64_url_encode(&digest);

    PkceChallenge {
        code_verifier,
        code_challenge,
    }
}

// ── Authorization URL ────────────────────────────────────────────────

/// Build the Google OAuth2 authorization URL.
pub fn build_authorization_url(
    config: &OAuthClientConfig,
    redirect_port: u16,
    pkce: &PkceChallenge,
    state: &str,
) -> String {
    let redirect_uri = format!("http://127.0.0.1:{redirect_port}");
    let scopes = config.scopes.join(" ");

    format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&\
         code_challenge={}&code_challenge_method=S256&\
         access_type=offline&prompt=consent&state={}",
        config.auth_uri,
        percent_encode(&config.client_id),
        percent_encode(&redirect_uri),
        percent_encode(&scopes),
        percent_encode(&pkce.code_challenge),
        percent_encode(state),
    )
}

// ── Browser ──────────────────────────────────────────────────────────

/// Open a URL in the system's default browser.
pub fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();

    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();

    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd")
        .args(["/c", "start", url])
        .spawn();

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let result: Result<std::process::Child, std::io::Error> =
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "unsupported OS"));

    result.map(|_| ()).map_err(|e| anyhow!("Failed to open browser: {e}"))
}

// ── Callback server ──────────────────────────────────────────────────

/// Bind a localhost TCP listener on an OS-assigned port.
pub async fn start_callback_server() -> Result<(TcpListener, u16)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("Failed to bind callback server")?;
    let port = listener
        .local_addr()
        .context("Failed to get local addr")?
        .port();
    Ok((listener, port))
}

/// Wait for the OAuth2 callback, validate state, return the authorization code.
pub async fn wait_for_callback(
    listener: TcpListener,
    expected_state: &str,
    timeout_secs: u64,
) -> Result<String> {
    let accept_fut = async {
        let (mut stream, _addr) = listener.accept().await?;

        // Read the HTTP request (we only need the first line).
        let mut buf = vec![0u8; 4096];
        let n = stream.read(&mut buf).await?;
        let request = String::from_utf8_lossy(&buf[..n]);

        // Extract the request path (GET /path?query HTTP/1.1).
        let first_line = request.lines().next().unwrap_or("");
        let path = first_line.split_whitespace().nth(1).unwrap_or("");

        // Parse query parameters.
        let query_str = path.split('?').nth(1).unwrap_or("");
        let params = parse_query_string(query_str);

        // Check for error from Google.
        if let Some(err) = params.get("error") {
            let desc = params.get("error_description").map(|s| s.as_str()).unwrap_or("");
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
                 <html><body><h2>Login failed</h2><p>{err}: {desc}</p>\
                 <p>You can close this tab.</p></body></html>"
            );
            stream.write_all(response.as_bytes()).await?;
            return Err(anyhow!("OAuth error: {err} — {desc}"));
        }

        // Validate state (CSRF protection).
        let state = params.get("state").map(|s| s.as_str()).unwrap_or("");
        if state != expected_state {
            let response = "HTTP/1.1 400 Bad Request\r\n\r\nState mismatch";
            stream.write_all(response.as_bytes()).await?;
            return Err(anyhow!("OAuth state mismatch (possible CSRF attack)"));
        }

        // Extract authorization code.
        let code = params
            .get("code")
            .ok_or_else(|| anyhow!("No authorization code in callback"))?
            .clone();

        // Send success response.
        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
            <html><body><h2>Login successful!</h2>\
            <p>You can close this tab and return to the terminal.</p></body></html>";
        stream.write_all(response.as_bytes()).await?;

        Ok::<String, anyhow::Error>(code)
    };

    tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), accept_fut)
        .await
        .map_err(|_| anyhow!("Login timed out after {timeout_secs}s. Please try again."))?
}

// ── Token exchange ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
    scope: Option<String>,
    #[allow(dead_code)]
    token_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenErrorResponse {
    error: String,
    error_description: Option<String>,
}

/// Exchange an authorization code for tokens.
pub async fn exchange_code(
    config: &OAuthClientConfig,
    code: &str,
    code_verifier: &str,
    redirect_port: u16,
) -> Result<TokenCredential> {
    let client = reqwest::Client::new();
    let redirect_uri = format!("http://127.0.0.1:{redirect_port}");

    let resp = client
        .post(&config.token_uri)
        .form(&[
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("code", code),
            ("code_verifier", code_verifier),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri.as_str()),
        ])
        .send()
        .await
        .context("Token exchange request failed")?;

    let status = resp.status();
    let body = resp.text().await.context("Failed to read token response")?;

    if !status.is_success() {
        if let Ok(err) = serde_json::from_str::<TokenErrorResponse>(&body) {
            return Err(anyhow!(
                "Token exchange failed: {} — {}",
                err.error,
                err.error_description.unwrap_or_default()
            ));
        }
        return Err(anyhow!("Token exchange failed (HTTP {status}): {body}"));
    }

    let token: TokenResponse = serde_json::from_str(&body)
        .context("Failed to parse token response")?;

    let now = now_secs();
    let scopes = token
        .scope
        .map(|s| s.split(' ').map(String::from).collect())
        .unwrap_or_default();

    Ok(TokenCredential {
        access_token: token.access_token,
        refresh_token: token.refresh_token.unwrap_or_default(),
        expires_at: now + token.expires_in,
        provider: "gemini".into(),
        scopes,
    })
}

/// Refresh an access token using a refresh token.
pub async fn refresh_access_token(
    config: &OAuthClientConfig,
    refresh_token: &str,
) -> Result<TokenCredential> {
    let client = reqwest::Client::new();

    let resp = client
        .post(&config.token_uri)
        .form(&[
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .context("Token refresh request failed")?;

    let status = resp.status();
    let body = resp.text().await.context("Failed to read refresh response")?;

    if !status.is_success() {
        if let Ok(err) = serde_json::from_str::<TokenErrorResponse>(&body) {
            return Err(anyhow!(
                "Token refresh failed: {} — {}",
                err.error,
                err.error_description.unwrap_or_default()
            ));
        }
        return Err(anyhow!("Token refresh failed (HTTP {status}): {body}"));
    }

    let token: TokenResponse = serde_json::from_str(&body)
        .context("Failed to parse refresh response")?;

    let now = now_secs();
    Ok(TokenCredential {
        access_token: token.access_token,
        // Google may not return a new refresh token on refresh.
        refresh_token: token
            .refresh_token
            .unwrap_or_else(|| refresh_token.to_string()),
        expires_at: now + token.expires_in,
        provider: "gemini".into(),
        scopes: token
            .scope
            .map(|s| s.split(' ').map(String::from).collect())
            .unwrap_or_default(),
    })
}

/// Best-effort token revocation.
pub async fn revoke_token(config: &OAuthClientConfig, token: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let _ = client
        .post(&config.revoke_uri)
        .form(&[("token", token)])
        .send()
        .await;
    Ok(())
}

// ── Anthropic / Claude OAuth ────────────────────────────────────────

/// Build the Anthropic OAuth2 authorization URL.
///
/// Differs from Google: no `access_type`, no `prompt`, uses `/callback`
/// redirect path, and includes `code=true` for Claude Max upsell page.
pub fn build_claude_authorization_url(
    config: &AnthropicOAuthConfig,
    redirect_port: u16,
    pkce: &PkceChallenge,
    state: &str,
) -> String {
    let redirect_uri = format!("http://localhost:{redirect_port}/callback");
    let scopes = config.scopes.join(" ");

    format!(
        "{}?code=true&client_id={}&response_type=code&redirect_uri={}&scope={}&\
         code_challenge={}&code_challenge_method=S256&state={}",
        config.authorize_url,
        percent_encode(&config.client_id),
        percent_encode(&redirect_uri),
        percent_encode(&scopes),
        percent_encode(&pkce.code_challenge),
        percent_encode(state),
    )
}

/// Exchange an authorization code for tokens (Anthropic).
///
/// Uses JSON body (not form-encoded) and no `client_secret`.
pub async fn exchange_claude_code(
    config: &AnthropicOAuthConfig,
    code: &str,
    code_verifier: &str,
    redirect_port: u16,
    state: &str,
) -> Result<TokenCredential> {
    let client = reqwest::Client::new();
    let redirect_uri = format!("http://localhost:{redirect_port}/callback");

    let body = serde_json::json!({
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": redirect_uri,
        "client_id": config.client_id,
        "code_verifier": code_verifier,
        "state": state,
    });

    let resp = client
        .post(&config.token_url)
        .json(&body)
        .send()
        .await
        .context("Claude token exchange request failed")?;

    let status = resp.status();
    let resp_body = resp.text().await.context("Failed to read token response")?;

    if !status.is_success() {
        if let Ok(err) = serde_json::from_str::<TokenErrorResponse>(&resp_body) {
            return Err(anyhow!(
                "Token exchange failed: {} — {}",
                err.error,
                err.error_description.unwrap_or_default()
            ));
        }
        return Err(anyhow!("Token exchange failed (HTTP {status}): {resp_body}"));
    }

    let token: TokenResponse = serde_json::from_str(&resp_body)
        .context("Failed to parse token response")?;

    let now = now_secs();
    let scopes = token
        .scope
        .map(|s| s.split(' ').map(String::from).collect())
        .unwrap_or_default();

    Ok(TokenCredential {
        access_token: token.access_token,
        refresh_token: token.refresh_token.unwrap_or_default(),
        expires_at: now + token.expires_in,
        provider: "claude".into(),
        scopes,
    })
}

/// Refresh an access token using a refresh token (Anthropic).
///
/// Uses JSON body, no `client_secret`.
pub async fn refresh_claude_token(
    config: &AnthropicOAuthConfig,
    refresh_token: &str,
) -> Result<TokenCredential> {
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": config.client_id,
    });

    let resp = client
        .post(&config.token_url)
        .json(&body)
        .send()
        .await
        .context("Claude token refresh request failed")?;

    let status = resp.status();
    let resp_body = resp.text().await.context("Failed to read refresh response")?;

    if !status.is_success() {
        if let Ok(err) = serde_json::from_str::<TokenErrorResponse>(&resp_body) {
            return Err(anyhow!(
                "Token refresh failed: {} — {}",
                err.error,
                err.error_description.unwrap_or_default()
            ));
        }
        return Err(anyhow!("Token refresh failed (HTTP {status}): {resp_body}"));
    }

    let token: TokenResponse = serde_json::from_str(&resp_body)
        .context("Failed to parse refresh response")?;

    let now = now_secs();
    Ok(TokenCredential {
        access_token: token.access_token,
        refresh_token: token
            .refresh_token
            .unwrap_or_else(|| refresh_token.to_string()),
        expires_at: now + token.expires_in,
        provider: "claude".into(),
        scopes: token
            .scope
            .map(|s| s.split(' ').map(String::from).collect())
            .unwrap_or_default(),
    })
}

// ── High-level orchestrator ──────────────────────────────────────────

/// Run the full OAuth2 login flow for a provider.
///
/// Opens the browser, waits for callback, exchanges code, saves tokens.
pub async fn run_oauth_flow(provider: &str) -> Result<()> {
    match provider {
        "gemini" | "google" => run_gemini_oauth_flow(provider).await,
        "claude" | "anthropic" => run_claude_oauth_flow().await,
        _ => Err(anyhow!("Unsupported OAuth provider: {provider}\n  Supported: gemini, claude")),
    }
}

/// Gemini (Google) OAuth flow.
async fn run_gemini_oauth_flow(provider: &str) -> Result<()> {
    let config = client_config::load_gemini_config();

    // Check for placeholder credentials.
    if client_config::is_placeholder_config(&config) {
        return Err(anyhow!(
            "OAuth client not configured.\n\
             Create a Google Cloud OAuth client (Desktop type) and save it to:\n  \
             ~/.rust-agent/oauth-clients/gemini.json\n\n\
             Format:\n  {}\n\n\
             See: https://ai.google.dev/gemini-api/docs/oauth",
            serde_json::to_string_pretty(&client_config::default_gemini_config())
                .unwrap_or_default()
        ));
    }

    let pkce = generate_pkce();
    let state = uuid::Uuid::new_v4().to_string();
    let (listener, port) = start_callback_server().await?;
    let auth_url = build_authorization_url(&config, port, &pkce, &state);

    eprintln!("  Opening browser for Google login...");
    if let Err(e) = open_browser(&auth_url) {
        eprintln!("  Could not open browser: {e}");
        eprintln!("  Please open this URL manually:");
        eprintln!("  {auth_url}");
    }

    let code = wait_for_callback(listener, &state, 120).await?;
    let credential = exchange_code(&config, &code, &pkce.code_verifier, port).await?;
    info!(provider, "OAuth token obtained successfully");

    let mut store = CredentialStore::load()?;
    store.set_token(provider, credential);
    store.save()?;

    Ok(())
}

/// Claude (Anthropic) OAuth flow.
async fn run_claude_oauth_flow() -> Result<()> {
    let config = client_config::load_claude_config();

    let pkce = generate_pkce();
    let state = uuid::Uuid::new_v4().to_string();
    let (listener, port) = start_callback_server().await?;
    let auth_url = build_claude_authorization_url(&config, port, &pkce, &state);

    eprintln!("  Opening browser for Anthropic login...");
    if let Err(e) = open_browser(&auth_url) {
        eprintln!("  Could not open browser: {e}");
        eprintln!("  Please open this URL manually:");
        eprintln!("  {auth_url}");
    }

    let code = wait_for_callback(listener, &state, 120).await?;
    let credential = exchange_claude_code(&config, &code, &pkce.code_verifier, port, &state).await?;
    info!("Claude OAuth token obtained successfully");

    let mut store = CredentialStore::load()?;
    store.set_token("claude", credential);
    store.save()?;

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Base64url encoding without padding (RFC 7636).
fn base64_url_encode(data: &[u8]) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    URL_SAFE_NO_PAD.encode(data)
}

/// Minimal percent-encoding for URL query parameters.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~' => out.push(b as char),
            _ => {
                out.push('%');
                out.push_str(&format!("{b:02X}"));
            }
        }
    }
    out
}

/// Parse a query string into key-value pairs.
fn parse_query_string(query: &str) -> std::collections::HashMap<String, String> {
    query
        .split('&')
        .filter(|s| !s.is_empty())
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next()?;
            let val = parts.next().unwrap_or("");
            Some((
                percent_decode(key),
                percent_decode(val),
            ))
        })
        .collect()
}

/// Minimal percent-decoding.
fn percent_decode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().unwrap_or(b'0');
            let lo = chars.next().unwrap_or(b'0');
            let hex = [hi, lo];
            if let Ok(val) = u8::from_str_radix(&String::from_utf8_lossy(&hex), 16) {
                out.push(val);
            }
        } else if b == b'+' {
            out.push(b' ');
        } else {
            out.push(b);
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_verifier_length() {
        let pkce = generate_pkce();
        // 64 bytes → 86 chars base64url (no padding)
        assert_eq!(pkce.code_verifier.len(), 86);
        assert!(!pkce.code_verifier.contains('='));
        assert!(!pkce.code_verifier.contains('+'));
        assert!(!pkce.code_verifier.contains('/'));
    }

    #[test]
    fn pkce_challenge_is_sha256() {
        let pkce = generate_pkce();
        // SHA-256 → 32 bytes → 43 chars base64url
        assert_eq!(pkce.code_challenge.len(), 43);
    }

    #[test]
    fn authorization_url_format() {
        let config = client_config::default_gemini_config();
        let pkce = PkceChallenge {
            code_verifier: "test_verifier".into(),
            code_challenge: "test_challenge".into(),
        };
        let url = build_authorization_url(&config, 8080, &pkce, "mystate");
        assert!(url.starts_with("https://accounts.google.com"));
        assert!(url.contains("client_id="));
        assert!(url.contains("code_challenge=test_challenge"));
        assert!(url.contains("state=mystate"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8080"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
    }

    #[test]
    fn query_string_parsing() {
        let params = parse_query_string("code=abc123&state=xyz&error=test%20val");
        assert_eq!(params.get("code").unwrap(), "abc123");
        assert_eq!(params.get("state").unwrap(), "xyz");
        assert_eq!(params.get("error").unwrap(), "test val");
    }

    #[test]
    fn percent_encode_roundtrip() {
        let original = "hello world/foo@bar=baz";
        let encoded = percent_encode(original);
        let decoded = percent_decode(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn claude_authorization_url_format() {
        let config = client_config::default_claude_config();
        let pkce = PkceChallenge {
            code_verifier: "test_verifier".into(),
            code_challenge: "test_challenge".into(),
        };
        let url = build_claude_authorization_url(&config, 9090, &pkce, "mystate");
        assert!(url.starts_with("https://platform.claude.com/oauth/authorize"));
        assert!(url.contains("client_id=9d1c250a"));
        assert!(url.contains("code_challenge=test_challenge"));
        assert!(url.contains("state=mystate"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A9090%2Fcallback"));
        assert!(url.contains("code=true"));
        // Must NOT have Google-specific params
        assert!(!url.contains("access_type="));
        assert!(!url.contains("prompt="));
    }
}

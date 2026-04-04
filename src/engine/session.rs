//! Session persistence — save and load conversation state to disk.
//!
//! Sessions are stored as JSON files at `~/.rust-agent/sessions/{id}.json`.
//! The [`Session`] struct holds the conversation messages, metadata, and
//! provides methods for saving, loading, and listing available sessions.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

// ── Session storage path ─────────────────────────────────────────────

/// Returns the directory where sessions are stored.
fn sessions_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rust-agent")
        .join("sessions")
}

// ── Session types ────────────────────────────────────────────────────

/// A persisted conversation session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier.
    pub id: String,
    /// Conversation messages in the provider's wire format.
    pub messages: Vec<Value>,
    /// Unix timestamp when the session was created.
    pub created_at: u64,
    /// Working directory when the session started.
    pub cwd: String,
    /// Model used for this session.
    pub model: String,
    /// Provider used for this session.
    pub provider: String,
    /// Optional human-readable session summary.
    pub summary: Option<String>,
}

/// Lightweight metadata for listing sessions without loading full message history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    /// Unique session identifier.
    pub id: String,
    /// Unix timestamp when the session was created.
    pub created_at: u64,
    /// Working directory when the session started.
    pub cwd: String,
    /// Model used.
    pub model: String,
    /// Number of messages in the conversation.
    pub message_count: usize,
    /// Optional summary text.
    pub summary: Option<String>,
}

// ── Session implementation ───────────────────────────────────────────

impl Session {
    /// Create a new session with the given ID and metadata.
    pub fn new(id: String, model: String, provider: String) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            id,
            messages: Vec::new(),
            created_at,
            cwd: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| ".".to_string()),
            model,
            provider,
            summary: None,
        }
    }

    /// Append a message to the session.
    pub fn append_message(&mut self, msg: Value) {
        self.messages.push(msg);
    }

    /// Save the session to disk.
    pub fn save(&self) -> Result<()> {
        let dir = sessions_dir();
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create sessions directory: {}", dir.display()))?;

        let path = dir.join(format!("{}.json", self.id));
        let json = serde_json::to_string_pretty(self)
            .with_context(|| "Failed to serialize session")?;
        fs::write(&path, json)
            .with_context(|| format!("Failed to write session file: {}", path.display()))?;

        Ok(())
    }

    /// Load a session from disk by ID.
    pub fn load(id: &str) -> Result<Self> {
        let path = sessions_dir().join(format!("{}.json", id));
        let json = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read session file: {}", path.display()))?;
        let session: Session = serde_json::from_str(&json)
            .with_context(|| "Failed to deserialize session")?;
        Ok(session)
    }

    /// List all available sessions, sorted by creation time (newest first).
    pub fn list_sessions() -> Result<Vec<SessionSummary>> {
        let dir = sessions_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut summaries = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            if let Ok(json) = fs::read_to_string(&path) {
                if let Ok(session) = serde_json::from_str::<Session>(&json) {
                    summaries.push(SessionSummary {
                        id: session.id,
                        created_at: session.created_at,
                        cwd: session.cwd,
                        model: session.model,
                        message_count: session.messages.len(),
                        summary: session.summary,
                    });
                }
            }
        }

        summaries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(summaries)
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_has_correct_defaults() {
        let session = Session::new(
            "test-id".to_string(),
            "test-model".to_string(),
            "test-provider".to_string(),
        );
        assert_eq!(session.id, "test-id");
        assert_eq!(session.model, "test-model");
        assert!(session.messages.is_empty());
        assert!(session.created_at > 0);
    }

    #[test]
    fn append_message_works() {
        let mut session = Session::new(
            "test-id".to_string(),
            "model".to_string(),
            "provider".to_string(),
        );
        session.append_message(serde_json::json!({"role": "user", "content": "hello"}));
        assert_eq!(session.messages.len(), 1);
    }
}

//! Session persistence — save and load conversation state to disk.
//!
//! **JSONL format** (append-only, crash-safe):
//! `~/.rust-agent/sessions/<project-hash>/<session-id>.jsonl`
//!
//! Each line is a JSON object tagged by `type`:
//! - `header`  — session metadata (first line)
//! - `message` — a single conversation message
//! - `compact_boundary` — marks where older messages were compacted
//! - `cost`    — per-turn token/cost data
//!
//! **Backward compat**: legacy `{id}.json` files in the flat sessions dir
//! are still loadable and listed.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

// ── Session storage paths ───────────────────────────────────────────

/// Returns the root sessions directory: `~/.rust-agent/sessions/`.
pub fn sessions_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rust-agent")
        .join("sessions")
}

/// Deterministic short hash of a working directory path.
///
/// SHA-256 first 8 bytes → 16 hex chars.
fn project_hash(cwd: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(cwd.as_bytes());
    let hash = hasher.finalize();
    hash[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

/// Returns the project-scoped session directory.
///
/// `~/.rust-agent/sessions/<project-hash>/`
pub fn project_sessions_dir(cwd: &str) -> PathBuf {
    sessions_dir().join(project_hash(cwd))
}

// ── JSONL entry types ───────────────────────────────────────────────

/// A single entry in a JSONL session file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEntry {
    /// Session metadata header (always the first line).
    Header {
        session_id: String,
        model: String,
        provider: String,
        cwd: String,
        created_at: u64,
    },
    /// A conversation message (user, assistant, tool, system).
    Message {
        #[serde(flatten)]
        data: Value,
    },
    /// Marks where older messages were compacted away.
    CompactBoundary {
        reason: String,
        preserved_messages: usize,
        timestamp: u64,
    },
    /// Per-turn cost/usage data.
    Cost {
        model: String,
        input_tokens: u64,
        output_tokens: u64,
        cost_usd: f64,
    },
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
    /// Number of messages already written to the JSONL file.
    #[serde(skip)]
    pub saved_message_count: usize,
    /// Whether the JSONL header has been written.
    #[serde(skip)]
    pub header_written: bool,
}

/// Lightweight metadata for listing sessions without loading full history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub created_at: u64,
    pub cwd: String,
    pub model: String,
    pub message_count: usize,
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
            saved_message_count: 0,
            header_written: false,
        }
    }

    /// Append a message to the in-memory session.
    pub fn append_message(&mut self, msg: Value) {
        self.messages.push(msg);
    }

    // ── JSONL path ──────────────────────────────────────────────────

    /// Returns the JSONL file path for this session.
    fn jsonl_path(&self) -> PathBuf {
        project_sessions_dir(&self.cwd).join(format!("{}.jsonl", self.id))
    }

    // ── JSONL writing ───────────────────────────────────────────────

    /// Append a single entry to the JSONL file.
    fn append_entry(&self, entry: &SessionEntry) -> Result<()> {
        let dir = project_sessions_dir(&self.cwd);
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create session dir: {}", dir.display()))?;

        let path = self.jsonl_path();
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to open session: {}", path.display()))?;

        let line = serde_json::to_string(entry)
            .with_context(|| "Failed to serialize session entry")?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    /// Write the JSONL header (called once on first save).
    fn write_header(&mut self) -> Result<()> {
        if self.header_written {
            return Ok(());
        }
        self.append_entry(&SessionEntry::Header {
            session_id: self.id.clone(),
            model: self.model.clone(),
            provider: self.provider.clone(),
            cwd: self.cwd.clone(),
            created_at: self.created_at,
        })?;
        self.header_written = true;
        Ok(())
    }

    /// Save new messages since last save (append-only, no full rewrite).
    pub fn save(&mut self) -> Result<()> {
        self.write_header()?;

        let new_msgs = self.messages[self.saved_message_count..].to_vec();
        for msg in &new_msgs {
            self.append_entry(&SessionEntry::Message { data: msg.clone() })?;
        }
        self.saved_message_count = self.messages.len();
        Ok(())
    }

    /// Append a compact boundary marker.
    pub fn write_compact_boundary(&self, reason: &str, preserved: usize) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.append_entry(&SessionEntry::CompactBoundary {
            reason: reason.to_string(),
            preserved_messages: preserved,
            timestamp: now,
        })
    }

    /// Append a cost entry.
    pub fn write_cost(&self, model: &str, input_tokens: u64, output_tokens: u64, cost_usd: f64) -> Result<()> {
        self.append_entry(&SessionEntry::Cost {
            model: model.to_string(),
            input_tokens,
            output_tokens,
            cost_usd,
        })
    }

    // ── JSONL loading ───────────────────────────────────────────────

    /// Load a session from a JSONL file.
    ///
    /// Parses line-by-line, skips invalid lines, honors compact boundaries
    /// (clears messages before the boundary).
    pub fn load_jsonl(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read session: {}", path.display()))?;

        let mut session: Option<Session> = None;
        let mut messages = Vec::new();

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(entry) = serde_json::from_str::<SessionEntry>(line) else {
                continue; // skip malformed lines (crash-safe)
            };
            match entry {
                SessionEntry::Header {
                    session_id,
                    model,
                    provider,
                    cwd,
                    created_at,
                } => {
                    session = Some(Session {
                        id: session_id,
                        messages: Vec::new(),
                        created_at,
                        cwd,
                        model,
                        provider,
                        summary: None,
                        saved_message_count: 0,
                        header_written: true,
                    });
                }
                SessionEntry::Message { data } => {
                    messages.push(data);
                }
                SessionEntry::CompactBoundary { .. } => {
                    messages.clear(); // honor compaction
                }
                SessionEntry::Cost { .. } => {} // informational, skip
            }
        }

        let mut s = session
            .ok_or_else(|| anyhow::anyhow!("No header found in session file"))?;
        s.saved_message_count = messages.len();
        s.messages = messages;
        Ok(s)
    }

    // ── Legacy JSON support ─────────────────────────────────────────

    /// Load a legacy JSON session by ID (from flat sessions dir).
    pub fn load(id: &str) -> Result<Self> {
        // Try JSONL first (scan project dirs)
        let root = sessions_dir();
        if root.exists() {
            if let Ok(entries) = fs::read_dir(&root) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let jsonl = path.join(format!("{id}.jsonl"));
                        if jsonl.exists() {
                            return Self::load_jsonl(&jsonl);
                        }
                    }
                }
            }
        }

        // Fallback: legacy flat JSON
        let path = root.join(format!("{id}.json"));
        let json = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read session: {}", path.display()))?;
        let session: Session = serde_json::from_str(&json)
            .with_context(|| "Failed to deserialize session")?;
        Ok(session)
    }

    // ── Listing ────────────────────────────────────────────────────

    /// List all available sessions (JSONL + legacy JSON), newest first.
    pub fn list_sessions() -> Result<Vec<SessionSummary>> {
        let root = sessions_dir();
        if !root.exists() {
            return Ok(Vec::new());
        }

        let mut summaries = Vec::new();

        for entry in fs::read_dir(&root)?.flatten() {
            let path = entry.path();

            if path.is_dir() {
                // Scan project-scoped JSONL sessions
                if let Ok(sub) = fs::read_dir(&path) {
                    for sub_entry in sub.flatten() {
                        let sub_path = sub_entry.path();
                        if sub_path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                            if let Some(summ) = summary_from_jsonl(&sub_path) {
                                summaries.push(summ);
                            }
                        }
                    }
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
                // Legacy flat JSON
                if let Some(summ) = summary_from_json(&path) {
                    summaries.push(summ);
                }
            }
        }

        summaries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(summaries)
    }
}

// ── Summary helpers ─────────────────────────────────────────────────

/// Read just the header from a JSONL file to build a summary.
fn summary_from_jsonl(path: &Path) -> Option<SessionSummary> {
    let content = fs::read_to_string(path).ok()?;
    let mut session_id = String::new();
    let mut created_at = 0u64;
    let mut cwd = String::new();
    let mut model = String::new();
    let mut msg_count = 0usize;

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<SessionEntry>(line) else {
            continue;
        };
        match entry {
            SessionEntry::Header {
                session_id: sid,
                model: m,
                cwd: c,
                created_at: ca,
                ..
            } => {
                session_id = sid;
                model = m;
                cwd = c;
                created_at = ca;
            }
            SessionEntry::Message { .. } => {
                msg_count += 1;
            }
            SessionEntry::CompactBoundary { .. } => {
                msg_count = 0; // reset after compaction
            }
            _ => {}
        }
    }

    if session_id.is_empty() {
        return None;
    }

    Some(SessionSummary {
        id: session_id,
        created_at,
        cwd,
        model,
        message_count: msg_count,
        summary: None,
    })
}

/// Build a summary from a legacy JSON session file.
fn summary_from_json(path: &Path) -> Option<SessionSummary> {
    let json = fs::read_to_string(path).ok()?;
    let session: Session = serde_json::from_str(&json).ok()?;
    Some(SessionSummary {
        id: session.id,
        created_at: session.created_at,
        cwd: session.cwd,
        model: session.model,
        message_count: session.messages.len(),
        summary: session.summary,
    })
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
        assert!(!session.header_written);
        assert_eq!(session.saved_message_count, 0);
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

    #[test]
    fn project_hash_deterministic() {
        let h1 = project_hash("/home/user/project");
        let h2 = project_hash("/home/user/project");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16);
        // Different paths produce different hashes
        let h3 = project_hash("/home/user/other");
        assert_ne!(h1, h3);
    }

    #[test]
    fn session_entry_roundtrip() {
        let entry = SessionEntry::Header {
            session_id: "abc".into(),
            model: "gemini".into(),
            provider: "Gemini".into(),
            cwd: "/tmp".into(),
            created_at: 1234,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"type\":\"header\""));
        let parsed: SessionEntry = serde_json::from_str(&json).unwrap();
        match parsed {
            SessionEntry::Header { session_id, .. } => assert_eq!(session_id, "abc"),
            _ => panic!("expected Header"),
        }
    }

    #[test]
    fn message_entry_roundtrip() {
        let entry = SessionEntry::Message {
            data: serde_json::json!({"role": "user", "content": "hi"}),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"type\":\"message\""));
    }

    #[test]
    fn jsonl_save_and_load() {
        let id = format!("test-jsonl-{}", std::process::id());
        let mut session = Session::new(id.clone(), "test-model".into(), "Gemini".into());
        // Override cwd to a temp path for predictable project hash
        session.cwd = "/tmp/test-jsonl-project".into();

        session.append_message(serde_json::json!({"role": "user", "content": "hello"}));
        session.append_message(serde_json::json!({"role": "assistant", "content": "hi"}));
        session.save().expect("save should succeed");

        // Verify JSONL file exists
        let path = session.jsonl_path();
        assert!(path.exists(), "JSONL file should exist at {}", path.display());

        // Load and verify
        let loaded = Session::load_jsonl(&path).expect("load should succeed");
        assert_eq!(loaded.id, id);
        assert_eq!(loaded.model, "test-model");
        assert_eq!(loaded.messages.len(), 2);
        assert!(loaded.header_written);

        // Append more messages and re-save (incremental)
        let mut session2 = loaded;
        session2.append_message(serde_json::json!({"role": "user", "content": "again"}));
        session2.save().expect("incremental save should succeed");

        let reloaded = Session::load_jsonl(&path).expect("reload should succeed");
        assert_eq!(reloaded.messages.len(), 3);

        // Cleanup
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(path.parent().unwrap());
    }

    #[test]
    fn compact_boundary_clears_old_messages() {
        let id = format!("test-compact-{}", std::process::id());
        let mut session = Session::new(id.clone(), "model".into(), "Gemini".into());
        session.cwd = "/tmp/test-compact-project".into();

        session.append_message(serde_json::json!({"role": "user", "content": "old"}));
        session.save().unwrap();

        session.write_compact_boundary("context_limit", 1).unwrap();

        session.append_message(serde_json::json!({"role": "user", "content": "new"}));
        session.save().unwrap();

        let loaded = Session::load_jsonl(&session.jsonl_path()).unwrap();
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.messages[0]["content"], "new");

        // Cleanup
        let path = session.jsonl_path();
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(path.parent().unwrap());
    }
}

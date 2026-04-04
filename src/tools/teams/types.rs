//! Team message types — shared data structures for team collaboration.

use serde::{Deserialize, Serialize};

/// A single message in a team conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMessage {
    /// Unique message identifier.
    pub id: String,
    /// The team (channel / namespace) this message belongs to.
    pub team: String,
    /// Author of the message.
    pub author: String,
    /// Message body.
    pub content: String,
    /// Unix timestamp (seconds).
    pub timestamp: u64,
}

/// Metadata about a team channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamInfo {
    /// Team / channel name.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Number of messages in the team.
    pub message_count: usize,
    /// Unix timestamp of creation.
    pub created_at: u64,
}

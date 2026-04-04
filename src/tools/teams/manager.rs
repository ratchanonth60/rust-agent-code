//! Team storage manager — local-file backed team message store.
//!
//! Teams are stored as JSON files at `~/.rust-agent/teams/{name}.json`.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::types::{TeamInfo, TeamMessage};

// ── Storage path ────────────────────────────────────────────────────

/// Returns the directory where team data is stored.
fn teams_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rust-agent")
        .join("teams")
}

/// Path to a specific team's message file.
fn team_file(name: &str) -> PathBuf {
    teams_dir().join(format!("{}.json", name))
}

// ── Public API ──────────────────────────────────────────────────────

/// Create a new team channel.  Returns an error if it already exists.
pub fn create_team(name: &str, description: Option<&str>) -> Result<TeamInfo> {
    let dir = teams_dir();
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create teams directory: {}", dir.display()))?;

    let path = team_file(name);
    if path.exists() {
        anyhow::bail!("Team '{}' already exists", name);
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let messages: Vec<TeamMessage> = Vec::new();
    let json = serde_json::to_string_pretty(&messages)?;
    fs::write(&path, json)
        .with_context(|| format!("Failed to write team file: {}", path.display()))?;

    Ok(TeamInfo {
        name: name.to_string(),
        description: description.map(String::from),
        message_count: 0,
        created_at: now,
    })
}

/// Delete a team channel and all its messages.
pub fn delete_team(name: &str) -> Result<()> {
    let path = team_file(name);
    if !path.exists() {
        anyhow::bail!("Team '{}' does not exist", name);
    }
    fs::remove_file(&path)
        .with_context(|| format!("Failed to delete team file: {}", path.display()))
}

/// Send a message to a team channel.
pub fn send_message(team: &str, author: &str, content: &str) -> Result<TeamMessage> {
    let path = team_file(team);
    if !path.exists() {
        anyhow::bail!("Team '{}' does not exist. Create it first.", team);
    }

    let json = fs::read_to_string(&path)?;
    let mut messages: Vec<TeamMessage> = serde_json::from_str(&json)
        .unwrap_or_default();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let msg = TeamMessage {
        id: format!("{}-{}", team, messages.len() + 1),
        team: team.to_string(),
        author: author.to_string(),
        content: content.to_string(),
        timestamp: now,
    };
    messages.push(msg.clone());

    let json = serde_json::to_string_pretty(&messages)?;
    fs::write(&path, json)?;

    Ok(msg)
}

/// List all teams.
pub fn list_teams() -> Result<Vec<TeamInfo>> {
    let dir = teams_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut teams = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let name = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        if let Ok(json) = fs::read_to_string(&path) {
            let messages: Vec<TeamMessage> = serde_json::from_str(&json).unwrap_or_default();
            let created_at = entry.metadata()
                .and_then(|m| m.created())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            teams.push(TeamInfo {
                name,
                description: None,
                message_count: messages.len(),
                created_at,
            });
        }
    }

    Ok(teams)
}

/// Get messages from a team, optionally limited to the most recent N.
pub fn get_messages(team: &str, limit: Option<usize>) -> Result<Vec<TeamMessage>> {
    let path = team_file(team);
    if !path.exists() {
        anyhow::bail!("Team '{}' does not exist", team);
    }

    let json = fs::read_to_string(&path)?;
    let messages: Vec<TeamMessage> = serde_json::from_str(&json).unwrap_or_default();

    Ok(match limit {
        Some(n) => messages.into_iter().rev().take(n).collect(),
        None => messages,
    })
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn cleanup(name: &str) {
        let _ = fs::remove_file(team_file(name));
    }

    #[test]
    fn create_and_delete_team() {
        let name = &format!("test-team-{}", std::process::id());
        cleanup(name);

        let info = create_team(name, Some("test team")).unwrap();
        assert_eq!(info.name, *name);
        assert_eq!(info.message_count, 0);

        // Should not allow creating duplicate
        assert!(create_team(name, None).is_err());

        delete_team(name).unwrap();
        assert!(delete_team(name).is_err()); // Already deleted
    }

    #[test]
    fn send_and_get_messages() {
        let name = &format!("test-msgs-{}", std::process::id());
        cleanup(name);

        create_team(name, None).unwrap();
        send_message(name, "alice", "hello").unwrap();
        send_message(name, "bob", "world").unwrap();

        let msgs = get_messages(name, None).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].author, "alice");
        assert_eq!(msgs[1].author, "bob");

        let recent = get_messages(name, Some(1)).unwrap();
        assert_eq!(recent.len(), 1);

        cleanup(name);
    }
}

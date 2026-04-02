//! Domain models — task lifecycle, conversation messages, and attachments.

use serde::{Deserialize, Serialize};

/// The environment or execution mode an agent task operates within.
///
/// Maps 1-to-1 with the TypeScript `TaskType` union.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    LocalBash,
    LocalAgent,
    RemoteAgent,
    InProcessTeammate,
    LocalWorkflow,
    MonitorMcp,
    Dream,
}

/// Lifecycle status of an agent task.
///
/// A task progresses from [`Pending`](Self::Pending) →
/// [`Running`](Self::Running) → one of the terminal states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Killed,
}

impl TaskStatus {
    /// Returns `true` when no further state transitions are possible.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_agent::models::TaskStatus;
    /// assert!(TaskStatus::Completed.is_terminal());
    /// assert!(!TaskStatus::Running.is_terminal());
    /// ```
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Killed)
    }
}

/// Base representation of an agent task's state.
///
/// Mirrors the TypeScript `TaskStateBase` interface. Currently declared
/// for forward-compatibility; not yet wired into the runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStateBase {
    pub id: String,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub description: String,
    pub tool_use_id: Option<String>,
    pub start_time: u64,
    pub end_time: Option<u64>,
    pub total_paused_ms: Option<u64>,
    pub output_file: String,
    pub output_offset: usize,
    pub notified: bool,
}

/// Participant role in a conversation turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

/// A single message in the conversation history.
///
/// # Examples
///
/// ```
/// use rust_agent::models::Message;
/// let msg = Message::new_user("hello");
/// assert_eq!(msg.content, "hello");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    pub attachments: Vec<Attachment>,
}

/// A file or data blob attached to a [`Message`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    #[serde(rename = "type")]
    pub attachment_type: String,
    pub name: String,
    pub species: Option<String>,
    pub file_path: Option<String>,
}

impl Message {
    /// Creates a [`Role::User`] message with empty attachments.
    pub fn new_user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            attachments: vec![],
        }
    }

    /// Creates a [`Role::System`] message with empty attachments.
    pub fn new_system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            attachments: vec![],
        }
    }
}

use clap::ValueEnum;

/// Permission modes controlling how tool execution is authorized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PermissionMode {
    /// Ask for everything except read-only tools in the working directory.
    Default,
    /// Auto-allow file writes within the working directory.
    AcceptEdits,
    /// Auto-allow nearly all operations (still respects dangerous path checks).
    BypassPermissions,
    /// Read-only mode: deny all destructive tools.
    Plan,
    /// Non-interactive: convert all "ask" decisions to "deny".
    DontAsk,
}

impl std::fmt::Display for PermissionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::AcceptEdits => write!(f, "accept-edits"),
            Self::BypassPermissions => write!(f, "bypass"),
            Self::Plan => write!(f, "plan"),
            Self::DontAsk => write!(f, "dont-ask"),
        }
    }
}

/// The result of a permission check.
#[derive(Debug, Clone)]
pub enum PermissionDecision {
    /// Tool execution is allowed.
    Allow,
    /// Tool execution is denied.
    Deny { reason: String },
    /// The user should be prompted for decision.
    Ask { tool_name: String, description: String },
}

/// A persistent permission rule (from settings or session).
#[derive(Debug, Clone)]
pub struct PermissionRule {
    pub tool_name: String,
    /// Optional content pattern (e.g., "git *" for Bash commands).
    pub pattern: Option<String>,
    pub behavior: RuleBehavior,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleBehavior {
    Allow,
    Deny,
    Ask,
}

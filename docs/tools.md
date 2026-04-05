# Tool System

> How tools work in Rust Agent — the trait, built-in tools, and extending.

---

## The `Tool` Trait

Every tool implements a single async trait defined in `src/tools/mod.rs`:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique name used by the LLM for function calling.
    fn name(&self) -> &str;

    /// Human-readable description shown in the tool schema.
    fn description(&self) -> &str;

    /// JSON Schema describing the tool's input parameters.
    fn input_schema(&self) -> Value;

    /// Execute the tool with the given input.
    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult>;

    // Optional capability flags:
    fn is_destructive(&self) -> bool { false }
    fn is_read_only(&self) -> bool { false }
    fn is_concurrency_safe(&self) -> bool { false }
    fn aliases(&self) -> Vec<&str> { vec![] }
}
```

### ToolContext

Passed to every tool invocation with session-level state:

```rust
pub struct ToolContext {
    pub auto_mode: bool,          // skip permission prompts
    pub debug: bool,              // verbose output
    pub tools_available: Vec<String>,
    pub max_budget_usd: Option<f64>,
    pub cwd: PathBuf,             // working directory
    pub permission_mode: PermissionMode,
    pub session_id: Option<String>,
    pub is_agent: bool,           // true for sub-agent invocations
}
```

### ToolResult

```rust
pub struct ToolResult {
    pub output: Value,   // JSON response sent back to the LLM
    pub is_error: bool,  // marks the result as an error
}
```

---

## Built-in Tools (23)

### File Operations

| Tool | Description | Flags |
|------|-------------|-------|
| **Read** | Read file contents with optional line range (`offset`, `limit`) | read-only |
| **Write** | Create or overwrite files | destructive |
| **Edit** | Exact string replacement with uniqueness guard | destructive |
| **NotebookEdit** | Jupyter `.ipynb` cell editing (replace, insert, delete) | destructive |

### Search

| Tool | Description | Flags |
|------|-------------|-------|
| **Glob** | File pattern matching (`**/*.rs`), sorted by mtime | read-only |
| **Grep** | Content search via ripgrep with regex support | read-only |

### Execution

| Tool | Description | Flags |
|------|-------------|-------|
| **Bash** | Shell command execution with configurable timeout | destructive |
| **Sleep** | Async wait (1–300 seconds) | read-only |
| **BackgroundTask** | Spawn a shell command as a background process | destructive |
| **TaskOutput** | Read stdout/stderr from a background task | read-only |
| **TaskStop** | Stop a running background task | destructive |

### Web

| Tool | Description | Flags |
|------|-------------|-------|
| **WebFetch** | Fetch URL content with HTML tag stripping | read-only |
| **WebSearch** | Web search queries | read-only |

### Workflow

| Tool | Description | Flags |
|------|-------------|-------|
| **TodoWrite** | Structured task checklist with shared state | — |
| **EnterPlanMode** | Switch to planning mode (read-only tools) | — |
| **ExitPlanMode** | Exit planning mode | — |

### Git

| Tool | Description | Flags |
|------|-------------|-------|
| **EnterWorktree** | Create an isolated git worktree | destructive |
| **ExitWorktree** | Clean up and exit worktree | destructive |

### Agent & Collaboration

| Tool | Description | Flags |
|------|-------------|-------|
| **Agent** | Spawn a sub-agent with a fresh `QueryEngine` | concurrency-safe |
| **CreateTeam** | Create a named team of collaborating agents | — |
| **DeleteTeam** | Delete a team | — |
| **SendTeamMessage** | Send a message to a team channel | — |

### System

| Tool | Description | Flags |
|------|-------------|-------|
| **AskUserQuestion** | Interactive prompt via TUI (mpsc + oneshot) | — |
| **Config** | Read/write agent configuration | — |
| **Skill** | Invoke user-defined skill prompt templates | — |

---

## Tool Registration

Tools are constructed in `src/tools/registry.rs` by `default_tools()`:

```rust
pub fn default_tools(
    todo_list: SharedTodoList,
    question_tx: Option<QuestionSender>,
) -> (Vec<Box<dyn Tool + Send + Sync>>, SharedTaskRegistry) {
    let task_registry = crate::tasks::new_shared_registry();
    // ... construct tools, passing shared state ...
    (tools, task_registry)
}
```

The returned `SharedTaskRegistry` is stored in `QueryEngine` and passed to the
TUI for status pill rendering.

`AgentTool` is added separately via `QueryEngine::with_agent_tool()` — it's
excluded from sub-agent engines to prevent infinite recursion.

---

## Permission Flow

Before every tool execution, the engine calls `check_tool_permission()`:

```
Tool invocation
  │
  ▼
check_permission(tool, input, mode, cwd, rules)
  │
  ├─ Allow  → execute immediately
  ├─ Deny   → return "Permission denied"
  └─ Ask    → send to TUI → user responds (y/n/a) → execute or deny
```

The permission mode, tool flags (`is_destructive`, `is_read_only`), and path
safety checks all factor into the decision. "Always Allow" responses create
session-scoped rules so the user isn't re-prompted.

---

## Writing a Custom Tool

1. Create a new module under `src/tools/` (e.g. `src/tools/my_tool/mod.rs`)
2. Implement the `Tool` trait
3. Register it in `src/tools/registry.rs`

```rust
use async_trait::async_trait;
use serde_json::{json, Value};
use crate::tools::{Tool, ToolContext, ToolResult};

pub struct MyTool;

#[async_trait]
impl Tool for MyTool {
    fn name(&self) -> &str { "MyTool" }

    fn description(&self) -> &str {
        "Does something useful."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "Input message"
                }
            },
            "required": ["message"]
        })
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let msg = input["message"].as_str().unwrap_or("hello");
        Ok(ToolResult::ok(json!({ "result": format!("Got: {}", msg) })))
    }
}
```

Then add `Box::new(MyTool)` to the tools vector in `default_tools()`.

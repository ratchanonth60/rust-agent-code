# rust-agent

Native Rust CLI agent inspired by Claude Code. Connects to any OpenAI-compatible LLM (Gemini, OpenAI, etc.) and runs an agentic tool-use loop with a built-in TUI.

## Features

- **Agentic tool-use loop** — sends queries to an LLM, dispatches tool calls, feeds results back, repeats until the model produces a final answer
- **6 built-in tools** — Bash, ReadFile, WriteFile, Edit, Glob, Grep
- **Interactive TUI** — Ratatui-based terminal UI with conversation history, input prompt, and tool execution spinner
- **One-shot mode** — pass `--query` for non-interactive scripting
- **Multi-provider** — OpenAI and Gemini via OpenAI-compatible API
- **Persistent memory** — file-based memory system at `~/.rust-agent/memory/`
- **Output styles** — load custom markdown style definitions from `~/.rust-agent/output-styles/`
- **Cost tracking** — per-model token usage and cost breakdown

## Quick Start

### Prerequisites

- Rust 1.70+
- [ripgrep](https://github.com/BurntSushi/ripgrep) (`rg`) — required by GrepTool

### Build

```bash
cd rust-agent
cargo build --release
```

### Configure

Create a `.env` file or export environment variables:

```bash
# For Gemini (default)
export GEMINI_API_KEY=your-key-here

# For OpenAI
export OPENAI_API_KEY=your-key-here
```

### Run

```bash
# Interactive TUI mode
cargo run

# One-shot query
cargo run -- --query "list all .rs files in src/"

# Use OpenAI instead of Gemini
cargo run -- --provider openai --model gpt-4o

# Auto mode (skip permission prompts)
cargo run -- --auto --query "create hello.txt"
```

## CLI Options

| Flag | Description | Default |
|---|---|---|
| `-q, --query <TEXT>` | One-shot query (skips TUI) | — |
| `-m, --model <NAME>` | Model name | `gemini-3-flash-preview` |
| `-p, --provider <NAME>` | `gemini` or `openai` | `gemini` |
| `--auto` | Skip permission prompts | `false` |
| `--bare` | Minimal UI | `false` |

## Architecture

```
CLI (clap) → main.rs → QueryEngine → LLM API
                │             │             ↑
                │             ↓             │
                │       Tool Dispatch → Tool Execution → Tool Results
                │                                        (back to LLM)
                ↓
          TUI (ratatui) ←─ mpsc channels ─→ Engine (tokio task)
```

### Tool System

Every tool implements the `Tool` trait:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;
    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult>;
    fn is_destructive(&self) -> bool { false }
    fn is_read_only(&self) -> bool { false }
}
```

Built-in tools:

| Tool | File | Description |
|---|---|---|
| `bash` | `tools/bash/executor.rs` | Shell command execution with timeout |
| `read_file` | `tools/fs/read_file.rs` | Read file contents |
| `write_file` | `tools/fs/write_file.rs` | Write file with overwrite protection |
| `Edit` | `tools/edit/edit_file.rs` | Exact string replacement with uniqueness guard |
| `Glob` | `tools/glob_tool/search.rs` | File pattern matching |
| `Grep` | `tools/grep_tool/search.rs` | Content search via ripgrep |

### Project Structure

```
src/
├── main.rs                 # Entry point, CLI args, mode dispatch
├── output_styles.rs        # Markdown output style loading
├── engine/
│   ├── mod.rs              # Re-exports
│   ├── query.rs            # QueryEngine + agentic loop
│   └── cost_tracker.rs     # Token/cost tracking
├── tools/
│   ├── mod.rs              # Tool trait, ToolContext, ToolResult
│   ├── bash/executor.rs    # BashTool
│   ├── fs/read_file.rs     # ReadFileTool
│   ├── fs/write_file.rs    # WriteFileTool
│   ├── edit/edit_file.rs   # FileEditTool
│   ├── glob_tool/search.rs # GlobTool
│   └── grep_tool/search.rs # GrepTool
├── models/mod.rs           # Task, Message, Role types
├── ui/
│   ├── mod.rs              # Terminal setup/restore
│   └── app.rs              # Ratatui TUI + event loop
├── mem/mod.rs              # Memory system prompt builder
└── keybindings/mod.rs      # KeyEvent → Action mapping
```

### Keybindings (TUI)

| Key | Action |
|---|---|
| `Enter` | Submit query |
| `Esc` | Clear input |
| `Ctrl+C` | Interrupt |
| `Ctrl+D` | Exit |
| `Ctrl+L` | Redraw |

## Customization

### Memory

The agent maintains persistent memory at `~/.rust-agent/memory/MEMORY.md`. It will read and update this file across sessions to build context about your preferences and projects.

### Output Styles

Drop `.md` files into `~/.rust-agent/output-styles/` (global) or `.rust-agent/output-styles/` (project-local) to customize how the agent formats its responses. YAML frontmatter is stripped automatically.

## Roadmap

See [planning.md](planning.md) for the full implementation roadmap. Key upcoming phases:

- **Context system** — git status, CLAUDE.md project instructions
- **Permission system** — interactive approve/deny for destructive tools
- **Streaming** — real-time token-by-token output
- **Slash commands** — `/help`, `/clear`, `/cost`, `/model`
- **Sub-agents** — AgentTool for parallel task delegation
- **MCP support** — Model Context Protocol client

## Dependencies

| Crate | Purpose |
|---|---|
| `tokio` | Async runtime |
| `async-openai` | OpenAI-compatible API client |
| `ratatui` + `crossterm` | Terminal UI |
| `clap` | CLI argument parsing |
| `serde` + `serde_json` | Serialization |
| `anyhow` | Error handling |
| `glob` | File pattern matching |
| `regex` | Regex for output style frontmatter stripping |
| `tracing` | Structured logging |

## License

Private — not yet licensed for distribution.

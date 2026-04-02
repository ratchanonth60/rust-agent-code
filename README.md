# rust-agent

Native Rust CLI agent inspired by Claude Code. Supports Claude (native Anthropic API), OpenAI, Gemini, and any OpenAI-compatible provider. Runs an agentic tool-use loop with a built-in TUI.

## Features

- **Multi-provider** -- Claude (native API), OpenAI, Gemini, OpenAI-compatible via `async-openai`
- **Agentic tool-use loop** -- sends queries to an LLM, dispatches tool calls, feeds results back, repeats until the model produces a final answer
- **10 built-in tools** -- Bash, ReadFile, WriteFile, Edit, Glob, Grep, TodoWrite, Sleep, WebFetch, AskUserQuestion
- **Claude SSE streaming** -- real-time token-by-token output in TUI mode
- **Permission system** -- 5 modes (Default, AcceptEdits, BypassPermissions, Plan, DontAsk) with interactive Y/n/a prompts and dangerous path blocking
- **Context system** -- auto-loads CLAUDE.md instructions, git status, and system info into prompts
- **Interactive TUI** -- Ratatui-based terminal UI styled after Claude Code
- **Three execution modes** -- interactive TUI, one-shot (`--query`), bare stdin/stdout (`--bare`)
- **Cost tracking** -- per-model token usage and USD cost breakdown
- **Context management** -- token estimation, microcompact (clears old tool results near context limit)
- **Persistent memory** -- file-based memory at `~/.rust-agent/memory/`
- **Output styles** -- custom markdown style definitions from `~/.rust-agent/output-styles/`
- **Keybindings** -- configurable key mappings with 17 action contexts

## Quick Start

### Prerequisites

- Rust 1.70+
- [ripgrep](https://github.com/BurntSushi/ripgrep) (`rg`) -- required by GrepTool

### Build

```bash
cd rust-agent
cargo build --release
```

### Configure

Create a `.env` file or export environment variables:

```bash
# For Claude (default for Claude provider)
export ANTHROPIC_API_KEY=your-key-here

# For Gemini (default provider)
export GEMINI_API_KEY=your-key-here

# For OpenAI
export OPENAI_API_KEY=your-key-here

# For OpenAI-compatible providers
export OPENAI_COMPAT_API_KEY=your-key-here
export OPENAI_COMPAT_API_BASE=https://your-api-base/v1
```

### Run

```bash
# Interactive TUI mode (default: Gemini)
cargo run

# Use Claude
cargo run -- --provider claude

# One-shot query
cargo run -- --query "list all .rs files in src/"

# Use OpenAI
cargo run -- --provider openai --model gpt-4o

# Bare mode (stdin/stdout, no TUI)
cargo run -- --bare --provider claude

# With permission mode and budget cap
cargo run -- --permission-mode accept-edits --max-budget 1.00

# Auto mode (skip all permission prompts)
cargo run -- --auto --query "create hello.txt"
```

## CLI Options

| Flag                       | Description                                                         | Default           |
| -------------------------- | ------------------------------------------------------------------- | ----------------- |
| `-q, --query <TEXT>`       | One-shot query (skips TUI)                                          | --                |
| `--bare`                   | Bare mode: stdin/stdout, no TUI                                     | `false`           |
| `--auto`                   | Skip all permission prompts                                         | `false`           |
| `--provider <NAME>`        | `claude`, `gemini`, `openai`, `open-ai-compatible`                  | `gemini`          |
| `--model <NAME>`           | Model name override                                                 | auto per provider |
| `--api-key <KEY>`          | API key override                                                    | from env          |
| `--api-base <URL>`         | API base URL override                                               | from env          |
| `--max-tokens <N>`         | Max output tokens per LLM call                                      | `8192`            |
| `--max-budget <USD>`       | Session budget cap in USD                                           | unlimited         |
| `--permission-mode <MODE>` | `default`, `accept-edits`, `bypass-permissions`, `plan`, `dont-ask` | `default`         |

### Default Models

| Provider          | Default Model              |
| ----------------- | -------------------------- |
| Claude            | `claude-sonnet-4-20250514` |
| Gemini            | `gemini-2.5-pro`           |
| OpenAI            | `gpt-4o-mini`              |
| OpenAI-compatible | `gpt-4o-mini`              |

## Architecture

```
CLI (clap) --> main.rs --> QueryEngine --> LLM API (Anthropic / OpenAI / Gemini)
                |                |                    ^
                |                v                    |
                |          Tool Dispatch --> Tool Execution --> Tool Results
                |          (Tool trait)     (10 built-in)      (back to LLM)
                |
                +---> TUI (ratatui) <-- mpsc channels --> Engine (tokio task)
                +---> Bare mode (stdin/stdout)
                +---> Context System (CLAUDE.md, git, sysinfo)
                +---> Permission System (allow/deny/ask per tool)
                +---> Cost Tracker (token usage, $/model, budget cap)
                +---> Compaction (microcompact when nearing context limit)
```

## Tool System

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
    fn is_concurrency_safe(&self) -> bool { false }
}
```

### Built-in Tools

| Tool              | Description                                    |
| ----------------- | ---------------------------------------------- |
| `Bash`            | Shell command execution with timeout           |
| `Read`            | Read file contents with optional line range    |
| `Write`           | Write/create files                             |
| `Edit`            | Exact string replacement with uniqueness guard |
| `Glob`            | File pattern matching (sorted by mtime)        |
| `Grep`            | Content search via ripgrep                     |
| `TodoWrite`       | Task checklist with shared state               |
| `Sleep`           | Async wait (1-300 seconds)                     |
| `WebFetch`        | URL fetch with HTML tag stripping              |
| `AskUserQuestion` | Interactive prompts via TUI channel            |

## Permission System

The permission checker runs before every tool execution:

| Mode                 | Behavior                                                  |
| -------------------- | --------------------------------------------------------- |
| `default`            | Ask for destructive tools, allow read-only                |
| `accept-edits`       | Allow file edits within working directory, ask for others |
| `bypass-permissions` | Allow everything (except dangerous paths)                 |
| `plan`               | Deny all destructive tools                                |
| `dont-ask`           | Deny destructive tools silently                           |

Dangerous paths (`.env`, `.ssh/`, `.git/`, `.gnupg/`) are always blocked regardless of mode.

In TUI mode, permission prompts show: `Allow? (y)es / (n)o / (a)lways`

## TUI

The terminal UI follows Claude Code's visual style:

- `>` prompt for user input
- `⎿` prefix for assistant responses
- `●` status dot for tool execution (dim=running, green=done, red=error)
- `─` horizontal dividers between conversation turns
- Status line showing active tool and session cost
- Borderless, clean layout

### Keybindings

| Key      | Action       |
| -------- | ------------ |
| `Enter`  | Submit query |
| `Esc`    | Clear input  |
| `Ctrl+C` | Interrupt    |
| `Ctrl+D` | Exit         |
| `Ctrl+L` | Redraw       |

### Slash Commands

| Command  | Description                |
| -------- | -------------------------- |
| `/help`  | Show available commands    |
| `/clear` | Clear conversation history |
| `/cost`  | Show token usage and cost  |
| `/exit`  | Exit the agent             |

## Project Structure

```
src/
  main.rs                    # Entry point, CLI args, 3 execution modes
  engine/
    mod.rs                   # Re-exports
    config.rs                # EngineConfig (auto_mode, permission_mode, etc.)
    query.rs                 # QueryEngine + agentic loop (OpenAI + Claude)
    streaming.rs             # Claude SSE stream parser
    cost_tracker.rs          # Token/cost tracking per model
    tokens.rs                # Token estimation + context window map
    compaction.rs            # Microcompact + auto-compact prompt
  tools/
    mod.rs                   # Tool trait, ToolContext, ToolResult
    bash/executor.rs         # BashTool
    fs/read_file.rs          # ReadFileTool
    fs/write_file.rs         # WriteFileTool
    edit/edit_file.rs        # FileEditTool
    glob_tool/search.rs      # GlobTool
    grep_tool/search.rs      # GrepTool
    todo/mod.rs              # TodoWriteTool
    sleep/mod.rs             # SleepTool
    web_fetch/mod.rs         # WebFetchTool
    ask_user/mod.rs          # AskUserQuestionTool
  ui/
    mod.rs                   # Terminal setup/restore
    app.rs                   # Ratatui TUI + event loop + permission prompts
  models/mod.rs              # Task, Message, Role types
  mem/mod.rs                 # Memory system prompt builder
  output_styles.rs           # Output style loading
  keybindings/               # Configurable key mappings (17 contexts)
  permissions/               # Permission system (types, checker, path_safety)
  context/                   # CLAUDE.md + git status + system info
```

## Dependencies

| Crate                   | Purpose                                   |
| ----------------------- | ----------------------------------------- |
| `tokio`                 | Async runtime                             |
| `reqwest`               | HTTP client (Claude native API, WebFetch) |
| `async-openai`          | OpenAI-compatible API client              |
| `ratatui` + `crossterm` | Terminal UI                               |
| `clap`                  | CLI argument parsing                      |
| `serde` + `serde_json`  | Serialization                             |
| `anyhow`                | Error handling                            |
| `glob`                  | File pattern matching                     |
| `regex`                 | Regex (GrepTool, frontmatter stripping)   |
| `futures-util`          | Stream utilities (SSE parsing)            |
| `tracing`               | Structured logging                        |
| `dotenvy`               | .env file loading                         |

## Roadmap

See [planning.md](planning.md) for the full implementation roadmap.

Remaining (Phase 7):

- Session persistence (JSONL save/resume)
- Command history (up-arrow recall)
- MCP client (JSON-RPC over stdio)
- Sub-agents (AgentTool)
- Tool concurrency (parallel read-only)
- OpenAI streaming

## License

Private -- not yet licensed for distribution.

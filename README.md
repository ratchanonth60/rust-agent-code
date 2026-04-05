# rust-agent

Native Rust CLI agent inspired by Claude Code. Supports Claude (native Anthropic API), OpenAI, Gemini, and any OpenAI-compatible provider. Runs an agentic tool-use loop with a built-in TUI, MCP client, plugin/skill system, and team collaboration.

## Features

### Core
- **Multi-provider** — Claude (native API), OpenAI, Gemini, OpenAI-compatible via `async-openai`
- **Agentic tool-use loop** — sends queries to an LLM, dispatches tool calls, feeds results back, repeats until the model produces a final answer
- **23 built-in tools** — file I/O, shell execution, search, web, tasks, planning, worktrees, teams, sub-agents, skills, and more
- **Three execution modes** — interactive TUI, one-shot (`--query`), bare stdin/stdout (`--bare`)

### Streaming
- **Claude SSE streaming** — real-time token-by-token output via native Anthropic API
- **OpenAI streaming** — via async-openai `create_stream`
- **Gemini streaming** — raw HTTP SSE with `thought_signature` preservation for thinking models

### TUI
- **Interactive TUI** — Ratatui-based terminal UI styled after Claude Code
- **Dialog overlays** — model picker, theme picker, and full settings dialog (`/settings`)
- **Autocomplete** — slash commands and `@file` references with debounced file scanning
- **Markdown rendering** — pulldown-cmark with styled headings, bold, italic, code blocks, lists
- **Syntax highlighting** — syntect-based with `base16-ocean.dark` theme
- **Diff viewer** — color-coded unified diffs
- **Animated UI** — hero title, typewriter tagline, braille spinners, rail dividers, blinking cursor

### Systems
- **Permission system** — 5 modes with path safety guards and per-session rules
- **Context system** — auto-loads CLAUDE.md instructions, git status, and system info into prompts
- **Cost tracking** — per-model token usage and USD cost breakdown with budget enforcement
- **Context management** — token estimation, microcompact, auto-compact with LLM summarization and circuit breaker
- **Session persistence** — JSON save/load/list/resume at `~/.rust-agent/sessions/`
- **Persistent memory** — file-based at `~/.rust-agent/memory/` with MEMORY.md index
- **Keybindings** — 18 contexts, 70+ actions, chord sequences, user overrides via `~/.rust-agent/keybindings.json`
- **33 slash commands** — configuration, git, session, MCP, skills, diagnostics, and more
- **MCP client** — JSON-RPC 2.0 over stdio/SSE/HTTP, tool/resource discovery from `~/.rust-agent/mcp.json`
- **Plugin system** — `~/.rust-agent/plugins/` with JSON manifests and hook scripts
- **Skill system** — markdown prompt templates from `~/.rust-agent/skills/`
- **Output styles** — custom markdown style definitions from `~/.rust-agent/output-styles/`
- **Sub-agents** — AgentTool spawns fresh QueryEngine instances for parallel work

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
CLI (clap) ──→ main.rs ──→ QueryEngine ──→ LLM API (Anthropic/OpenAI/Gemini)
                │                │                    ↑
                │                ↓                    │
                │          Tool Dispatch ───→ Tool Execution ───→ Tool Results
                │          (Tool trait)       (23 built-in)       (back to LLM)
                │
                ├──→ TUI (ratatui) ←── mpsc channels ──→ Engine (tokio task)
                │     ├── Dialog overlays (model/theme/settings)
                │     ├── Autocomplete (commands + @file)
                │     ├── Markdown + syntax highlighting
                │     └── Diff viewer
                ├──→ Bare mode (stdin/stdout, no TUI)
                ├──→ Context System (CLAUDE.md, git status, system info)
                ├──→ Permission System (5 modes, path safety, session rules)
                ├──→ Cost Tracker (token usage, $/model, budget enforcement)
                ├──→ Compaction (microcompact + LLM auto-compact, circuit breaker)
                ├──→ Session Persistence (JSON save/load/list/resume)
                ├──→ MCP Client (JSON-RPC 2.0 over stdio/SSE/HTTP)
                ├──→ Plugin System (~/.rust-agent/plugins/)
                └──→ Skill System (~/.rust-agent/skills/)
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

### Built-in Tools (23)

| Tool                | Description                                    |
| ------------------- | ---------------------------------------------- |
| `Bash`              | Shell command execution with timeout           |
| `Read`              | Read file contents with optional line range    |
| `Write`             | Write/create files                             |
| `Edit`              | Exact string replacement with uniqueness guard |
| `Glob`              | File pattern matching (sorted by mtime)        |
| `Grep`              | Content search via ripgrep                     |
| `NotebookEdit`      | Jupyter .ipynb cell editing                    |
| `TodoWrite`         | Task checklist with shared state               |
| `Sleep`             | Async wait (1-300 seconds)                     |
| `WebFetch`          | URL fetch with HTML tag stripping              |
| `WebSearch`         | Web search queries                             |
| `AskUserQuestion`   | Interactive prompts via TUI channel            |
| `BackgroundTask`    | Launch background shell processes              |
| `TaskOutput`        | Read output from background tasks              |
| `TaskStop`          | Stop running background tasks                  |
| `EnterPlanMode`     | Switch to planning mode (read-only tools)      |
| `ExitPlanMode`      | Exit planning mode                             |
| `EnterWorktree`     | Create isolated git worktree                   |
| `ExitWorktree`      | Clean up and exit worktree                     |
| `ConfigTool`        | Read/write agent configuration                 |
| `SkillTool`         | Invoke user-defined skill prompts              |
| `CreateTeam`        | Create a team of sub-agents                    |
| `AgentTool`         | Spawn a sub-agent for complex tasks            |

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

- `>` prompt for user input with animated glyph alternation
- `⎿` prefix for assistant responses
- `●` status dot for tool execution (spinner=running, green=done, red=error)
- Animated `─╌┄` rail dividers between conversation turns
- Status line with activity state, braille spinner, and session cost
- Hero title "RUST AGENT" with typewriter tagline animation
- Borderless, clean layout

### Dialog Overlays

| Dialog     | Trigger      | Description                                           |
| ---------- | ------------ | ----------------------------------------------------- |
| Settings   | `/settings`  | Full configuration editor with grouped settings       |
| Model      | `/model`     | Model picker grouped by provider                      |
| Theme      | `/theme`     | Theme picker with custom output styles                |

Settings dialog features:
- Three groups: General, Display, Provider
- Navigate with ↑/↓ (or j/k), cycle values with ←/→ (or h/l) or Enter
- Auto-saves to `~/.rust-agent/config.json` on close

### Keybindings

| Key      | Action       |
| -------- | ------------ |
| `Enter`  | Submit query |
| `Esc`    | Clear input  |
| `Ctrl+C` | Interrupt    |
| `Ctrl+D` | Exit         |
| `Ctrl+L` | Redraw       |
| `Tab`    | Accept autocomplete |
| `Up/Down` | Scroll / History navigation |

Custom keybindings can be defined in `~/.rust-agent/keybindings.json`.

### Slash Commands (33)

| Command         | Description                           | Type    |
| --------------- | ------------------------------------- | ------- |
| `/help`         | Show available commands               | Local   |
| `/clear`        | Clear conversation history            | Local   |
| `/cost`         | Show token usage and cost             | Local   |
| `/exit`         | Exit the agent                        | Local   |
| `/settings`     | Open interactive settings dialog      | Local   |
| `/config`       | Show or set configuration             | Local   |
| `/model`        | Select or set model (opens dialog)    | Local   |
| `/theme`        | Select theme (opens dialog)           | Local   |
| `/output-style` | Set output style                      | Local   |
| `/vim`          | Toggle vim input mode                 | Local   |
| `/effort`       | Set reasoning effort level            | Local   |
| `/fast`         | Toggle fast mode                      | Local   |
| `/plan`         | Toggle plan mode                      | Local   |
| `/permissions`  | View/manage permission rules          | Local   |
| `/stats`        | Show session statistics               | Local   |
| `/status`       | Show current status                   | Local   |
| `/context`      | Show context info                     | Local   |
| `/keybindings`  | Show keybinding configuration         | Local   |
| `/doctor`       | Run diagnostics                       | Local   |
| `/memory`       | Show/manage persistent memory         | Local   |
| `/diff`         | Show git diff                         | Local   |
| `/branch`       | Show/manage git branches              | Local   |
| `/commit`       | Generate commit message               | Prompt  |
| `/review`       | Review code changes                   | Prompt  |
| `/compact`      | Compact conversation context          | Prompt  |
| `/export`       | Export conversation                   | Local   |
| `/resume`       | Resume a previous session             | Local   |
| `/mcp`          | MCP server management                 | Local   |
| `/skill`        | Run a skill prompt template           | Prompt  |

## Configuration

### Persistent Config (`~/.rust-agent/config.json`)

```json
{
  "editor_mode": "normal",
  "theme": "default",
  "default_model": "gemini-2.5-pro",
  "default_provider": "gemini",
  "output_style": null
}
```

### Configuration Files

| File | Location | Purpose |
|------|----------|---------|
| `config.json` | `~/.rust-agent/` | Global preferences |
| `keybindings.json` | `~/.rust-agent/` | Custom key mappings |
| `mcp.json` | `~/.rust-agent/` or `.rust-agent/` | MCP server definitions |
| `MEMORY.md` | `~/.rust-agent/memory/` | Agent memory index |
| `*.md` | `~/.rust-agent/output-styles/` | Custom output formatting |
| `plugin.json` | `~/.rust-agent/plugins/{name}/` | Plugin manifests |
| `*.md` | `~/.rust-agent/skills/` | Skill prompt templates |
| `*.json` | `~/.rust-agent/sessions/` | Session persistence |
| `.env` | Project root | API keys and provider config |
| `CLAUDE.md` | Global and project directories | Project instructions |

## MCP (Model Context Protocol)

The built-in MCP client supports:

- **Transport**: stdio, SSE, and HTTP
- **Discovery**: automatic tool and resource listing from connected servers
- **Proxy tools**: remote MCP tools appear as native `Tool` trait objects
- **Config**: `~/.rust-agent/mcp.json` or `.rust-agent/mcp.json` per project

## Project Structure

```
src/
  main.rs                           # Entry point, CLI args, 3 execution modes
  engine/
    mod.rs                          # Re-exports
    config.rs                       # EngineConfig (auto_mode, permission_mode, etc.)
    query.rs                        # QueryEngine + agentic loop (Claude + OpenAI + Gemini)
    streaming.rs                    # Claude SSE stream parser
    cost_tracker.rs                 # Token/cost tracking per model
    tokens.rs                       # Token estimation + context window map
    compaction.rs                   # Microcompact + auto-compact + circuit breaker
    session.rs                      # Session persistence (JSON save/load/list)
    state.rs                        # SharedEngineState (Arc<RwLock<...>>)
    agent_tool.rs                   # Sub-agent tool (AgentTool)
  tools/
    mod.rs                          # Tool trait, ToolContext, ToolResult
    registry.rs                     # default_tools() builder (23 tools)
    bash/                           # BashTool (shell execution)
    fs/                             # ReadFileTool, WriteFileTool
    edit/                           # FileEditTool (exact string replacement)
    glob_tool/                      # GlobTool (pattern matching)
    grep_tool/                      # GrepTool (ripgrep wrapper)
    notebook/                       # NotebookEditTool (.ipynb)
    todo/                           # TodoWriteTool (shared state)
    sleep/                          # SleepTool (async wait)
    web_fetch/                      # WebFetchTool (HTML stripping)
    web_search/                     # WebSearchTool
    ask_user/                       # AskUserQuestionTool (mpsc + oneshot)
    tasks/                          # BackgroundTaskTool, TaskOutputTool, TaskStopTool
    plan_mode/                      # EnterPlanModeTool, ExitPlanModeTool
    worktree/                       # EnterWorktreeTool, ExitWorktreeTool
    config_tool/                    # ConfigTool (runtime config)
    skill_tool/                     # SkillTool (invoke user-defined skills)
    teams/                          # CreateTeamTool, DeleteTeamTool, SendTeamMessageTool
  ui/
    mod.rs                          # Terminal setup/restore
    app.rs                          # App struct + event loop + slash commands
    app/autocomplete.rs             # Autocomplete engine (commands + @file)
    app/render.rs                   # Rendering: conversation, status, prompt, dialogs
    dialogs/mod.rs                  # Dialog system (ActiveDialog, Dialog trait)
    dialogs/model_picker.rs         # Model selection dialog
    dialogs/theme_picker.rs         # Theme selection dialog
    dialogs/settings_dialog.rs      # Full settings editor dialog
    diff_viewer.rs                  # Unified diff renderer
    highlight.rs                    # Syntect-based code highlighting
    markdown.rs                     # pulldown-cmark markdown → ratatui
  commands/
    mod.rs                          # build_default_registry() (33 commands)
    types.rs                        # Command trait, CommandResult, CommandContext
    registry.rs                     # CommandRegistry (name + alias lookup)
    help.rs, clear.rs, cost.rs      # Core commands
    exit.rs, config_cmd.rs          # Core commands
    settings_cmd.rs                 # Settings dialog command
    model.rs, theme.rs              # Configuration commands
    output_style.rs, vim.rs         # Configuration commands
    effort.rs, fast.rs, plan.rs     # Mode commands
    permissions_cmd.rs              # Permission management
    stats.rs, status.rs, context.rs # Information commands
    keybindings_cmd.rs, doctor.rs   # Diagnostics
    memory.rs                       # Memory management
    diff.rs, branch.rs              # Git commands
    commit.rs, review.rs            # Git prompt commands
    compact.rs, export.rs           # Session management
    resume.rs                       # Session resume
    mcp.rs, skill.rs                # MCP + Skills
  keybindings/                      # Full keybinding system
    types.rs                        # 18 contexts, 70+ actions, chord types
    default_bindings.rs             # Built-in defaults
    loader.rs                       # Loads ~/.rust-agent/keybindings.json
    parser.rs                       # "ctrl+k" → ParsedKeystroke
    matcher.rs                      # KeyEvent → binding match
    resolver.rs                     # resolve_key() with chord state
    reserved.rs                     # Reserved shortcuts
  permissions/                      # 5-mode permission system
    types.rs                        # PermissionMode, PermissionDecision, PermissionRule
    checker.rs                      # check_permission() decision chain
    path_safety.rs                  # Dangerous path detection
  context/                          # System prompt context injection
    claudemd.rs                     # CLAUDE.md loader (global + project)
    git.rs                          # Git branch, status, recent log
    system_info.rs                  # OS, arch, cwd, shell
  config/mod.rs                     # GlobalConfig (persistent at ~/.rust-agent/config.json)
  models/mod.rs                     # TaskType, TaskStatus, Role, Message, Attachment
  mem/mod.rs                        # Memory system (file-based, MEMORY.md index)
  output_styles.rs                  # Output style loading from *.md files
  plugins/                          # Plugin system (~/.rust-agent/plugins/)
  skills/                           # Skill system (markdown prompt templates)
  mcp/                              # MCP client (JSON-RPC 2.0)
    types.rs                        # JSON-RPC 2.0, McpToolDef, McpResource
    transport.rs                    # Stdio/SSE/HTTP transport
    client.rs                       # MCP client lifecycle
    manager.rs                      # McpManager (multi-server)
    tools.rs                        # McpProxyTool wrapper
```

## Dependencies

| Crate                   | Purpose                                     |
| ----------------------- | ------------------------------------------- |
| `tokio`                 | Async runtime                               |
| `reqwest`               | HTTP client (Claude native API, WebFetch)   |
| `async-openai`          | OpenAI-compatible API client                |
| `ratatui` + `crossterm` | Terminal UI                                 |
| `clap`                  | CLI argument parsing                        |
| `serde` + `serde_json`  | Serialization                               |
| `anyhow`                | Error handling                              |
| `glob`                  | File pattern matching                       |
| `regex`                 | Regex (GrepTool, path extraction)           |
| `futures-util`          | Stream utilities (SSE parsing)              |
| `tracing`               | Structured logging                          |
| `dotenvy`               | .env file loading                           |
| `pulldown-cmark`        | Markdown parsing for TUI                    |
| `syntect`               | Syntax highlighting for code blocks         |
| `uuid`                  | Session ID generation                       |
| `chrono`                | Date/time handling                          |
| `ferris-says`           | Welcome banner                              |

## Roadmap

See [planning.md](planning.md) for the full implementation roadmap.

Remaining work:

- Parallel tool execution for OpenAI/Gemini providers
- OAuth / API key management (`/login`, `/logout`)
- GitHub integration (`/pr`, `/issue`)
- IDE bridge (VS Code extension)
- Coordinator mode (multi-agent orchestration)
- Advanced task types (DreamTask, RemoteAgentTask)
- Sandbox/container execution
- Voice integration
- Tips system and auto-update

## License

MIT

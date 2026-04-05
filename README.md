<p align="center">
  <img src="rust-agent.png" width="256" alt="Rust" />
</p>

<h1 align="center">Rust Agent</h1>

<p align="center">
  <strong>AI coding companion for your terminal</strong><br />
  A native Rust CLI agent inspired by Claude Code
</p>

<p align="center">
  <a href="#quick-start">Quick Start</a> &bull;
  <a href="#providers">Providers</a> &bull;
  <a href="#tui">TUI</a> &bull;
  <a href="#tools">Tools</a> &bull;
  <a href="#configuration">Configuration</a> &bull;
  <a href="docs/architecture.md">Architecture</a>
</p>

---

## What is Rust Agent?

Rust Agent runs an **agentic tool-use loop** — it sends your query to an LLM, inspects the response for tool calls, executes them, feeds the results back, and repeats until the model produces a final answer. It ships with 23 built-in tools, a full TUI, and three execution modes.

## Quick Start

### Prerequisites

- **Rust 1.70+**
- **[ripgrep](https://github.com/BurntSushi/ripgrep)** (`rg`) — required by GrepTool

### One-Line Install

```bash
curl -fsSL https://raw.githubusercontent.com/ratchanonth60/rust-agent-code/master/install.sh | bash
```

The install script checks dependencies, builds from source, and installs to `~/.local/bin/`. Run `install.sh --help` for options.

### Manual Install

```bash
# Clone and build
git clone https://github.com/ratchanonth60/rust-agent-code.git
cd rust-agent-code
cargo build --release

# Set your API key
export GEMINI_API_KEY=your-key-here    # default provider
# or
export ANTHROPIC_API_KEY=your-key-here # for Claude
# or
export OPENAI_API_KEY=your-key-here    # for OpenAI

# Launch the TUI
cargo run
```

### Uninstall

```bash
./install.sh --uninstall
```

### Execution Modes

```bash
# Interactive TUI (default)
cargo run

# One-shot query
cargo run -- --query "explain src/main.rs"

# Bare mode — stdin/stdout, no TUI
cargo run -- --bare
```

---

## Providers

Rust Agent supports four LLM backends. Switch with `--provider`:

| Provider | Flag | Default Model | API Key Env |
|----------|------|---------------|-------------|
| **Gemini** | `--provider gemini` | `gemini-2.5-flash` | `GEMINI_API_KEY` |
| **Claude** | `--provider claude` | `claude-sonnet-4-6` | `ANTHROPIC_API_KEY` |
| **OpenAI** | `--provider openai` | `gpt-4o-mini` | `OPENAI_API_KEY` |
| **OpenAI-compatible** | `--provider open-ai-compatible` | `gpt-4o-mini` | `OPENAI_COMPAT_API_KEY` |

Each provider has its own streaming implementation:

- **Claude** — native Anthropic Messages API with SSE + parallel tool execution
- **OpenAI** — async-openai with `create_stream`
- **Gemini** — raw HTTP SSE with `thought_signature` preservation for thinking models

---

## TUI

The terminal interface follows Claude Code's visual style:

```
  ✦ RUST AGENT  ─╌┄╌─╌┄╌─╌┄╌─╌┄╌─╌┄╌─╌┄╌
    AI coding companion for your terminal

│ > list all .rs files in src/
│   ⎿ Let me search for Rust files...
  ⠹ Glob
  ✓ Glob
│   ⎿ Found 47 .rs files across the project...

 ● rust-agent | ready ─╌┄╌─╌┄╌─ 1 task ─╌─ $0.0023
❯ _
```

**Features:**
- Animated hero title with typewriter tagline
- Braille spinners for tool execution and streaming
- Animated `─╌┄` rail dividers
- Background task pill in the status line
- Slash command and `@file` autocomplete
- Dialog overlays — `/settings`, `/model`, `/theme`
- Markdown rendering with syntax highlighting and diff viewer
- Permission prompts: `Allow? (y)es / (n)o / (a)lways`

### Keybindings

| Key | Action |
|-----|--------|
| `Enter` | Submit query |
| `Esc` | Clear input |
| `Ctrl+C` | Interrupt |
| `Ctrl+D` | Exit |
| `Tab` | Accept autocomplete |
| `Up/Down` | Scroll or history |

Custom keybindings: `~/.rust-agent/keybindings.json`

---

## Tools

23 built-in tools, all implementing the same `Tool` trait:

| Category | Tools |
|----------|-------|
| **File I/O** | `Read`, `Write`, `Edit`, `NotebookEdit` |
| **Search** | `Glob`, `Grep` |
| **Execution** | `Bash`, `Sleep`, `BackgroundTask`, `TaskOutput`, `TaskStop` |
| **Web** | `WebFetch`, `WebSearch` |
| **Workflow** | `TodoWrite`, `EnterPlanMode`, `ExitPlanMode` |
| **Git** | `EnterWorktree`, `ExitWorktree` |
| **Agent** | `Agent` (sub-agents), `CreateTeam`, `DeleteTeam`, `SendTeamMessage` |
| **System** | `AskUserQuestion`, `Config`, `Skill` |

See [docs/tools.md](docs/tools.md) for the full tool reference.

---

## Slash Commands

33 slash commands for in-session control:

| Command | Description |
|---------|-------------|
| `/help` | Show available commands |
| `/settings` | Open interactive settings dialog |
| `/model` | Switch model (opens picker) |
| `/cost` | Token usage and session cost |
| `/diff` | Show git diff |
| `/commit` | Generate commit message |
| `/review` | Review code changes |
| `/compact` | Compact conversation context |
| `/resume` | Resume a previous session |
| `/mcp` | MCP server management |
| `/doctor` | Run diagnostics |

*...and 22 more. Type `/help` in the TUI to see all.*

---

## Permission System

Every tool invocation passes through a permission checker before execution:

| Mode | Behavior |
|------|----------|
| `default` | Ask for destructive tools, auto-allow read-only |
| `accept-edits` | Allow file edits in the working directory |
| `bypass-permissions` | Allow everything except dangerous paths |
| `plan` | Deny all destructive tools |
| `dont-ask` | Deny destructive tools silently |

Dangerous paths (`.env`, `.ssh/`, `.git/`, `.gnupg/`) are **always** blocked.

```bash
cargo run -- --permission-mode accept-edits --max-budget 1.00
```

---

## Configuration

### CLI Flags

| Flag | Description | Default |
|------|-------------|---------|
| `-q, --query <TEXT>` | One-shot query | — |
| `--bare` | Bare mode (no TUI) | `false` |
| `--auto` | Skip permission prompts | `false` |
| `--provider <NAME>` | LLM provider | `gemini` |
| `--model <NAME>` | Model override | auto |
| `--max-tokens <N>` | Output token limit | `8192` |
| `--max-budget <USD>` | Session budget cap | unlimited |
| `--permission-mode <MODE>` | Permission mode | `default` |

### Config Files

| File | Location | Purpose |
|------|----------|---------|
| `config.json` | `~/.rust-agent/` | Global preferences |
| `keybindings.json` | `~/.rust-agent/` | Custom key mappings |
| `mcp.json` | `~/.rust-agent/` | MCP server definitions |
| `MEMORY.md` | `~/.rust-agent/memory/` | Persistent agent memory |
| `CLAUDE.md` | Project root | Project instructions |
| `.env` | Project root | API keys |

---

## Subsystems

| System | Description |
|--------|-------------|
| **Cost Tracking** | Per-model token usage and USD cost with budget enforcement |
| **Context Management** | Token estimation, microcompact, LLM auto-compact with circuit breaker |
| **Session Persistence** | JSON save/load/list/resume at `~/.rust-agent/sessions/` |
| **Persistent Memory** | File-based at `~/.rust-agent/memory/` with MEMORY.md index |
| **Task Registry** | Unified background task tracking (shell + agent) with TUI pill |
| **MCP Client** | JSON-RPC 2.0 over stdio/SSE/HTTP with tool/resource discovery |
| **Plugin System** | JSON manifests and hook scripts in `~/.rust-agent/plugins/` |
| **Skill System** | Markdown prompt templates in `~/.rust-agent/skills/` |
| **Output Styles** | Custom formatting definitions in `~/.rust-agent/output-styles/` |
| **Sub-agents** | `AgentTool` spawns isolated `QueryEngine` instances |

---

## Documentation

| Document | Description |
|----------|-------------|
| [docs/architecture.md](docs/architecture.md) | Project structure, module map, data flow |
| [docs/tools.md](docs/tools.md) | Tool trait, built-in tools, writing custom tools |
| [planning.md](planning.md) | Implementation roadmap |

---

## Dependencies

| Crate | Purpose |
|-------|---------|
| `tokio` | Async runtime |
| `reqwest` | HTTP client |
| `async-openai` | OpenAI-compatible API |
| `ratatui` + `crossterm` | Terminal UI |
| `clap` | CLI argument parsing |
| `serde` + `serde_json` | Serialization |
| `pulldown-cmark` | Markdown rendering |
| `syntect` | Syntax highlighting |

---

## Roadmap

See [planning.md](planning.md) for the full roadmap. Key upcoming work:

- Parallel tool execution for OpenAI/Gemini
- OAuth / API key management
- GitHub integration (`/pr`, `/issue`)
- Coordinator mode (multi-agent orchestration)
- Sandbox/container execution

---

## License

MIT

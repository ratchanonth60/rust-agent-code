# Rust Agent — Reverse Engineering Planning

> Port of Claude Code (TypeScript) → Rust-based AI Agent CLI
> Status: **Phase 1 MVP done** — core agentic loop works with 3 tools + TUI

---

## Architecture Overview (Target)

```
CLI (clap) ──→ main.rs ──→ QueryEngine ──→ LLM API (Anthropic/OpenAI/Gemini)
                │                │                    ↑
                │                ↓                    │
                │          Tool Dispatch ───→ Tool Execution ───→ Tool Results
                │          (Tool trait)       (bash, fs, grep..)  (back to LLM)
                │
                ├──→ TUI (ratatui) ←── mpsc channels ──→ Engine (tokio task)
                │
                ├──→ Context System (CLAUDE.md, git status, system prompt)
                ├──→ Permission System (allow/deny/ask per tool)
                ├──→ Cost Tracker (token usage, $/model)
                ├──→ Session History (append-only log)
                └──→ Compaction Service (summarize when context full)
```

---

## What's Done ✅

### Core Engine (`engine/`)
- [x] `QueryEngine` with agentic tool-use loop (call LLM → tool calls → execute → feed back → repeat)
- [x] Multi-provider support via OpenAI-compatible API (`ModelProvider::OpenAI`, `ModelProvider::Gemini`)
- [x] System prompt composition (memory + output styles)
- [x] Tool dispatch by name matching
- [x] UI event channel integration (`ToolStarted`/`ToolFinished`)
- [x] `CostTracker` struct defined (but **not wired into engine yet**)

### Tool System (`tools/`)
- [x] `Tool` trait with `name()`, `description()`, `input_schema()`, `call()`, `is_destructive()`, `is_read_only()`, `is_concurrency_safe()`
- [x] `ToolContext` (debug, auto_mode, tools_available, max_budget_usd)
- [x] `ToolResult` with `ok()`/`err()` constructors
- [x] **BashTool** — command execution with timeout, stdout/stderr/exit_code
- [x] **ReadFileTool** — file reading
- [x] **WriteFileTool** — file creation/writing with overwrite protection

### Models (`models/`)
- [x] `TaskType`, `TaskStatus`, `TaskStateBase` (defined, not used yet)
- [x] `Role`, `Message`, `Attachment` types

### UI (`ui/`)
- [x] Ratatui TUI with 2-panel layout (conversation + prompt)
- [x] Async event loop (engine ↔ UI via mpsc channels)
- [x] Spinner animation for running tools
- [x] Terminal setup/teardown with panic hook

### Keybindings (`keybindings/`)
- [x] Static resolver: Ctrl+C, Ctrl+D, Ctrl+L, Enter, Esc, Up/Down

### Memory (`mem/`)
- [x] Memory system prompt builder (file-based `~/.rust-agent/memory/`)
- [x] ⚠️ **BUG**: `build_memory_prompt()` — `format!()` returns early, code after line 36 is unreachable. Variable `prompt` is never bound.

### Output Styles (`output_styles.rs`)
- [x] Load markdown style definitions from `~/.rust-agent/output-styles/` and `./.rust-agent/output-styles/`

### CLI
- [x] `--query` (one-shot mode), `--bare`, `--auto` flags via clap

---

## What's NOT Done Yet 🔲

### Priority 1 — Critical Missing Pieces

#### 1.1 Fix Known Bugs
- [ ] Fix `mem/mod.rs` — `build_memory_prompt()` unreachable code (bind `format!()` result to `let mut prompt = ...`)
- [ ] Wire `--auto` flag through to `ToolContext.auto_mode`
- [ ] Wire `--bare` flag to actually change behavior (skip TUI chrome?)

#### 1.2 Anthropic API Native Support
> The TS version uses Anthropic Messages API directly, not OpenAI-compatible. This is important for: extended thinking, caching, tool_use stop_reason, streaming.
- [ ] Add `ModelProvider::Anthropic` variant
- [ ] Implement direct Anthropic Messages API client (or use `anthropic-sdk` crate if available)
- [ ] Support `tool_use` stop_reason properly (currently only checks `tool_calls` field)
- [ ] Streaming response support (`text_delta`, `tool_use` events)

#### 1.3 More Tools (high-value, port from TS)
- [ ] **FileEditTool** — exact string replacement in files (the `Edit` tool in TS). This is the most-used tool.
- [ ] **GlobTool** — file pattern matching (`glob` crate already in deps)
- [ ] **GrepTool** — content search (use `grep` crate or shell out to `rg`)
- [ ] **AgentTool** — sub-agent spawning (recursive QueryEngine call with isolated context)
- [ ] **TodoWriteTool** — structured task list management (write to state/file)

#### 1.4 Permission System
> In TS, this is a multi-layer system: rules → hooks → classifier → user prompt
- [ ] `PermissionMode` enum: `allowAll`, `default`, `deny`
- [ ] `PermissionRule` type: tool + pattern → allow/deny/ask
- [ ] `check_permission()` function called before each `tool.call()`
- [ ] Interactive permission prompt in TUI (Y/n/always)
- [ ] Settings file loading (`~/.rust-agent/settings.json`) for persistent rules

#### 1.5 Context System (System Prompt)
> The TS version has a rich context system: CLAUDE.md files, git status, date, environment info.
- [ ] `get_system_context()` — git branch, recent commits, working tree status
- [ ] `get_user_context()` — load CLAUDE.md from cwd and parent dirs
- [ ] Inject environment info (platform, shell, date, model name) into system prompt
- [ ] Compose system prompt sections: base instructions + memory + context + output styles

#### 1.6 Wire CostTracker into Engine
- [ ] Track `input_tokens` / `output_tokens` from API response `usage` field
- [ ] Track API call duration
- [ ] Track tool execution duration
- [ ] Display cost on exit (`/cost` command or at session end)
- [ ] Max budget enforcement (`max_budget_usd` in ToolContext)

---

### Priority 2 — Important Features

#### 2.1 Tool Concurrency
> TS runs read-only tools in parallel, write tools serially.
- [ ] Partition tool calls by `is_read_only()` / `is_concurrency_safe()`
- [ ] Execute concurrent batch with `tokio::join!` / `FuturesUnordered`
- [ ] Execute write tools sequentially

#### 2.2 Conversation History & Session Resume
- [ ] Append-only session log file (JSON lines)
- [ ] `--resume` flag to reload previous session
- [ ] Session ID generation

#### 2.3 Context Compaction
> When context window fills up, summarize older messages to free tokens.
- [ ] Token counting (approximate: chars/4 or use tiktoken-rs)
- [ ] Auto-compaction trigger at threshold
- [ ] Compaction prompt (summarize conversation so far)
- [ ] Replace old messages with summary message

#### 2.4 Settings & Configuration
- [ ] `~/.rust-agent/settings.json` — model, permission rules, MCP servers, hooks
- [ ] `.rust-agent/settings.local.json` — project-level overrides
- [ ] Environment variable overrides (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, etc.)
- [ ] `ConfigTool` — runtime config changes via LLM

#### 2.5 Command System (Slash Commands)
- [ ] `/help`, `/clear`, `/compact`, `/cost`, `/exit`, `/model`, `/config`
- [ ] Command parsing from user input (detect `/` prefix)
- [ ] Command registry pattern (similar to tool registry)

#### 2.6 Improved TUI
- [ ] Scrolling in conversation history (currently no scroll)
- [ ] Markdown rendering in terminal (basic: bold, code blocks, lists)
- [ ] Syntax highlighting for code blocks
- [ ] Multi-line input support
- [ ] Command history with Up/Down arrow (currently stubbed)
- [ ] Diff display for file edits
- [ ] Color themes

---

### Priority 3 — Advanced Features

#### 3.1 MCP (Model Context Protocol) Client
> This is a big subsystem. The TS version supports stdio, SSE, and StreamableHTTP transports.
- [ ] MCP client over stdio transport
- [ ] MCP tool wrapping (dynamic tools from MCP servers)
- [ ] MCP resource reading
- [ ] MCP server configuration in settings
- [ ] MCP server lifecycle management

#### 3.2 Sub-Agent / Coordinator Mode
- [ ] `AgentTool` running isolated sub-queries
- [ ] Agent memory snapshots
- [ ] Coordinator mode (main agent only delegates)
- [ ] Inter-agent messaging (`SendMessageTool`)

#### 3.3 Task System
- [ ] Background task management (spawn, list, stop, get output)
- [ ] `LocalShellTask` — background bash commands
- [ ] `LocalAgentTask` — background agent queries
- [ ] Task status tracking and notification

#### 3.4 Hook System
> TS supports command/prompt/agent/http hook types triggered on events.
- [ ] Hook configuration in settings
- [ ] Pre/post tool execution hooks
- [ ] Pre/post query hooks

#### 3.5 Plugin System
- [ ] Plugin loading from directory
- [ ] Plugin-provided tools and commands
- [ ] Plugin enable/disable in settings

#### 3.6 Git Integration
- [ ] Git status in context
- [ ] Commit message generation
- [ ] PR creation via `gh` CLI
- [ ] Worktree support (`EnterWorktreeTool` / `ExitWorktreeTool`)

#### 3.7 LSP Integration
- [ ] LSP client over stdio (JSON-RPC)
- [ ] Diagnostic collection from language servers
- [ ] `LSPTool` for model to query diagnostics

---

## File-by-file Mapping: TS → Rust

| TypeScript Source | Rust Target | Status |
|---|---|---|
| `Tool.ts` | `tools/mod.rs` (Tool trait) | ✅ Done (simplified) |
| `tools/BashTool/` | `tools/bash/executor.rs` | ✅ Done |
| `tools/FileReadTool/` | `tools/fs/read_file.rs` | ✅ Done |
| `tools/FileWriteTool/` | `tools/fs/write_file.rs` | ✅ Done |
| `tools/FileEditTool/` | `tools/fs/edit_file.rs` | 🔲 TODO |
| `tools/GlobTool/` | `tools/glob/` | 🔲 TODO |
| `tools/GrepTool/` | `tools/grep/` | 🔲 TODO |
| `tools/AgentTool/` | `tools/agent/` | 🔲 TODO |
| `tools/TodoWriteTool/` | `tools/todo/` | 🔲 TODO |
| `tools/WebFetchTool/` | `tools/web_fetch/` | 🔲 TODO |
| `tools/EnterPlanModeTool/` | `tools/plan_mode/` | 🔲 TODO |
| `tools/SkillTool/` | `tools/skill/` | 🔲 TODO |
| `tools/MCPTool/` | `tools/mcp/` | 🔲 TODO |
| `query.ts` | `engine/query.rs` | ✅ Partial (no streaming, no compaction) |
| `query/config.ts` | `engine/config.rs` | 🔲 TODO |
| `query/stopHooks.ts` | `engine/stop_hooks.rs` | 🔲 TODO |
| `query/tokenBudget.ts` | `engine/token_budget.rs` | 🔲 TODO |
| `context.ts` | `context/mod.rs` | 🔲 TODO |
| `Task.ts` | `models/mod.rs` (TaskStateBase) | ✅ Done (types only) |
| `tasks.ts` + `tasks/` | `tasks/` | 🔲 TODO |
| `commands.ts` + `commands/` | `commands/` | 🔲 TODO |
| `tools.ts` (registry) | `tools/registry.rs` | 🔲 TODO |
| `cost-tracker.ts` | `engine/cost_tracker.rs` | ✅ Struct done, not wired |
| `history.ts` | `history/` | 🔲 TODO |
| `services/api/claude.ts` | `services/api/` | 🔲 TODO |
| `services/mcp/client.ts` | `services/mcp/` | 🔲 TODO |
| `services/compact/` | `services/compact/` | 🔲 TODO |
| `services/tools/toolOrchestration.ts` | `engine/orchestration.rs` | 🔲 TODO |
| `state/AppStateStore.ts` | `state/` | 🔲 TODO |
| `hooks/useCanUseTool.tsx` | `permissions/` | 🔲 TODO |
| `types/permissions.ts` | `permissions/types.rs` | 🔲 TODO |
| `schemas/hooks.ts` | `schemas/hooks.rs` | 🔲 TODO |
| `constants/` | `constants/` | 🔲 TODO |

---

## Recommended Implementation Order

```
Sprint 1 (Foundation):
  1. Fix mem bug
  2. FileEditTool (most important missing tool)
  3. GlobTool + GrepTool
  4. Wire CostTracker
  5. Wire --auto and --bare flags

Sprint 2 (Context & Permissions):
  6. Context system (CLAUDE.md, git status, env info)
  7. Anthropic API native client
  8. Permission system (basic allow/deny)
  9. Settings file loading
  10. Streaming responses

Sprint 3 (Usability):
  11. Slash commands (/help, /clear, /cost, /compact, /model)
  12. Session history & resume
  13. Context compaction
  14. TUI improvements (scroll, markdown, history)
  15. Tool concurrency (parallel read-only)

Sprint 4 (Advanced):
  16. AgentTool (sub-agents)
  17. MCP client (stdio transport)
  18. Task system (background tasks)
  19. Hook system
  20. Git integration helpers
```

---

## Key Architectural Differences: TS → Rust

| Aspect | TypeScript (Claude Code) | Rust Agent |
|---|---|---|
| UI Framework | Ink (React for terminal) | Ratatui (immediate-mode TUI) |
| Async Runtime | Node.js event loop | Tokio |
| API Client | Custom fetch-based | `async-openai` (OpenAI-compat) |
| State Management | External store + React hooks | TBD — likely Arc<Mutex<AppState>> |
| Tool Dispatch | Dynamic JS objects | Trait objects `Box<dyn Tool>` |
| Permission UI | Ink components | TUI overlay / crossterm prompts |
| Plugin System | Dynamic require/import | TBD — likely dylib or WASM |
| MCP Transport | Node streams | Tokio io / reqwest |
| Config Format | JSON (settings.json) | JSON (serde_json) |
| Streaming | Node readable streams | Tokio streams / async iterators |

---

## Notes

- The `async-openai` crate works well for OpenAI-compatible APIs but doesn't support Anthropic-native features (extended thinking, prompt caching, tool_use stop_reason). Consider using `reqwest` directly for Anthropic API.
- `indicatif` is in deps but unused — could be used for progress bars in one-shot mode.
- `reqwest` is in deps but only used indirectly by `async-openai` — useful for direct Anthropic API calls.
- The `cli/` and `utils/` directories are empty placeholders ready for expansion.

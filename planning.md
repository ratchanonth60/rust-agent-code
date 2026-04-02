# Rust Agent — Implementation Planning & Progress

> Port of Claude Code (TypeScript) → Rust-based AI Agent CLI
> Supports: Claude (native API) + OpenAI/Gemini/OpenAI-compatible (via async-openai)

---

## Architecture

```
CLI (clap) ──→ main.rs ──→ QueryEngine ──→ LLM API (Anthropic/OpenAI/Gemini)
                │                │                    ↑
                │                ↓                    │
                │          Tool Dispatch ───→ Tool Execution ───→ Tool Results
                │          (Tool trait)       (bash, fs, grep..)  (back to LLM)
                │
                ├──→ TUI (ratatui) ←── mpsc channels ──→ Engine (tokio task)
                ├──→ Bare mode (stdin/stdout, no TUI)
                ├──→ Context System (CLAUDE.md, git status, system prompt)
                ├──→ Permission System (allow/deny/ask per tool)
                ├──→ Cost Tracker (token usage, $/model, budget enforcement)
                ├──→ Session History (JSONL append-only log)
                └──→ Compaction Service (summarize when context full)
```

---

## Progress Tracker

### Phase 1: Bug Fixes & Quick Wins ✅ DONE

- [x] **1.1** Register missing tools — `FileEditTool`, `GlobTool`, `GrepTool` added to `QueryEngine::new()`
- [x] **1.2** Create `EngineConfig` (`src/engine/config.rs`) — auto_mode, bare_mode, debug, max_budget_usd, max_tokens
- [x] **1.3** Wire CLI flags → `EngineConfig` → `ToolContext` (no more hardcoded `auto_mode: true`)
- [x] **1.4** Fix `max_tokens` — configurable via `--max-tokens` flag (default 8192, was hardcoded 1024)
- [x] **1.5** Wire `CostTracker` into engine — `Arc<Mutex<CostTracker>>` on `QueryEngine`, tracks usage from both OpenAI and Claude responses
- [x] **1.6** Add model pricing — per-model cost calculation (Claude Sonnet $3/$15, Opus $15/$75, GPT-4o $2.50/$10, etc.)
- [x] **1.7** Implement bare mode — `--bare` flag: simple stdin/stdout loop, no TUI
- [x] **1.8** Cost summary on exit — printed after one-shot, bare, and TUI modes
- [x] **1.9** Fix `ClaudeMessagesResponse` — now parses `usage` field for token tracking
- [x] **1.10** Update default Claude model — `claude-sonnet-4-20250514`
- [x] **1.11** New CLI flags — `--max-tokens`, `--max-budget`

### Phase 2: Streaming Responses ✅ DONE

- [x] **2.1** Claude SSE streaming — parse `data:` events from `stream: true` response
- [x] **2.2** New `StreamEvent` enum — TextDelta, ToolUseStart, ToolUseInputDelta, MessageStop, Error
- [x] **2.3** OpenAI streaming — (deferred: uses non-streaming for now, streaming via async-openai planned)
- [x] **2.4** TUI streaming display — real-time token output with `UiEvent::StreamDelta`, blinking cursor
- [x] **2.5** New dep: `futures-util = "0.3"` added to Cargo.toml

### Phase 3: Permission System ✅ DONE

- [x] **3.1** `PermissionMode` enum — Default, AcceptEdits, BypassPermissions, Plan, DontAsk (with `ValueEnum` for clap)
- [x] **3.2** Permission checker — `check_permission()` decision chain: Plan→deny, read-only→allow, rules, dangerous path (bypass-immune), mode-specific behavior
- [x] **3.3** Path safety — `is_dangerous_path()` blocks .env/.git/.ssh/etc., `is_within_directory()` prevents traversal
- [x] **3.4** Interactive TUI prompt — Y/n/a via `UiEvent::PermissionRequest` + oneshot channel, red input bar indicator
- [x] **3.5** CLI `--permission-mode` flag — wired into `EngineConfig.permission_mode`
- [x] **3.6** Wired into engine — `check_tool_permission()` called before every `tool.call()` in all 3 paths (OpenAI, Claude streaming, Claude non-streaming)
- [x] **3.7** Session rules — "Always Allow" adds `PermissionRule` to `Arc<Mutex<Vec<PermissionRule>>>` for session persistence
- [x] **3.8** Unit tests — `test_dangerous_paths` for path safety

### Phase 4: Context Management 🔲

- [ ] **4.1** Token estimation — ~4 chars per token, model context window map
- [ ] **4.2** Microcompact — clear old tool results preserving recent N turns
- [ ] **4.3** Auto-compact — LLM summarization when >80% context used
- [ ] **4.4** Wire into query loop — check before each LLM call

### Phase 5: Additional Tools 🔲

- [ ] **5.1** TodoWriteTool — task checklist with shared state
- [ ] **5.2** SleepTool — async wait
- [ ] **5.3** WebFetchTool — URL fetch + HTML stripping (dep: `scraper`)
- [ ] **5.4** AskUserQuestionTool — interactive prompts via TUI
- [ ] **5.5** NotebookEditTool — Jupyter .ipynb editing

### Phase 6: Session, History & Context 🔲

- [ ] **6.1** Context system — CLAUDE.md loading, git status, system info (dep: `chrono`)
- [ ] **6.2** Session persistence — JSONL transcript (dep: `uuid`)
- [ ] **6.3** Session resume — `--resume [session_id]`
- [ ] **6.4** Command history — up-arrow recall from `~/.rust-agent/history.jsonl`
- [ ] **6.5** Slash commands — /help, /clear, /compact, /cost, /exit, /model

### Phase 7: Advanced (Future) 🔲

- [ ] MCP client (JSON-RPC over stdio)
- [ ] AgentTool (sub-agents)
- [ ] Tool concurrency (parallel read-only)
- [ ] EnterPlanMode/ExitPlanMode
- [ ] Plugin/Skill system

---

## File Structure

```
src/
  main.rs                    # CLI + 3 modes (one-shot, bare, TUI)
  engine/
    mod.rs                   # Re-exports
    config.rs                # EngineConfig struct
    cost_tracker.rs          # CostTracker + ModelUsage
    query.rs                 # QueryEngine + agentic loop (OpenAI + Claude)
    streaming.rs             # SSE parser for Claude streaming ✅
    tokens.rs                # [TODO] Token estimation
    compaction.rs            # [TODO] Context compaction
  tools/
    mod.rs                   # Tool trait + ToolContext + ToolResult
    bash/executor.rs         # BashTool
    fs/read_file.rs          # ReadFileTool
    fs/write_file.rs         # WriteFileTool
    edit/edit_file.rs        # FileEditTool
    glob_tool/search.rs      # GlobTool
    grep_tool/search.rs      # GrepTool
    todo/                    # [TODO] TodoWriteTool
    sleep/                   # [TODO] SleepTool
    web_fetch/               # [TODO] WebFetchTool
    ask_user/                # [TODO] AskUserQuestionTool
    notebook/                # [TODO] NotebookEditTool
  ui/
    mod.rs                   # Terminal setup/teardown
    app.rs                   # Ratatui TUI (App struct + event loop)
  models/mod.rs              # TaskType, TaskStatus, Role, Message
  mem/mod.rs                 # Memory system prompt builder
  output_styles.rs           # Output style loading
  keybindings/               # Full keybinding system (17 contexts)
  permissions/               # Permission system (types, checker, path_safety) ✅
  context/                   # [TODO] CLAUDE.md + git + sysinfo
  session/                   # [TODO] Session persistence
  history/                   # [TODO] Command history
  commands/                  # [TODO] Slash commands
```

---

## Dependencies

| Crate             | Version   | Purpose                  | Status     |
| ----------------- | --------- | ------------------------ | ---------- |
| tokio             | 1.37      | Async runtime            | ✅         |
| reqwest           | 0.12      | HTTP client (Claude API) | ✅         |
| async-openai      | 0.23      | OpenAI-compatible API    | ✅         |
| ratatui/crossterm | 0.26/0.27 | Terminal UI              | ✅         |
| clap              | 4.5       | CLI parsing              | ✅         |
| serde/serde_json  | 1.0       | Serialization            | ✅         |
| anyhow            | 1.0       | Error handling           | ✅         |
| tracing           | 0.1       | Logging                  | ✅         |
| async-trait       | 0.1       | Async traits             | ✅         |
| dirs              | 6.0       | Home directory           | ✅         |
| glob              | 0.3       | File globbing            | ✅         |
| regex             | 1.12      | Regex (GrepTool)         | ✅         |
| futures-util      | 0.3       | Stream utilities         | ✅         |
| scraper           | 0.20      | HTML parsing             | 🔲 Phase 5 |
| chrono            | 0.4       | Date/time                | 🔲 Phase 6 |
| uuid              | 1.0       | Session IDs              | 🔲 Phase 6 |

---

## Key Design Decisions

1. **Multi-provider via single engine** — Claude uses raw reqwest (native API), everything else uses `async-openai` (OpenAI-compatible)
2. **`Arc<Mutex<CostTracker>>`** — shared between engine and main for cost display on exit
3. **`EngineConfig`** — single config object flows CLI flags into engine behavior
4. **Bare mode** — simple stdin/stdout loop for scripting/piping, separate from TUI
5. **Tool dispatch by name** — `find_tool()` checks `name()` and `aliases()`
6. **Pricing table** — model name pattern matching for cost estimation

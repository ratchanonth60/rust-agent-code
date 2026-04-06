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
                │          (Tool trait)       (23 built-in)       (back to LLM)
                │
                ├──→ TUI (ratatui) ←── mpsc channels ──→ Engine (tokio task)
                │     ├── Dialog overlays (model/theme/settings)
                │     ├── Autocomplete (commands + @file)
                │     └── Markdown + syntax highlighting + diff viewer
                ├──→ Bare mode (stdin/stdout, no TUI)
                ├──→ Context System (CLAUDE.md, git status, system prompt)
                ├──→ Auth System (Google OAuth2 + PKCE, credential store, env var fallback)
                ├──→ Permission System (5 modes, path safety, session rules)
                ├──→ Cost Tracker (token usage, $/model, budget enforcement)
                ├──→ Session Persistence (JSONL append-only, project-scoped, compact boundaries)
                ├──→ Compaction Service (microcompact + LLM auto-compact, circuit breaker)
                ├──→ MCP Client (JSON-RPC 2.0 over stdio/SSE/HTTP)
                ├──→ Plugin System (~/.rust-agent/plugins/)
                └──→ Skill System (~/.rust-agent/skills/)
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
- [x] **2.3** OpenAI streaming — async-openai `create_stream` with `stream_options: { include_usage: true }`
- [x] **2.4** Gemini streaming — raw HTTP SSE via `query_gemini_compat` with `thought_signature` preservation
- [x] **2.5** TUI streaming display — real-time token output with `UiEvent::StreamDelta`, blinking cursor
- [x] **2.6** New dep: `futures-util = "0.3"` added to Cargo.toml

### Phase 3: Permission System ✅ DONE

- [x] **3.1** `PermissionMode` enum — Default, AcceptEdits, BypassPermissions, Plan, DontAsk (with `ValueEnum` for clap)
- [x] **3.2** Permission checker — `check_permission()` decision chain: Plan→deny, read-only→allow, rules, dangerous path (bypass-immune), mode-specific behavior
- [x] **3.3** Path safety — `is_dangerous_path()` blocks .env/.git/.ssh/etc., `is_within_directory()` prevents traversal
- [x] **3.4** Interactive TUI prompt — Y/n/a via `UiEvent::PermissionRequest` + oneshot channel, red input bar indicator
- [x] **3.5** CLI `--permission-mode` flag — wired into `EngineConfig.permission_mode`
- [x] **3.6** Wired into engine — `check_tool_permission()` called before every `tool.call()` in all provider paths
- [x] **3.7** Session rules — "Always Allow" adds `PermissionRule` to `Arc<Mutex<Vec<PermissionRule>>>` for session persistence
- [x] **3.8** Unit tests — `test_dangerous_paths` for path safety

### Phase 4: Context Management ✅ DONE

- [x] **4.1** Token estimation — `engine/tokens.rs`: ~4 chars/token, model context window map (Claude 200K, GPT-4o 128K, Gemini 1M)
- [x] **4.2** Microcompact — `engine/compaction.rs`: clear old tool results >500 chars, preserve recent 6 turns
- [x] **4.3** Auto-compact with LLM summarization — `build_compact_prompt()` + circuit breaker pattern
- [x] **4.4** Wire into query loop — all provider paths check `should_compact()` before each LLM call

### Phase 5: Tools ✅ DONE (23 tools)

- [x] **5.1** Core tools — Bash, ReadFile, WriteFile, FileEditTool, GlobTool, GrepTool
- [x] **5.2** TodoWriteTool — shared `Arc<Mutex<Vec<TodoItem>>>`, full replacement semantics
- [x] **5.3** SleepTool — `tokio::time::sleep`, 1-300 seconds, read-only + concurrency-safe
- [x] **5.4** WebFetchTool — reqwest fetch + built-in HTML tag stripping, truncate at 50K chars
- [x] **5.5** WebSearchTool — web search queries
- [x] **5.6** AskUserQuestionTool — sends QuestionRequest via mpsc channel, awaits oneshot response
- [x] **5.7** NotebookEditTool — Jupyter .ipynb cell editing
- [x] **5.8** Task tools — BackgroundTaskTool, TaskOutputTool, TaskStopTool
- [x] **5.9** Plan tools — EnterPlanModeTool, ExitPlanModeTool
- [x] **5.10** Worktree tools — EnterWorktreeTool, ExitWorktreeTool
- [x] **5.11** ConfigTool — read/write agent configuration at runtime
- [x] **5.12** SkillTool — invoke user-defined skill prompts
- [x] **5.13** Team tools — CreateTeamTool, DeleteTeamTool, SendTeamMessageTool
- [x] **5.14** AgentTool — spawns fresh QueryEngine for sub-agent tasks

### Phase 6: Systems ✅ DONE

- [x] **6.1** Context system — `context/` module: CLAUDE.md loading (global + project), git context, system info
- [x] **6.2** Session persistence — JSONL append-only save/load/list at `~/.rust-agent/sessions/<project>/`
- [x] **6.3** Session resume — `/resume` command with `ResumeSession` result variant
- [x] **6.4** Command history — up/down arrow recall in TUI
- [x] **6.5** Slash commands — 33 commands in registry
- [x] **6.6** Keybindings — 18 contexts, 70+ actions, chord sequences, user overrides
- [x] **6.7** Memory system — file-based at `~/.rust-agent/memory/`
- [x] **6.8** Output styles — custom markdown styles from `~/.rust-agent/output-styles/`
- [x] **6.9** Plugin system — `~/.rust-agent/plugins/` with JSON manifests
- [x] **6.10** Skill system — markdown prompt templates from `~/.rust-agent/skills/`
- [x] **6.11** MCP client — JSON-RPC 2.0, stdio/SSE/HTTP, tool/resource discovery
- [x] **6.12** Global config — persistent `~/.rust-agent/config.json`

### Phase 7: TUI Polish ✅ DONE

- [x] **7.1** Hero title + animated tagline
- [x] **7.2** Autocomplete — slash commands + `@file` references with debounced file scanning
- [x] **7.3** Markdown rendering — pulldown-cmark with styled elements
- [x] **7.4** Syntax highlighting — syntect with `base16-ocean.dark`
- [x] **7.5** Diff viewer — color-coded unified diffs
- [x] **7.6** Dialog overlays — ModelPickerDialog, ThemePickerDialog
- [x] **7.7** Settings dialog — full configuration editor with grouped settings (General/Display/Provider)
- [x] **7.8** Animated status rail with braille spinners

### Phase 8: Advanced (Future) 🔲

- [ ] **8.1** Parallel tool execution for OpenAI/Gemini providers
- [x] **8.2** OAuth / API key management (`/login`, `/logout`, `/auth-status`) — Google OAuth2 + PKCE for Gemini
- [x] **8.2b** Session JSONL upgrade — append-only, project-scoped, crash-safe with compact boundaries
- [ ] **8.3** GitHub integration (`/pr`, `/issue`, PR review automation)
- [ ] **8.4** IDE bridge (VS Code extension integration)
- [ ] **8.5** Coordinator mode (multi-agent orchestration)
- [ ] **8.6** Advanced task types (DreamTask, RemoteAgentTask, InProcessTeammateTask)
- [ ] **8.7** Sandbox/container execution
- [ ] **8.8** Voice input/output
- [ ] **8.9** System notifications
- [ ] **8.10** Analytics/telemetry
- [ ] **8.11** Tips system and auto-update
- [ ] **8.12** ~38 remaining slash commands from TS original

---

## Port Status vs TypeScript Original

| Category | TS Original | Rust Port | Coverage |
|----------|------------|-----------|----------|
| Tools | ~35 | 23 | ~66% |
| Commands | ~70 | 36 | ~51% |
| Providers | 3 | 3 + compatible | 100% |
| Streaming | All | All | 100% |
| Permissions | 5 modes | 5 modes | 100% |
| Context | Full | Full | 100% |
| MCP | Full | Full | 100% |
| Plugins | Full | Full | 100% |
| Skills | Full | Full | 100% |
| TUI | React/Ink | ratatui | ~90% |
| OAuth | Yes | Gemini (Google OAuth2 + PKCE) | ~33% |
| IDE Bridge | Yes | No | 0% |
| Coordinator | Yes | No | 0% |
| Voice | Yes | No | 0% |

**Overall port progress: ~80%**

---

## File Structure

```
src/
  main.rs                           # CLI + 3 modes (one-shot, bare, TUI)
  auth/
    mod.rs, credentials.rs          # OAuth credential store + token management
    oauth.rs, client_config.rs      # PKCE flow, localhost callback, token exchange
  engine/
    mod.rs, config.rs, query.rs     # Core engine + agentic loop
    streaming.rs, tokens.rs         # SSE parser + token estimation
    cost_tracker.rs, compaction.rs  # Cost + context management
    session.rs, state.rs            # JSONL persistence + shared state
    agent_tool.rs                   # Sub-agent spawning
  tools/
    mod.rs, registry.rs             # Tool trait + 23-tool registry
    bash/, fs/, edit/, glob_tool/   # Core file/shell tools
    grep_tool/, notebook/           # Search + notebook
    todo/, sleep/, web_fetch/       # Utility tools
    web_search/, ask_user/          # Web + interactive
    tasks/, plan_mode/, worktree/   # Task + workflow tools
    config_tool/, skill_tool/       # Config + skills
    teams/                          # Team collaboration
  ui/
    mod.rs, app.rs                  # TUI + event loop
    app/autocomplete.rs             # Autocomplete engine
    app/render.rs                   # Rendering pipeline
    dialogs/                        # Model/theme/settings dialogs
    diff_viewer.rs, highlight.rs    # Diff + syntax highlighting
    markdown.rs                     # Markdown → ratatui
  commands/                         # 36 slash commands (incl. login/logout/auth-status)
  keybindings/                      # 18 contexts, 70+ actions
  permissions/                      # 5-mode permission system
  context/                          # CLAUDE.md + git + sysinfo
  config/, models/, mem/            # Config + types + memory
  output_styles.rs                  # Output style loading
  plugins/, skills/                 # Plugin + skill systems
  mcp/                              # MCP client (JSON-RPC 2.0)
```

---

## Dependencies

| Crate             | Version   | Purpose                      | Status     |
| ----------------- | --------- | ---------------------------- | ---------- |
| tokio             | 1.37      | Async runtime                | ✅         |
| reqwest           | 0.12      | HTTP client (Claude API)     | ✅         |
| async-openai      | 0.23      | OpenAI-compatible API        | ✅         |
| ratatui/crossterm | 0.26/0.27 | Terminal UI                  | ✅         |
| clap              | 4.5       | CLI parsing                  | ✅         |
| serde/serde_json  | 1.0       | Serialization                | ✅         |
| anyhow            | 1.0       | Error handling               | ✅         |
| tracing           | 0.1       | Logging                      | ✅         |
| async-trait       | 0.1       | Async traits                 | ✅         |
| dirs              | 6.0       | Home directory               | ✅         |
| glob              | 0.3       | File globbing                | ✅         |
| regex             | 1.12      | Regex (GrepTool)             | ✅         |
| futures-util      | 0.3       | Stream utilities             | ✅         |
| pulldown-cmark    | 0.12      | Markdown parsing             | ✅         |
| syntect           | 5         | Syntax highlighting          | ✅         |
| uuid              | 1.0       | Session IDs                  | ✅         |
| chrono            | 0.4       | Date/time                    | ✅         |
| ferris-says       | 0.3       | Welcome banner               | ✅         |
| sha2              | 0.10      | PKCE code challenge (OAuth)  | ✅         |
| base64            | 0.22      | PKCE encoding                | ✅         |
| rand              | 0.8       | PKCE + state nonce generation| ✅         |

---

## Key Design Decisions

1. **Multi-provider via single engine** — Claude uses raw reqwest (native API), everything else uses `async-openai` (OpenAI-compatible)
2. **`Arc<Mutex<CostTracker>>`** — shared between engine and main for cost display on exit
3. **`EngineConfig`** — single config object flows CLI flags into engine behavior
4. **Bare mode** — simple stdin/stdout loop for scripting/piping, separate from TUI
5. **Tool dispatch by name** — `find_tool()` checks `name()` and `aliases()`
6. **Pricing table** — model name pattern matching for cost estimation
7. **Dialog system** — trait-based overlays (Dialog trait → handle_key/render) with ActiveDialog state machine
8. **Gemini raw HTTP** — uses `Vec<Value>` messages instead of typed structs to preserve `thought_signature` fields

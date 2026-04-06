# Architecture

> Module map, data flow, and project structure for Rust Agent.

---

## High-Level Data Flow

```
                          ┌──────────────────────┐
   User Input ──────────► │     main.rs (CLI)     │
                          │  clap args + tokio    │
                          └─────────┬────────────┘
                                    │
              ┌─────────────────────┼─────────────────────┐
              │                     │                     │
              ▼                     ▼                     ▼
        ┌──────────┐        ┌────────────┐        ┌────────────┐
        │ One-shot │        │ Bare Mode  │        │ TUI Mode   │
        │          │        │ stdin/out  │        │ (ratatui)  │
        └────┬─────┘        └─────┬──────┘        └─────┬──────┘
             │                    │                     │
             └────────────────────┼─────────────────────┘
                                  │
                                  ▼
                    ┌─────────────────────────┐
                    │      QueryEngine        │
                    │  (agentic tool-use loop)│
                    └────────┬────────────────┘
                             │
                ┌────────────┼────────────────┐
                ▼            ▼                ▼
          ┌──────────┐ ┌──────────┐    ┌──────────┐
          │  Claude  │ │  OpenAI  │    │  Gemini  │
          │ (native) │ │(async-oa)│    │(raw HTTP)│
          └──────────┘ └──────────┘    └──────────┘
                             │
                    ┌────────┴────────┐
                    │  Tool Dispatch  │
                    │  (23 tools)     │
                    └────────┬────────┘
                             │
                    Results fed back to LLM
                    until final text answer

                    ┌─────────────────────────┐
                    │     Auth / Credential    │
                    │   OAuth2 → env var → err │
                    └─────────────────────────┘
```

## TUI Architecture

```
 ┌─ App ────────────────────────────────────────────────┐
 │                                                      │
 │  tx_to_engine ──► tokio::spawn ──► QueryEngine       │
 │                                        │             │
 │  rx_from_engine ◄── UiEvent ◄──────────┘             │
 │                                                      │
 │  rx_questions ◄──── AskUserQuestionTool              │
 │                                                      │
 │  ┌─ Render ────────────────────────────────┐         │
 │  │  conversation   (scrollable messages)    │         │
 │  │  status line    (activity + pill + cost) │         │
 │  │  autocomplete   (commands + @files)      │         │
 │  │  prompt         (input + cursor)         │         │
 │  │  dialog overlay (settings/model/theme)   │         │
 │  └─────────────────────────────────────────┘         │
 └──────────────────────────────────────────────────────┘
```

---

## Project Structure

```
src/
├── main.rs                         Entry point, CLI args, 3 execution modes
│
├── auth/                           Authentication & credential management
│   ├── mod.rs                      resolve_gemini_token() facade
│   ├── credentials.rs              CredentialStore + TokenCredential (load/save)
│   ├── oauth.rs                    PKCE generation, callback server, token exchange
│   ├── client_config.rs            OAuthClientConfig (Google endpoints)
│   └── resolver.rs                 Auth chain: OAuth → env var → error
│
├── engine/                         LLM query engine
│   ├── mod.rs                      Re-exports and module layout docs
│   ├── config.rs                   EngineConfig (auto_mode, max_tokens, etc.)
│   ├── query.rs                    QueryEngine struct, constructor, dispatcher
│   ├── agent_tool.rs               Sub-agent tool (spawns child QueryEngine)
│   ├── pricing.rs                  Per-model pricing table + cost calculation
│   ├── cost_tracker.rs             Token/cost accumulator with budget enforcement
│   ├── tokens.rs                   Token estimation + context window map
│   ├── compaction.rs               Microcompact + LLM auto-compact + circuit breaker
│   ├── streaming.rs                Claude SSE stream parser
│   ├── session.rs                  Session persistence (JSONL append-only, project-scoped)
│   ├── state.rs                    SharedEngineState (Arc<RwLock<...>>)
│   └── providers/                  Provider-specific agentic loops
│       ├── mod.rs                  Module declarations
│       ├── claude.rs               Anthropic Messages API (native SSE)
│       ├── openai.rs               OpenAI / compatible (async-openai)
│       └── gemini.rs               Gemini (raw HTTP SSE, thought_signature)
│
├── tasks/                          Unified task registry
│   ├── mod.rs                      TaskRegistry, SharedTaskRegistry, ID generation
│   ├── types.rs                    LocalBashTaskState, LocalAgentTaskState, TaskState
│   ├── shell.rs                    spawn, collect_output, kill, kill_for_agent
│   ├── agent.rs                    register, complete, fail, kill
│   ├── stop.rs                     Generic stop dispatch by TaskType
│   └── pill_label.rs               Status bar pill label (" N tasks ")
│
├── tools/                          Tool system
│   ├── mod.rs                      Tool trait, ToolContext, ToolResult
│   ├── registry.rs                 default_tools() builder
│   ├── bash/                       BashTool (shell execution with timeout)
│   ├── fs/                         ReadFileTool, WriteFileTool
│   ├── edit/                       FileEditTool (exact string replacement)
│   ├── glob_tool/                  GlobTool (pattern matching)
│   ├── grep_tool/                  GrepTool (ripgrep wrapper)
│   ├── notebook/                   NotebookEditTool (.ipynb)
│   ├── todo/                       TodoWriteTool (shared state)
│   ├── sleep/                      SleepTool (async wait)
│   ├── web_fetch/                  WebFetchTool (HTML stripping)
│   ├── web_search/                 WebSearchTool
│   ├── ask_user/                   AskUserQuestionTool (mpsc + oneshot)
│   ├── tasks/                      BackgroundTaskTool, TaskOutputTool, TaskStopTool
│   ├── plan_mode/                  EnterPlanModeTool, ExitPlanModeTool
│   ├── worktree/                   EnterWorktreeTool, ExitWorktreeTool
│   ├── config_tool/                ConfigTool (runtime config read/write)
│   ├── skill_tool/                 SkillTool (invoke user-defined skills)
│   └── teams/                      CreateTeamTool, DeleteTeamTool, SendTeamMessageTool
│
├── ui/                             Terminal UI
│   ├── mod.rs                      setup_terminal, restore_terminal
│   ├── app.rs                      App struct, UiEvent, MessageEntry, event loop
│   ├── app/
│   │   ├── render.rs               Conversation, status line, prompt, dialog overlay
│   │   ├── autocomplete.rs         Slash command + @file autocomplete
│   │   ├── commands_handler.rs     Slash command parsing and dispatch
│   │   ├── dialog_handler.rs       Dialog open/close/result lifecycle
│   │   └── history.rs              Input history up/down navigation
│   ├── dialogs/
│   │   ├── mod.rs                  ActiveDialog enum, Dialog trait, DialogAction
│   │   ├── model_picker.rs         Model selection (grouped by provider)
│   │   ├── theme_picker.rs         Theme selection
│   │   └── settings_dialog.rs      Full settings editor (3 groups)
│   ├── diff_viewer.rs              Color-coded unified diff renderer
│   ├── highlight.rs                Syntect-based syntax highlighting
│   └── markdown.rs                 pulldown-cmark → ratatui spans
│
├── commands/                       Slash command system
│   ├── mod.rs                      build_default_registry() (36 commands)
│   ├── types.rs                    Command trait, CommandResult, CommandContext
│   ├── registry.rs                 CommandRegistry (name + alias lookup)
│   ├── help.rs                     /help
│   ├── clear.rs                    /clear
│   ├── cost.rs                     /cost
│   ├── exit.rs                     /exit
│   ├── config_cmd.rs               /config
│   ├── settings_cmd.rs             /settings
│   ├── model.rs                    /model
│   ├── theme.rs                    /theme
│   ├── output_style.rs             /output-style
│   ├── vim.rs                      /vim
│   ├── effort.rs                   /effort
│   ├── fast.rs                     /fast
│   ├── plan.rs                     /plan
│   ├── permissions_cmd.rs          /permissions
│   ├── stats.rs                    /stats
│   ├── status.rs                   /status
│   ├── context.rs                  /context
│   ├── keybindings_cmd.rs          /keybindings
│   ├── doctor.rs                   /doctor
│   ├── memory.rs                   /memory
│   ├── diff.rs                     /diff
│   ├── branch.rs                   /branch
│   ├── commit.rs                   /commit
│   ├── review.rs                   /review
│   ├── compact.rs                  /compact
│   ├── export.rs                   /export
│   ├── resume.rs                   /resume
│   ├── login.rs                    /login (OAuth browser flow)
│   ├── logout.rs                   /logout (revoke + remove tokens)
│   ├── auth_status.rs              /auth-status (show credential state)
│   ├── mcp.rs                      /mcp
│   └── skill.rs                    /skill
│
├── keybindings/                    Keybinding system
│   ├── mod.rs                      Re-exports
│   ├── types.rs                    18 contexts, 70+ actions, chord types
│   ├── default_bindings.rs         Built-in default key mappings
│   ├── loader.rs                   Loads ~/.rust-agent/keybindings.json
│   ├── parser.rs                   "ctrl+k" → ParsedKeystroke
│   ├── matcher.rs                  KeyEvent → binding match
│   ├── resolver.rs                 resolve_key() with chord state
│   └── reserved.rs                 Reserved shortcuts (Ctrl+C/D/M)
│
├── permissions/                    Permission system
│   ├── mod.rs                      Re-exports
│   ├── types.rs                    PermissionMode, PermissionDecision, PermissionRule
│   ├── checker.rs                  check_permission() decision chain
│   └── path_safety.rs             Dangerous path detection
│
├── context/                        System prompt context
│   ├── mod.rs                      build_context_prompt()
│   ├── claudemd.rs                 CLAUDE.md loader (global + project scopes)
│   ├── git.rs                      Git branch, status, recent log
│   └── system_info.rs              OS, arch, cwd, shell
│
├── config/
│   └── mod.rs                      GlobalConfig (persistent at ~/.rust-agent/config.json)
│
├── models/
│   └── mod.rs                      TaskType, TaskStatus, Role, Message, Attachment
│
├── mem/
│   └── mod.rs                      Memory system (file-based, MEMORY.md index)
│
├── output_styles.rs                Output style loading from *.md files
│
├── plugins/                        Plugin system
│   ├── mod.rs                      Re-exports
│   └── loader.rs                   Discovery + JSON manifest loading
│
├── skills/                         Skill system
│   ├── mod.rs                      Re-exports
│   └── loader.rs                   Markdown prompt template loader
│
└── mcp/                            MCP client
    ├── mod.rs                      Re-exports
    ├── types.rs                    JSON-RPC 2.0, McpToolDef, McpResource
    ├── transport.rs                Stdio/SSE/HTTP transport
    ├── client.rs                   MCP client lifecycle
    ├── manager.rs                  McpManager (multi-server)
    └── tools.rs                    McpProxyTool wrapper
```

---

## Key Design Decisions

### Split `impl` Pattern

Large types like `QueryEngine` use Rust's ability to write `impl` blocks in
separate files. The core struct and constructor live in `query.rs`, while each
provider's agentic loop lives in its own file under `providers/`:

```rust
// engine/query.rs        — struct + new() + shared helpers
// engine/providers/claude.rs  — impl QueryEngine { fn query_claude() }
// engine/providers/openai.rs  — impl QueryEngine { fn query_openai_compatible() }
// engine/providers/gemini.rs  — impl QueryEngine { fn query_gemini_compat() }
```

Similarly, `App` is split into `app.rs` (state + event loop) and sub-modules
under `app/` (render, autocomplete, commands, dialogs, history).

### Unified Task Registry

Background tasks (shell processes and sub-agents) share a single
`TaskRegistry` behind `Arc<Mutex<...>>`. Task IDs use prefix letters
(`b001` for bash, `a001` for agent) for quick visual identification.
The registry is passed to tools, the engine, and the TUI — so the status
line pill updates in real-time.

### Provider Isolation

Each LLM provider has its own message format and streaming approach:

- **Claude**: `ClaudeMessage` / `ClaudeContentBlock` structs, native SSE via `reqwest`
- **OpenAI**: `ChatCompletionRequestMessage` via `async-openai` typed structs
- **Gemini**: `Vec<Value>` (untyped) to preserve opaque fields like `thought_signature`

### Channel-Based TUI

The TUI communicates with the engine via `tokio::sync::mpsc` channels:

- `tx_to_engine` — user input to engine
- `rx_from_engine` — `UiEvent` variants (stream deltas, tool events, responses)
- `rx_questions` — `AskUserQuestionTool` prompts

This keeps the UI thread responsive while the engine runs async tool-use loops.

### Auth Fallback Chain

Gemini authentication uses a priority chain with zero breaking changes:

```
CredentialStore (OAuth2 token, auto-refresh if expired)
  ↓ not found or refresh failed
GEMINI_API_KEY env var
  ↓ not found
Error with helpful message
```

OAuth uses **Authorization Code + PKCE (S256)** with a localhost callback server.
Credentials are stored in `~/.rust-agent/credentials.json` (chmod 600). The flow
opens the user's browser, Google redirects to `http://127.0.0.1:<random-port>/`,
and the agent exchanges the code for tokens.

### Session JSONL Format

Sessions use append-only JSONL (one JSON object per line) for crash safety:

```
~/.rust-agent/sessions/<project-hash-16char>/<session-id>.jsonl
```

Each line is tagged by `type`: `header`, `message`, `compact_boundary`, or `cost`.
On load, `compact_boundary` entries clear all preceding messages, which enables
context compaction without rewriting the file. Legacy `.json` files are still
loadable for backward compatibility.

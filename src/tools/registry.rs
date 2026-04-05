//! Tool registry — centralises tool construction for the engine.
//!
//! Instead of inline `vec![…]` in [`QueryEngine::new`], the registry provides
//! [`default_tools`] which returns both the tool vector and the shared task
//! registry used by background task tools.

use crate::tasks::SharedTaskRegistry;
use crate::tools::ask_user::{AskUserQuestionTool, QuestionSender};
use crate::tools::bash::BashTool;
use crate::tools::config_tool::ConfigTool;
use crate::tools::edit::FileEditTool;
use crate::tools::fs::{ReadFileTool, WriteFileTool};
use crate::tools::glob_tool::GlobTool;
use crate::tools::grep_tool::GrepTool;
use crate::tools::notebook::NotebookEditTool;
use crate::tools::plan_mode::{EnterPlanModeTool, ExitPlanModeTool};
use crate::tools::skill_tool::SkillTool;
use crate::tools::sleep::SleepTool;
use crate::tools::tasks::{BackgroundTaskTool, TaskOutputTool, TaskStopTool};
use crate::tools::teams::{CreateTeamTool, DeleteTeamTool, SendTeamMessageTool};
use crate::tools::todo::{SharedTodoList, TodoWriteTool};
use crate::tools::web_fetch::WebFetchTool;
use crate::tools::web_search::WebSearchTool;
use crate::tools::worktree::{self, EnterWorktreeTool, ExitWorktreeTool};
use crate::tools::Tool;

/// Returns the standard set of built-in tools and the shared task registry.
///
/// The returned [`SharedTaskRegistry`] should be stored in [`QueryEngine`]
/// and passed to the TUI for status pill rendering and agent task tracking.
pub fn default_tools(
    todo_list: SharedTodoList,
    question_tx: Option<QuestionSender>,
) -> (Vec<Box<dyn Tool + Send + Sync>>, SharedTaskRegistry) {
    let task_registry = crate::tasks::new_shared_registry();
    let worktree_state = worktree::new_shared_worktree_state();

    let tools: Vec<Box<dyn Tool + Send + Sync>> = vec![
        // File operations
        Box::new(ReadFileTool),
        Box::new(WriteFileTool),
        Box::new(FileEditTool),
        Box::new(NotebookEditTool),
        // Search
        Box::new(GlobTool),
        Box::new(GrepTool),
        // Execution
        Box::new(BashTool),
        Box::new(SleepTool),
        // Task management
        Box::new(BackgroundTaskTool {
            registry: task_registry.clone(),
        }),
        Box::new(TaskOutputTool {
            registry: task_registry.clone(),
        }),
        Box::new(TaskStopTool {
            registry: task_registry.clone(),
        }),
        // Workflow
        Box::new(TodoWriteTool { todos: todo_list }),
        Box::new(EnterPlanModeTool),
        Box::new(ExitPlanModeTool),
        // Worktree
        Box::new(EnterWorktreeTool {
            state: worktree_state.clone(),
        }),
        Box::new(ExitWorktreeTool {
            state: worktree_state,
        }),
        // Communication & web
        Box::new(WebFetchTool),
        Box::new(WebSearchTool),
        Box::new(AskUserQuestionTool::new(question_tx)),
        // Configuration
        Box::new(ConfigTool),
        // Skills
        Box::new(SkillTool),
        // Team collaboration
        Box::new(CreateTeamTool),
        Box::new(DeleteTeamTool),
        Box::new(SendTeamMessageTool),
    ];

    (tools, task_registry)
}

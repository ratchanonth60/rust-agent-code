//! Tool registry — centralises tool construction for the engine.
//!
//! Instead of inline `vec![…]` in [`QueryEngine::new`], the registry provides
//! [`default_tools`] (always-on) and [`optional_tools`] (feature-gated).

use crate::tools::ask_user::{AskUserQuestionTool, QuestionSender};
use crate::tools::bash::BashTool;
use crate::tools::edit::FileEditTool;
use crate::tools::fs::{ReadFileTool, WriteFileTool};
use crate::tools::glob_tool::GlobTool;
use crate::tools::grep_tool::GrepTool;
use crate::tools::notebook::NotebookEditTool;
use crate::tools::plan_mode::{EnterPlanModeTool, ExitPlanModeTool};
use crate::tools::sleep::SleepTool;
use crate::tools::tasks::{self, BackgroundTaskTool, TaskOutputTool, TaskStopTool};
use crate::tools::todo::{SharedTodoList, TodoWriteTool};
use crate::tools::web_fetch::WebFetchTool;
use crate::tools::Tool;

/// Returns the standard set of built-in tools.
///
/// These tools are always available regardless of configuration.
pub fn default_tools(
    todo_list: SharedTodoList,
    question_tx: Option<QuestionSender>,
) -> Vec<Box<dyn Tool + Send + Sync>> {
    let task_manager = tasks::new_shared_task_manager();

    vec![
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
            manager: task_manager.clone(),
        }),
        Box::new(TaskOutputTool {
            manager: task_manager.clone(),
        }),
        Box::new(TaskStopTool {
            manager: task_manager,
        }),
        // Workflow
        Box::new(TodoWriteTool { todos: todo_list }),
        Box::new(EnterPlanModeTool),
        Box::new(ExitPlanModeTool),
        // Communication
        Box::new(WebFetchTool),
        Box::new(AskUserQuestionTool::new(question_tx)),
    ]
}

//! Interactive question tool — prompts the user via the TUI.

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

use crate::tools::{Tool, ToolContext, ToolResult};

/// Channel sender held by the tool to send questions to the TUI.
pub type QuestionSender = mpsc::Sender<QuestionRequest>;

/// A question request sent from the tool to the TUI.
pub struct QuestionRequest {
    /// The question text to display.
    pub question: String,
    /// Optional multiple-choice options.
    pub options: Vec<String>,
    /// Oneshot channel for the user's answer.
    pub response_tx: oneshot::Sender<String>,
}

/// Sends a question to the user and awaits their typed response.
///
/// Requires a TUI channel (set via [`AskUserQuestionTool::new`]).
/// Without one, the tool returns an error.
pub struct AskUserQuestionTool {
    pub question_tx: Option<QuestionSender>,
}

impl AskUserQuestionTool {
    pub fn new(question_tx: Option<QuestionSender>) -> Self {
        Self { question_tx }
    }
}

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn name(&self) -> &str {
        "AskUserQuestion"
    }

    fn description(&self) -> &str {
        "Ask the user a question and get their response. Use when you need to \
         gather preferences, clarify instructions, or get decisions."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to ask the user"
                },
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of choices for the user"
                }
            },
            "required": ["question"]
        })
    }

    fn is_destructive(&self) -> bool { false }
    fn is_read_only(&self) -> bool { true }
    fn is_concurrency_safe(&self) -> bool { false }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let question = input.get("question")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'question' field"))?
            .to_string();

        let options: Vec<String> = input.get("options")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let Some(ref tx) = self.question_tx else {
            // No TUI channel — return a message asking to provide input another way
            return Ok(ToolResult::err(json!({
                "error": "No interactive UI available to ask the user."
            })));
        };

        let (resp_tx, resp_rx) = oneshot::channel();
        tx.send(QuestionRequest {
            question: question.clone(),
            options: options.clone(),
            response_tx: resp_tx,
        }).await.map_err(|_| anyhow::anyhow!("Failed to send question to UI"))?;

        let answer = resp_rx.await
            .map_err(|_| anyhow::anyhow!("UI channel closed before answering"))?;

        Ok(ToolResult::ok(json!({
            "question": question,
            "answer": answer
        })))
    }
}

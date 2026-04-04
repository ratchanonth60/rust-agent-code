//! NotebookEdit tool — edit Jupyter notebook (.ipynb) cells.
//!
//! Supports replacing, inserting, and deleting cells in `.ipynb` files.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::tools::{Tool, ToolContext, ToolResult};

// ── Types ───────────────────────────────────────────────────────────────

/// A minimal Jupyter notebook representation.
#[derive(Debug, Serialize, Deserialize)]
struct Notebook {
    cells: Vec<NotebookCell>,
    #[serde(flatten)]
    other: serde_json::Map<String, Value>,
}

/// A single notebook cell.
#[derive(Debug, Serialize, Deserialize)]
struct NotebookCell {
    cell_type: String,
    source: Vec<String>,
    #[serde(flatten)]
    other: serde_json::Map<String, Value>,
}

// ── Tool ────────────────────────────────────────────────────────────────

/// Edit a cell in a Jupyter notebook file.
pub struct NotebookEditTool;

#[async_trait]
impl Tool for NotebookEditTool {
    fn name(&self) -> &str {
        "NotebookEdit"
    }

    fn description(&self) -> &str {
        "Edit a cell in a Jupyter notebook (.ipynb). Supports replace, insert, and delete."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "notebook_path": {
                    "type": "string",
                    "description": "Absolute path to the .ipynb file"
                },
                "cell_number": {
                    "type": "integer",
                    "description": "0-indexed cell number to edit"
                },
                "new_source": {
                    "type": "string",
                    "description": "New source content for the cell"
                },
                "cell_type": {
                    "type": "string",
                    "enum": ["code", "markdown"],
                    "description": "Cell type (required for insert)"
                },
                "edit_mode": {
                    "type": "string",
                    "enum": ["replace", "insert", "delete"],
                    "description": "Edit mode: replace (default), insert, or delete"
                }
            },
            "required": ["notebook_path", "new_source"]
        })
    }

    fn is_destructive(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let path = input["notebook_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'notebook_path'"))?;
        let new_source = input["new_source"].as_str().unwrap_or("");
        let cell_number = input["cell_number"].as_u64().unwrap_or(0) as usize;
        let edit_mode = input["edit_mode"].as_str().unwrap_or("replace");
        let cell_type = input["cell_type"].as_str().unwrap_or("code");

        let content = std::fs::read_to_string(path)?;
        let mut notebook: Notebook = serde_json::from_str(&content)?;

        match edit_mode {
            "replace" => {
                if cell_number >= notebook.cells.len() {
                    return Ok(ToolResult::err(json!({
                        "error": format!("Cell {} out of range (notebook has {} cells)", cell_number, notebook.cells.len())
                    })));
                }
                notebook.cells[cell_number].source = split_source(new_source);
                if let Some(ct) = input["cell_type"].as_str() {
                    notebook.cells[cell_number].cell_type = ct.to_string();
                }
            }
            "insert" => {
                let idx = cell_number.min(notebook.cells.len());
                let mut other = serde_json::Map::new();
                other.insert("metadata".into(), json!({}));
                if cell_type == "code" {
                    other.insert("outputs".into(), json!([]));
                    other.insert("execution_count".into(), Value::Null);
                }
                notebook.cells.insert(
                    idx,
                    NotebookCell {
                        cell_type: cell_type.to_string(),
                        source: split_source(new_source),
                        other,
                    },
                );
            }
            "delete" => {
                if cell_number >= notebook.cells.len() {
                    return Ok(ToolResult::err(json!({
                        "error": format!("Cell {} out of range", cell_number)
                    })));
                }
                notebook.cells.remove(cell_number);
            }
            other => {
                return Ok(ToolResult::err(json!({
                    "error": format!("Unknown edit_mode: '{}'", other)
                })));
            }
        }

        let output = serde_json::to_string_pretty(&notebook)?;
        std::fs::write(path, output)?;

        Ok(ToolResult::ok(json!({
            "status": "ok",
            "edit_mode": edit_mode,
            "cell_number": cell_number,
            "total_cells": notebook.cells.len()
        })))
    }
}

/// Split source text into lines with trailing newlines (Jupyter format).
fn split_source(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if i < lines.len() - 1 {
                format!("{}\n", line)
            } else {
                line.to_string()
            }
        })
        .collect()
}

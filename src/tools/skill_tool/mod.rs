//! SkillTool — LLM-invocable tool to execute user-defined skills.
//!
//! Wraps the skill system so the LLM can invoke skills by name,
//! injecting the skill's prompt template into the conversation.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tools::{Tool, ToolContext, ToolResult};

/// Execute a user-defined skill by name.
pub struct SkillTool;

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "Skill"
    }

    fn description(&self) -> &str {
        "Execute a user-defined skill (prompt template) by name. \
         Skills are loaded from ~/.rust-agent/skills/ and <project>/.rust-agent/skills/."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "skill": {
                    "type": "string",
                    "description": "The skill name to execute"
                },
                "args": {
                    "type": "string",
                    "description": "Optional arguments to pass to the skill"
                }
            },
            "required": ["skill"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let skill_name = input["skill"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'skill' parameter"))?;
        let args = input["args"].as_str().unwrap_or("");

        let skills = crate::skills::load_skills(&ctx.cwd);

        if let Some(skill) = skills.iter().find(|s| s.name == skill_name) {
            let mut prompt = skill.prompt.clone();
            if !args.is_empty() {
                prompt.push_str(&format!("\n\nAdditional instructions: {}", args));
            }

            Ok(ToolResult::ok(json!({
                "skill": skill_name,
                "prompt": prompt,
                "description": skill.description,
                "allowed_tools": skill.allowed_tools,
            })))
        } else {
            // List available skills in the error
            let available: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
            Ok(ToolResult::err(json!({
                "error": format!("Skill '{}' not found", skill_name),
                "available_skills": available,
            })))
        }
    }
}

//! `/skill` — list or invoke user-defined skills.

use super::types::{Command, CommandContext, CommandResult, CommandType, PromptCommand};

/// List or invoke a skill.
pub struct SkillCommand;

impl Command for SkillCommand {
    fn name(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        "List or invoke user-defined skills"
    }

    fn argument_hint(&self) -> Option<&str> {
        Some("[name] [args...]")
    }

    fn command_type(&self) -> CommandType {
        CommandType::Prompt
    }

    fn execute(&self, args: &str, ctx: &CommandContext) -> anyhow::Result<CommandResult> {
        let parts: Vec<&str> = args.split_whitespace().collect();
        let skill_name = parts.first().copied().unwrap_or("");

        let skills = crate::skills::load_skills(&ctx.cwd);

        if skill_name.is_empty() {
            // List all skills
            if skills.is_empty() {
                return Ok(CommandResult::Text(
                    "  No skills found.\n  \
                     Add .md files to ~/.rust-agent/skills/ to create skills."
                        .to_string(),
                ));
            }

            let mut lines = Vec::new();
            lines.push("  Available Skills".to_string());
            lines.push("  ────────────────".to_string());
            for skill in &skills {
                lines.push(format!(
                    "    /{:<16} {}",
                    skill.name, skill.description
                ));
            }
            return Ok(CommandResult::Text(lines.join("\n")));
        }

        // Find and invoke the skill
        if let Some(skill) = skills.iter().find(|s| s.name == skill_name) {
            let extra_args = if parts.len() > 1 {
                parts[1..].join(" ")
            } else {
                String::new()
            };

            let mut prompt = skill.prompt.clone();
            if !extra_args.is_empty() {
                prompt.push_str(&format!("\n\nAdditional instructions: {}", extra_args));
            }

            Ok(CommandResult::Prompt(PromptCommand {
                content: prompt,
                allowed_tools: skill.allowed_tools.clone(),
            }))
        } else {
            let available: Vec<String> = skills.iter().map(|s| s.name.clone()).collect();
            Ok(CommandResult::Text(format!(
                "  Skill '{}' not found.\n  Available: {}",
                skill_name,
                if available.is_empty() {
                    "(none)".to_string()
                } else {
                    available.join(", ")
                }
            )))
        }
    }
}

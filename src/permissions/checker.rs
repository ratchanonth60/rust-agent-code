//! Core permission check logic.
//!
//! [`check_permission`] is the single entry point called before every
//! tool execution.  It evaluates mode, rules, path safety, and
//! produces a [`PermissionDecision`].

use std::path::Path;

use regex::Regex;

use crate::permissions::path_safety::{is_dangerous_path, is_within_directory};
use crate::permissions::types::*;
use crate::tools::Tool;
use serde_json::Value;

/// Check whether a tool is allowed to execute given the current permission mode.
///
/// Decision chain:
/// 1. Plan mode → deny all destructive tools
/// 2. Read-only tools → always allow
/// 3. BypassPermissions → allow (unless dangerous path)
/// 4. Matching rules → apply rule behavior
/// 5. Dangerous path check → ask (bypass-immune)
/// 6. AcceptEdits + within working dir → allow writes
/// 7. Default → ask for destructive tools
pub fn check_permission(
    tool: &dyn Tool,
    input: &Value,
    mode: PermissionMode,
    cwd: &Path,
    rules: &[PermissionRule],
) -> PermissionDecision {
    let tool_name = tool.name();

    // 1. Plan mode: deny all destructive tools
    if mode == PermissionMode::Plan && tool.is_destructive() {
        return PermissionDecision::Deny {
            reason: format!("Plan mode is active — '{}' is destructive and cannot run in read-only mode.", tool_name),
        };
    }

    // 2. Read-only tools always pass
    if tool.is_read_only() {
        return PermissionDecision::Allow;
    }

    // 3. Check tool-specific rules (last match wins)
    let mut rule_decision: Option<PermissionDecision> = None;
    for rule in rules {
        if rule.tool_name == tool_name || rule.tool_name == "*" {
            // If rule has a pattern, check if input matches
            if let Some(ref pattern) = rule.pattern {
                let input_str = input.to_string();
                if !input_str.contains(pattern) {
                    continue;
                }
            }
            rule_decision = Some(match rule.behavior {
                RuleBehavior::Allow => PermissionDecision::Allow,
                RuleBehavior::Deny => PermissionDecision::Deny {
                    reason: format!("Denied by permission rule for '{}'", tool_name),
                },
                RuleBehavior::Ask => PermissionDecision::Ask {
                    tool_name: tool_name.to_string(),
                    description: format!("Rule requires confirmation for '{}'", tool_name),
                },
            });
        }
    }
    if let Some(decision) = rule_decision {
        return decision;
    }

    // 4. Dangerous path check (bypass-immune)
    if let Some(file_path) = extract_file_path(tool_name, input) {
        if is_dangerous_path(&file_path) {
            return PermissionDecision::Ask {
                tool_name: tool_name.to_string(),
                description: format!("'{}' targets a sensitive path: {}", tool_name, file_path),
            };
        }
    }

    // 5. BypassPermissions → allow everything else
    if mode == PermissionMode::BypassPermissions {
        return PermissionDecision::Allow;
    }

    // 6. AcceptEdits + within working dir → allow writes
    if mode == PermissionMode::AcceptEdits {
        let file_path = extract_file_path(tool_name, input);
        let in_cwd = file_path.as_deref().map_or(false, |p| is_within_directory(p, cwd));
        if in_cwd {
            return PermissionDecision::Allow;
        }
        if tool.is_destructive() {
            return PermissionDecision::Ask {
                tool_name: tool_name.to_string(),
                description: format!("Allow '{}' to execute?", tool_name),
            };
        }
        return PermissionDecision::Allow;
    }

    // 7. DontAsk mode → deny everything that would otherwise ask
    if mode == PermissionMode::DontAsk && tool.is_destructive() {
        return PermissionDecision::Deny {
            reason: format!("Non-interactive mode: '{}' requires permission.", tool_name),
        };
    }

    // 8. Default mode: ask for destructive tools
    if tool.is_destructive() {
        return PermissionDecision::Ask {
            tool_name: tool_name.to_string(),
            description: format!("Allow '{}' to execute?", tool_name),
        };
    }

    PermissionDecision::Allow
}

/// Extract a file path from tool input if applicable.
fn extract_file_path(tool_name: &str, input: &Value) -> Option<String> {
    match tool_name {
        "write_file" | "Write" | "Edit" | "read_file" | "Read" => {
            input.get("file_path").and_then(|v| v.as_str()).map(String::from)
        }
        "Bash" | "bash" => {
            // Best-effort extraction of target paths from destructive shell commands.
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            extract_bash_target_path(cmd)
        }
        _ => None,
    }
}

/// Best-effort regex extraction of file paths from shell commands.
///
/// Matches destructive commands (`rm`, `mv`, `cp`, `chmod`, `chown`) and
/// redirect operators (`>`, `>>`) followed by an absolute or relative path.
fn extract_bash_target_path(cmd: &str) -> Option<String> {
    // Pattern: destructive command followed by flags then a path
    let destructive_re = Regex::new(
        r"(?:^|\s|;|&&|\|\|)\s*(?:sudo\s+)?(?:rm|mv|cp|chmod|chown)\s+(?:-\S+\s+)*([/~][\w./_-]+|\.[\w./_-]+)"
    ).ok()?;
    if let Some(caps) = destructive_re.captures(cmd) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }

    // Pattern: redirect operators writing to a file
    let redirect_re = Regex::new(
        r">{1,2}\s*([/~][\w./_-]+|\.[\w./_-]+)"
    ).ok()?;
    if let Some(caps) = redirect_re.captures(cmd) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }

    None
}

/// Apply DontAsk transformation: convert Ask → Deny.
pub fn apply_mode_transform(decision: PermissionDecision, mode: PermissionMode) -> PermissionDecision {
    match (&decision, mode) {
        (PermissionDecision::Ask { tool_name, .. }, PermissionMode::DontAsk) => {
            PermissionDecision::Deny {
                reason: format!("Non-interactive mode: '{}' denied.", tool_name),
            }
        }
        _ => decision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_bash_path_rm() {
        let result = extract_bash_target_path("rm -rf /etc/passwd");
        assert_eq!(result, Some("/etc/passwd".to_string()));
    }

    #[test]
    fn extract_bash_path_sudo_rm() {
        let result = extract_bash_target_path("sudo rm -rf /var/log/syslog");
        assert_eq!(result, Some("/var/log/syslog".to_string()));
    }

    #[test]
    fn extract_bash_path_redirect() {
        let result = extract_bash_target_path("echo secret > ~/.ssh/authorized_keys");
        assert_eq!(result, Some("~/.ssh/authorized_keys".to_string()));
    }

    #[test]
    fn extract_bash_path_redirect_append() {
        let result = extract_bash_target_path("cat data >> /tmp/output.log");
        assert_eq!(result, Some("/tmp/output.log".to_string()));
    }

    #[test]
    fn extract_bash_path_safe_command() {
        let result = extract_bash_target_path("ls /tmp");
        assert_eq!(result, None);
    }

    #[test]
    fn extract_bash_path_chained() {
        let result = extract_bash_target_path("echo hi && rm /etc/passwd");
        assert_eq!(result, Some("/etc/passwd".to_string()));
    }

    #[test]
    fn extract_file_path_for_write_tool() {
        let input = json!({"file_path": "/home/user/test.rs", "content": "fn main() {}"});
        assert_eq!(extract_file_path("Write", &input), Some("/home/user/test.rs".to_string()));
    }

    #[test]
    fn extract_file_path_for_bash_tool() {
        let input = json!({"command": "rm -rf /dangerous/path"});
        assert_eq!(extract_file_path("Bash", &input), Some("/dangerous/path".to_string()));
    }
}

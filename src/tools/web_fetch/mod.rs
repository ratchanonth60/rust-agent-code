use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tools::{Tool, ToolContext, ToolResult};

pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "WebFetch"
    }

    fn description(&self) -> &str {
        "Fetch content from a URL and return it as text. HTML tags are stripped. \
         Use for retrieving web content, API responses, or documentation."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch content from",
                    "format": "uri"
                },
                "prompt": {
                    "type": "string",
                    "description": "Optional prompt describing what information to extract"
                }
            },
            "required": ["url"]
        })
    }

    fn is_destructive(&self) -> bool { false }
    fn is_read_only(&self) -> bool { true }
    fn is_concurrency_safe(&self) -> bool { true }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let url = input.get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'url' field"))?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("RustAgent/1.0")
            .build()?;

        let response = client.get(url).send().await?;
        let status = response.status();

        if !status.is_success() {
            return Ok(ToolResult::err(json!({
                "error": format!("HTTP {}", status),
                "url": url
            })));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let body = response.text().await?;

        // Simple HTML tag stripping (basic, no dependency needed)
        let text = if content_type.contains("html") {
            strip_html_tags(&body)
        } else {
            body
        };

        // Truncate very long responses
        let truncated = if text.len() > 50_000 {
            format!("{}...\n[Truncated: {} total chars]", &text[..50_000], text.len())
        } else {
            text
        };

        Ok(ToolResult::ok(json!({
            "url": url,
            "content": truncated,
            "content_type": content_type
        })))
    }
}

/// Basic HTML tag stripping without external dependencies.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;

    let lower = html.to_lowercase();
    let chars: Vec<char> = html.chars().collect();
    let lower_chars: Vec<char> = lower.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        if !in_tag && chars[i] == '<' {
            // Check for script/style start
            let remaining: String = lower_chars[i..].iter().take(10).collect();
            if remaining.starts_with("<script") {
                in_script = true;
            } else if remaining.starts_with("<style") {
                in_style = true;
            } else if remaining.starts_with("</script") {
                in_script = false;
            } else if remaining.starts_with("</style") {
                in_style = false;
            }
            in_tag = true;
        } else if in_tag && chars[i] == '>' {
            in_tag = false;
        } else if !in_tag && !in_script && !in_style {
            result.push(chars[i]);
        }
        i += 1;
    }

    // Collapse multiple whitespace/newlines
    let mut collapsed = String::with_capacity(result.len());
    let mut prev_was_whitespace = false;
    for c in result.chars() {
        if c.is_whitespace() {
            if !prev_was_whitespace {
                collapsed.push('\n');
            }
            prev_was_whitespace = true;
        } else {
            collapsed.push(c);
            prev_was_whitespace = false;
        }
    }

    collapsed.trim().to_string()
}

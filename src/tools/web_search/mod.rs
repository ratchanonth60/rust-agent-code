//! WebSearch tool — web search via Claude's native beta tool or fallback.
//!
//! When used with the Claude provider, this tool signals that the native
//! `web_search` beta capability should be enabled.  For other providers it
//! falls back to a simple search-engine URL fetch.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tools::{Tool, ToolContext, ToolResult};

/// Search the web for information.
pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "WebSearch"
    }

    fn description(&self) -> &str {
        "Search the web for current information. Returns search results with titles, \
         snippets, and URLs."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 5)"
                }
            },
            "required": ["query"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let query = input["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;
        let max_results = input["max_results"].as_u64().unwrap_or(5);

        // Use DuckDuckGo HTML lite as a dependency-free fallback.
        let encoded_query = urlencecode(query);
        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            encoded_query
        );

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (compatible; RustAgent/0.1)")
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        let resp = client.get(&url).send().await?;
        let body = resp.text().await?;

        // Parse results from DuckDuckGo HTML
        let results = parse_ddg_results(&body, max_results as usize);

        if results.is_empty() {
            Ok(ToolResult::ok(json!({
                "query": query,
                "results": [],
                "message": "No results found."
            })))
        } else {
            Ok(ToolResult::ok(json!({
                "query": query,
                "results": results
            })))
        }
    }
}

/// Minimal URL-encode for search queries.
fn urlencecode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            ' ' => out.push('+'),
            _ => {
                for b in c.to_string().as_bytes() {
                    out.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    out
}

/// Parse search results from DuckDuckGo HTML lite page.
fn parse_ddg_results(html: &str, max: usize) -> Vec<Value> {
    let mut results = Vec::new();

    // DuckDuckGo HTML lite uses <a class="result__a" href="...">title</a>
    // and <a class="result__snippet">snippet</a>
    for chunk in html.split("class=\"result__a\"") {
        if results.len() >= max {
            break;
        }
        if chunk.contains("href=\"") {
            let href = extract_between(chunk, "href=\"", "\"").unwrap_or_default();
            let title = extract_between(chunk, ">", "</a>").unwrap_or_default();

            // Find snippet in the same result block
            let snippet = if let Some(snip_pos) = chunk.find("result__snippet") {
                let rest = &chunk[snip_pos..];
                extract_between(rest, ">", "</").unwrap_or_default()
            } else {
                String::new()
            };

            if !href.is_empty() && !title.is_empty() {
                // DuckDuckGo wraps URLs in a redirect — extract the actual URL
                let actual_url = if href.contains("uddg=") {
                    extract_between(&href, "uddg=", "&")
                        .map(|u| urldecode(&u))
                        .unwrap_or(href.clone())
                } else {
                    href.clone()
                };

                results.push(json!({
                    "title": strip_html_tags(&title).trim().to_string(),
                    "url": actual_url,
                    "snippet": strip_html_tags(&snippet).trim().to_string(),
                }));
            }
        }
    }

    results
}

/// Extract text between two markers.
fn extract_between(s: &str, start: &str, end: &str) -> Option<String> {
    let start_pos = s.find(start)? + start.len();
    let rest = &s[start_pos..];
    let end_pos = rest.find(end)?;
    Some(rest[..end_pos].to_string())
}

/// Minimal URL decode.
fn urldecode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                out.push(byte as char);
            }
        } else if c == '+' {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

/// Strip HTML tags from a string.
fn strip_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    // Decode common entities
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

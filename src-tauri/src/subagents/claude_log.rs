//! One Claude Code transcript record as lines of the sub-agent's activity log ([`LogEntry`]),
//! the way Claude Code itself shows its conversation: the task, assistant text, each tool call
//! with its key argument and a short result.
//!
//! Privacy: with `excerpts: false` (the frontend setting) only assistant records are parsed and
//! only tool names leave this module; no prompt, text, tool input or result is read. With
//! excerpts, every text is first stripped of terminal sequences, control and invisible format
//! characters, then masked by [`redact`] on the whole text, then capped. Thinking blocks are
//! never shown.

use serde::Deserialize;
use serde_json::Value;

use super::claude::{clean, masked};
use super::redact::{redact, strip_controls};
use super::LogEntry;

/// Task prompts and assistant text: enough to read, bounded per entry.
pub const MAX_LOG_TEXT_CHARS: usize = 4000;
/// Key argument of a tool call (a path, a command line, a pattern).
pub const MAX_DETAIL_CHARS: usize = 200;
/// First line of a tool result.
pub const MAX_RESULT_CHARS: usize = 200;
const MAX_TOOL_CHARS: usize = 64;

/// Only `type` and `isMeta` are read first; other record types stop there.
#[derive(Deserialize)]
struct Head {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "isMeta")]
    is_meta: Option<bool>,
}

#[derive(Deserialize)]
struct Record<B> {
    message: Option<Message<B>>,
}

#[derive(Deserialize)]
struct Message<B> {
    content: Option<Content<B>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Content<B> {
    Text(String),
    Blocks(Vec<B>),
}

/// Without excerpts: the tool name is all that is deserialized.
#[derive(Deserialize)]
struct NameBlock {
    #[serde(rename = "type")]
    kind: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
    name: Option<String>,
    input: Option<Value>,
    content: Option<ResultContent>,
    is_error: Option<bool>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ResultContent {
    Text(String),
    Blocks(Vec<ResultBlock>),
}

#[derive(Deserialize)]
struct ResultBlock {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

/// The log lines of one transcript line, in order; unparsable lines and other record types
/// yield none.
pub fn entries(line: &[u8], excerpts: bool) -> Vec<LogEntry> {
    let Ok(head) = serde_json::from_slice::<Head>(line) else {
        return Vec::new();
    };
    if head.is_meta == Some(true) {
        return Vec::new();
    }
    match (head.kind.as_deref(), excerpts) {
        (Some("assistant"), false) => tool_names(line),
        (Some("assistant"), true) => assistant(line),
        (Some("user"), true) => user(line),
        _ => Vec::new(),
    }
}

fn tool_names(line: &[u8]) -> Vec<LogEntry> {
    let Ok(record) = serde_json::from_slice::<Record<NameBlock>>(line) else {
        return Vec::new();
    };
    let Some(Content::Blocks(blocks)) = record.message.and_then(|m| m.content) else {
        return Vec::new();
    };
    blocks
        .into_iter()
        .filter(|block| block.kind.as_deref() == Some("tool_use"))
        .filter_map(|block| block.name.as_deref().and_then(|n| clean(n, MAX_TOOL_CHARS)))
        .map(|name| LogEntry::Tool { name, detail: None })
        .collect()
}

fn blocks_of(line: &[u8]) -> Option<Content<Block>> {
    serde_json::from_slice::<Record<Block>>(line)
        .ok()?
        .message?
        .content
}

fn assistant(line: &[u8]) -> Vec<LogEntry> {
    match blocks_of(line) {
        Some(Content::Text(text)) => long_text(&text).map(LogEntry::text).into_iter().collect(),
        Some(Content::Blocks(blocks)) => blocks
            .into_iter()
            .filter_map(|block| match block.kind.as_deref() {
                Some("text") => block
                    .text
                    .as_deref()
                    .and_then(long_text)
                    .map(LogEntry::text),
                Some("tool_use") => {
                    let name = block
                        .name
                        .as_deref()
                        .and_then(|n| clean(n, MAX_TOOL_CHARS))?;
                    let detail = block
                        .input
                        .as_ref()
                        .and_then(|input| tool_detail(&name, input));
                    Some(LogEntry::Tool { name, detail })
                }
                _ => None,
            })
            .collect(),
        None => Vec::new(),
    }
}

/// User records are the task (text) and the results of tool calls.
fn user(line: &[u8]) -> Vec<LogEntry> {
    match blocks_of(line) {
        Some(Content::Text(text)) => long_text(&text).map(LogEntry::prompt).into_iter().collect(),
        Some(Content::Blocks(blocks)) => blocks
            .into_iter()
            .filter_map(|block| match block.kind.as_deref() {
                Some("text") => block
                    .text
                    .as_deref()
                    .and_then(long_text)
                    .map(LogEntry::prompt),
                Some("tool_result") => Some(result(block.content, block.is_error == Some(true))),
                _ => None,
            })
            .collect(),
        None => Vec::new(),
    }
}

fn long_text(text: &str) -> Option<String> {
    masked(text, MAX_LOG_TEXT_CHARS)
}

/// First non-empty line and how many more there are; Claude Code shows a summary, not the output.
fn result(content: Option<ResultContent>, error: bool) -> LogEntry {
    let text = match content {
        Some(ResultContent::Text(text)) => text,
        Some(ResultContent::Blocks(blocks)) => blocks
            .into_iter()
            .filter(|block| block.kind.as_deref() == Some("text"))
            .filter_map(|block| block.text)
            .collect::<Vec<_>>()
            .join("\n"),
        None => String::new(),
    };
    let redacted = redact(&strip_controls(&text));
    let mut lines = redacted.lines().filter(|line| !line.trim().is_empty());
    let first = lines.next().and_then(|line| clean(line, MAX_RESULT_CHARS));
    let more = u32::try_from(lines.count()).unwrap_or(u32::MAX);
    LogEntry::Result {
        text: first,
        more_lines: more,
        error,
    }
}

/// The input field Claude Code shows next to a tool's name. Unknown tools (MCP tools, newer
/// ones) show the first of the usual fields they have.
fn tool_detail(name: &str, input: &Value) -> Option<String> {
    const USUAL: &[&str] = &[
        "file_path",
        "notebook_path",
        "path",
        "command",
        "pattern",
        "url",
        "query",
        "description",
        "skill",
    ];
    let fields = input.as_object()?;
    let key = match name {
        "Bash" | "PowerShell" => "command",
        "Read" | "Write" | "Edit" | "MultiEdit" => "file_path",
        "NotebookEdit" => "notebook_path",
        "Grep" | "Glob" => "pattern",
        "WebFetch" => "url",
        "WebSearch" => "query",
        "Task" | "Agent" => "description",
        _ => USUAL
            .iter()
            .copied()
            .find(|key| fields.get(*key).is_some_and(Value::is_string))?,
    };
    let value = fields.get(key)?.as_str()?;
    let redacted = redact(&strip_controls(value));
    let mut lines = redacted
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let first = lines.next()?;
    // A multi-line command shows its first line and says that more follows.
    let shown = if lines.next().is_some() {
        format!("{first} \u{2026}")
    } else {
        first.to_owned()
    };
    clean(&shown, MAX_DETAIL_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant_line(blocks: &str) -> String {
        format!("{{\"type\":\"assistant\",\"isSidechain\":true,\"message\":{{\"role\":\"assistant\",\"content\":[{blocks}]}}}}")
    }

    fn user_line(content: &str) -> String {
        format!("{{\"type\":\"user\",\"isSidechain\":true,\"message\":{{\"role\":\"user\",\"content\":{content}}}}}")
    }

    fn tool(name: &str, detail: Option<&str>) -> LogEntry {
        LogEntry::Tool {
            name: name.into(),
            detail: detail.map(Into::into),
        }
    }

    #[test]
    fn prompt_text_tools_and_results_in_order() {
        assert_eq!(
            entries(user_line("\"Find the parser\"").as_bytes(), true),
            [LogEntry::prompt("Find the parser".into())]
        );
        let line = assistant_line(concat!(
            "{\"type\":\"thinking\",\"thinking\":\"THINKING-MARKER\"},",
            "{\"type\":\"text\",\"text\":\"Looking around\"},",
            "{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Read\",\"input\":{\"file_path\":\"E:\\\\work space\\\\a.rs\",\"limit\":5}},",
            "{\"type\":\"tool_use\",\"id\":\"t2\",\"name\":\"Bash\",\"input\":{\"command\":\"cargo test\\ncargo fmt\"}},",
            "{\"type\":\"tool_use\",\"id\":\"t3\",\"name\":\"mcp__x__lookup\",\"input\":{\"limit\":1,\"query\":\"term\"}},",
            "{\"type\":\"tool_use\",\"id\":\"t4\",\"name\":\"TodoWrite\",\"input\":{\"todos\":[]}}"
        ));
        assert_eq!(
            entries(line.as_bytes(), true),
            [
                LogEntry::text("Looking around".into()),
                tool("Read", Some("E:\\work space\\a.rs")),
                tool("Bash", Some("cargo test \u{2026}")),
                tool("mcp__x__lookup", Some("term")),
                tool("TodoWrite", None),
            ]
        );
        let results = user_line(concat!(
            "[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"\\n  line one\\nline two\\n\\nline three\"},",
            "{\"type\":\"tool_result\",\"tool_use_id\":\"t2\",\"is_error\":true,\"content\":[{\"type\":\"text\",\"text\":\"failed\"},{\"type\":\"image\"}]},",
            "{\"type\":\"tool_result\",\"tool_use_id\":\"t3\"}]"
        ));
        assert_eq!(
            entries(results.as_bytes(), true),
            [
                LogEntry::Result {
                    text: Some("line one".into()),
                    more_lines: 2,
                    error: false
                },
                LogEntry::Result {
                    text: Some("failed".into()),
                    more_lines: 0,
                    error: true
                },
                LogEntry::Result {
                    text: None,
                    more_lines: 0,
                    error: false
                },
            ]
        );
    }

    #[test]
    fn without_excerpts_only_tool_names_leave() {
        let line = assistant_line(concat!(
            "{\"type\":\"text\",\"text\":\"TEXT-MARKER\"},",
            "{\"type\":\"tool_use\",\"name\":\"Grep\",\"input\":{\"pattern\":\"INPUT-MARKER\"}}"
        ));
        assert_eq!(entries(line.as_bytes(), false), [tool("Grep", None)]);
        assert!(entries(user_line("\"PROMPT-MARKER\"").as_bytes(), false).is_empty());
        let result = user_line("[{\"type\":\"tool_result\",\"content\":\"RESULT-MARKER\"}]");
        assert!(entries(result.as_bytes(), false).is_empty());
    }

    #[test]
    fn other_records_and_broken_lines_yield_nothing() {
        for line in [
            "{\"type\":\"attachment\",\"text\":\"x\"}",
            "{\"type\":\"system\",\"content\":\"x\"}",
            "{\"type\":\"user\",\"isMeta\":true,\"message\":{\"content\":\"META-MARKER\"}}",
            "not json",
            "{\"type\":\"assistant\",\"message\":{\"content\":\"cut",
        ] {
            assert!(entries(line.as_bytes(), true).is_empty(), "{line}");
        }
    }

    #[test]
    fn texts_are_redacted_cleaned_and_capped() {
        let line = assistant_line(&format!(
            "{{\"type\":\"text\",\"text\":\"API_KEY=FAKEVALUE0123456789 \\u202e{}\"}},{{\"type\":\"tool_use\",\"name\":\"Bash\",\"input\":{{\"command\":\"curl -H 'Authorization: Bearer FAKEVALUE0123456789' {}\"}}}}",
            "z".repeat(5000),
            "y".repeat(500)
        ));
        let found = entries(line.as_bytes(), true);
        let LogEntry::Text { text } = &found[0] else {
            panic!("{found:?}");
        };
        assert!(text.starts_with("API_KEY=[redacted] zzz"), "{text}");
        assert_eq!(text.chars().count(), MAX_LOG_TEXT_CHARS);
        let LogEntry::Tool {
            detail: Some(detail),
            ..
        } = &found[1]
        else {
            panic!("{found:?}");
        };
        assert!(!detail.contains("FAKEVALUE"), "{detail}");
        assert_eq!(detail.chars().count(), MAX_DETAIL_CHARS);
        let result = user_line(&format!(
            "[{{\"type\":\"tool_result\",\"content\":\"token=FAKEVALUE0123456789 {}\"}}]",
            "r".repeat(400)
        ));
        let found = entries(result.as_bytes(), true);
        let LogEntry::Result {
            text: Some(text), ..
        } = &found[0]
        else {
            panic!("{found:?}");
        };
        assert!(text.starts_with("token=[redacted]"), "{text}");
        assert_eq!(text.chars().count(), MAX_RESULT_CHARS);
    }

    #[test]
    fn colored_secrets_are_masked_in_every_field() {
        // `ESC[31m` splits the name from its value until the sequence is stripped.
        let color = "\\u001b[31m";
        let reset = "\\u001b[0m";
        let line = assistant_line(&format!(
            "{{\"type\":\"text\",\"text\":\"{color}API_KEY{reset}=FAKEVALUE0123456789\"}},{{\"type\":\"tool_use\",\"name\":\"Bash\",\"input\":{{\"command\":\"{color}sshpass{reset} -p FAKEVALUE0123456789 ssh h\"}}}}"
        ));
        let result = user_line(&format!(
            "[{{\"type\":\"tool_result\",\"content\":\"{color}DB_PASS{reset}=FAKEVALUE0123456789\"}}]"
        ));
        let mut found = entries(line.as_bytes(), true);
        found.extend(entries(result.as_bytes(), true));
        assert_eq!(
            found,
            [
                LogEntry::text("API_KEY=[redacted]".into()),
                tool("Bash", Some("sshpass -p [redacted] ssh h")),
                LogEntry::Result {
                    text: Some("DB_PASS=[redacted]".into()),
                    more_lines: 0,
                    error: false
                },
            ]
        );
    }
}

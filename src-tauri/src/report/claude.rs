//! A Claude Code main transcript (`<project>/<session>.jsonl`) as the items of a report.
//!
//! Fields read (measured with 2.1.319 to 2.1.325,
//! docs/wiki/pages/sources/session-report-transcripts.md):
//! `type`, `isSidechain`, `isMeta`, `isCompactSummary`, `origin.kind`, `timestamp`, `gitBranch`
//! and `message.content`; of `user` records the text blocks and the `tool_use_id` of tool
//! results, of `assistant` records the text and `tool_use` blocks (`id`, `name`, `input`). Of a
//! structured tool result (`toolUseResult`) only `agentId` and `totalTokens` of a sub-agent run
//! are read. Thinking blocks, tool results and every other record type are never shown.
//!
//! Every text is stripped of terminal sequences, control and invisible format characters, then
//! masked by [`crate::subagents::redact::redact`], then capped ([`masked`]).

use std::collections::HashMap;
use std::io;
use std::path::Path;

use serde_json::Value;

use super::{Conversation, Item};
use crate::civil::parse_rfc3339_millis;
use crate::subagents::claude::{clean, masked};
use crate::subagents::claude_log::{skipped_cheaply, tool_detail};
use crate::subagents::is_agent_id;
use crate::subagents::tail::{Tail, MAX_READ};

/// Larger transcripts lose their start (marked in the report). Measured ones reach 12 MB.
pub const MAX_TRANSCRIPT_BYTES: u64 = 256 * 1024 * 1024;
/// One prompt or one assistant text.
pub const MAX_TEXT_CHARS: usize = 50_000;
/// The key argument of a tool call: a path or the first line of a command.
pub const MAX_DETAIL_CHARS: usize = 160;
const MAX_DESCRIPTION_CHARS: usize = 200;
const MAX_NAME_CHARS: usize = 64;
const MAX_BRANCH_CHARS: usize = 200;

/// Tool names of a sub-agent run (`Task` before Claude Code 2.1, `Agent` since).
const AGENT_TOOLS: &[&str] = &["Agent", "Task"];

/// User texts Claude Code writes itself: command output, notices, hook context. They start
/// with their tag.
const SKIPPED_TAGS: &[&str] = &[
    "<local-command-stdout>",
    "<local-command-stderr>",
    "<local-command-caveat>",
    "<bash-stdout>",
    "<bash-stderr>",
    "<task-notification>",
    "<system-reminder>",
    "<user-prompt-submit-hook>",
    "<command-message>",
];

/// Turns the lines of one main transcript into report items, in order.
pub struct Parser {
    /// The session's working folder: file paths below it are shown relative to it.
    base: Option<String>,
    items: Vec<Item>,
    /// Item index of each sub-agent call by its `tool_use` id, for the result that follows.
    agents: HashMap<String, usize>,
    started_at: Option<i64>,
    last_activity: Option<i64>,
    branch: Option<String>,
}

impl Parser {
    pub fn new(base: Option<&Path>) -> Self {
        Self {
            base: base
                .map(|path| {
                    path.to_string_lossy()
                        .trim_end_matches(['\\', '/'])
                        .to_owned()
                })
                .filter(|base| !base.is_empty()),
            items: Vec::new(),
            agents: HashMap::new(),
            started_at: None,
            last_activity: None,
            branch: None,
        }
    }

    /// Lines before this point were left out.
    pub fn gap(&mut self) {
        self.items.push(Item::Gap);
    }

    /// One transcript line; unparsable lines and other record types are skipped.
    pub fn line(&mut self, line: &[u8]) {
        if skipped_cheaply(line) {
            return;
        }
        let Ok(record) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        let kind = record.get("type").and_then(Value::as_str);
        if !matches!(kind, Some("user" | "assistant")) || is_true(&record, "isSidechain") {
            return;
        }
        if let Some(at) = record
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_millis)
        {
            self.started_at = Some(self.started_at.map_or(at, |first| first.min(at)));
            self.last_activity = Some(self.last_activity.map_or(at, |last| last.max(at)));
        }
        if let Some(branch) = record
            .get("gitBranch")
            .and_then(Value::as_str)
            .and_then(|b| masked(b, MAX_BRANCH_CHARS))
        {
            self.branch = Some(branch);
        }
        if is_true(&record, "isMeta") {
            return;
        }
        if kind == Some("user") {
            self.user(&record);
        } else {
            self.assistant(&record);
        }
    }

    pub fn finish(self) -> Conversation {
        Conversation {
            items: self.items,
            started_at: self.started_at,
            last_activity: self.last_activity,
            branch: self.branch,
        }
    }

    fn user(&mut self, record: &Value) {
        if is_true(record, "isCompactSummary") {
            self.items.push(Item::Compacted);
            return;
        }
        // Messages from other sessions, task notices and the like carry their own origin; only
        // what a person typed (or Claude Code recorded without an origin) is a prompt.
        if record
            .pointer("/origin/kind")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind != "human")
        {
            return;
        }
        match record.pointer("/message/content") {
            Some(Value::String(text)) => self.user_text(text),
            Some(Value::Array(blocks)) => {
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(Value::as_str) {
                                self.user_text(text);
                            }
                        }
                        Some("tool_result") => self.tool_result(block, record),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn user_text(&mut self, text: &str) {
        let start = text.trim_start();
        if start.starts_with("<command-name>") {
            let name = between(start, "<command-name>", "</command-name>").unwrap_or_default();
            let args = between(start, "<command-args>", "</command-args>").unwrap_or_default();
            let name = name.trim();
            let slash = if name.starts_with('/') { "" } else { "/" };
            let line = format!("{slash}{name} {}", args.trim());
            if let Some(command) = masked(&line, MAX_TEXT_CHARS) {
                self.items.push(Item::Command(command));
            }
            return;
        }
        if start.starts_with("<bash-input>") {
            let command = between(start, "<bash-input>", "</bash-input>").unwrap_or_default();
            let input = serde_json::json!({ "command": command });
            self.items.push(Item::Tool {
                name: "!".to_owned(),
                detail: tool_detail("Bash", &input, None, MAX_DETAIL_CHARS),
            });
            return;
        }
        if SKIPPED_TAGS.iter().any(|tag| start.starts_with(tag)) {
            return;
        }
        if let Some(prompt) = masked(text, MAX_TEXT_CHARS) {
            self.items.push(Item::Prompt(prompt));
        }
    }

    /// Only the result of a sub-agent run is read, for its token count and agent id.
    fn tool_result(&mut self, block: &Value, record: &Value) {
        let Some(index) = block
            .get("tool_use_id")
            .and_then(Value::as_str)
            .and_then(|id| self.agents.get(id))
        else {
            return;
        };
        let outcome = record.get("toolUseResult");
        let tokens = outcome
            .and_then(|o| o.get("totalTokens"))
            .and_then(Value::as_u64);
        let id = outcome
            .and_then(|o| o.get("agentId"))
            .and_then(Value::as_str)
            .filter(|id| is_agent_id(id));
        if let Some(Item::SubAgent {
            tokens: shown,
            agent_id,
            ..
        }) = self.items.get_mut(*index)
        {
            if tokens.is_some() {
                *shown = tokens;
            }
            if let Some(id) = id {
                *agent_id = Some(id.to_owned());
            }
        }
    }

    fn assistant(&mut self, record: &Value) {
        match record.pointer("/message/content") {
            Some(Value::String(text)) => self.assistant_text(text),
            Some(Value::Array(blocks)) => {
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(Value::as_str) {
                                self.assistant_text(text);
                            }
                        }
                        Some("tool_use") => self.tool_use(block),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn assistant_text(&mut self, text: &str) {
        if let Some(text) = masked(text, MAX_TEXT_CHARS) {
            self.items.push(Item::Text(text));
        }
    }

    fn tool_use(&mut self, block: &Value) {
        let Some(name) = block
            .get("name")
            .and_then(Value::as_str)
            .and_then(|n| clean(n, MAX_NAME_CHARS))
        else {
            return;
        };
        let input = block.get("input").unwrap_or(&Value::Null);
        if AGENT_TOOLS.contains(&name.as_str()) {
            let text = |key: &str, max: usize| {
                input
                    .get(key)
                    .and_then(Value::as_str)
                    .and_then(|t| masked(&t.replace(['\r', '\n'], " "), max))
            };
            if let Some(id) = block.get("id").and_then(Value::as_str) {
                self.agents.insert(id.to_owned(), self.items.len());
            }
            self.items.push(Item::SubAgent {
                description: text("description", MAX_DESCRIPTION_CHARS),
                agent_type: text("subagent_type", MAX_NAME_CHARS),
                agent_id: None,
                tokens: None,
            });
            return;
        }
        let detail = tool_detail(&name, input, self.base.as_deref(), MAX_DETAIL_CHARS);
        self.items.push(Item::Tool { name, detail });
    }
}

fn is_true(record: &Value, key: &str) -> bool {
    record.get(key).and_then(Value::as_bool) == Some(true)
}

/// The text between `open` and the next `close` after it.
fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let end = text[start..].find(close)? + start;
    Some(&text[start..end])
}

/// Reads the whole transcript at `path` (never through a link: [`Tail`] opens it with
/// [`crate::subagents::tail::open_shared`]), at most [`MAX_TRANSCRIPT_BYTES`] of its end.
pub fn read(path: &Path, base: Option<&Path>) -> io::Result<Conversation> {
    let mut tail = Tail::with_initial(MAX_TRANSCRIPT_BYTES);
    let mut parser = Parser::new(base);
    // A transcript that keeps growing while it is read must not hold the export forever.
    let mut rounds = MAX_TRANSCRIPT_BYTES / MAX_READ + 2;
    let mut first = true;
    loop {
        let lines = tail.read_new(path)?;
        if first && tail.skipped_head() {
            parser.gap();
        }
        first = false;
        for line in &lines {
            parser.line(line);
        }
        rounds = rounds.saturating_sub(1);
        if !tail.behind() || rounds == 0 {
            break;
        }
    }
    Ok(parser.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(lines: &[String], base: Option<&Path>) -> Conversation {
        let mut parser = Parser::new(base);
        for line in lines {
            parser.line(line.as_bytes());
        }
        parser.finish()
    }

    fn user(content: &str, at: &str) -> String {
        format!(
            "{{\"type\":\"user\",\"isSidechain\":false,\"timestamp\":\"{at}\",\"gitBranch\":\"feat/x\",\"origin\":{{\"kind\":\"human\"}},\"message\":{{\"role\":\"user\",\"content\":{content}}}}}"
        )
    }

    fn assistant(blocks: &str, at: &str) -> String {
        format!(
            "{{\"type\":\"assistant\",\"isSidechain\":false,\"timestamp\":\"{at}\",\"gitBranch\":\"feat/x\",\"message\":{{\"role\":\"assistant\",\"content\":[{blocks}]}}}}"
        )
    }

    fn tool(name: &str, detail: Option<&str>) -> Item {
        Item::Tool {
            name: name.into(),
            detail: detail.map(Into::into),
        }
    }

    #[test]
    fn prompts_text_and_tools_in_order() {
        let base = Path::new("E:\\work space");
        let lines = [
            user("\"Fix the parser\"", "2026-11-14T10:00:00.000Z"),
            assistant(
                concat!(
                    "{\"type\":\"thinking\",\"thinking\":\"THINKING-MARKER\"},",
                    "{\"type\":\"text\",\"text\":\"I will look.\"},",
                    "{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Read\",\"input\":{\"file_path\":\"E:\\\\work space\\\\src\\\\a.rs\"}},",
                    "{\"type\":\"tool_use\",\"id\":\"t2\",\"name\":\"Bash\",\"input\":{\"command\":\"cargo test\\ncargo fmt\",\"description\":\"Run tests\"}}"
                ),
                "2026-11-14T10:00:05.000Z",
            ),
            user(
                "[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"RESULT-MARKER\"}]",
                "2026-11-14T10:00:06.000Z",
            ),
            assistant(
                "{\"type\":\"text\",\"text\":\"Done.\"}",
                "2026-11-14T10:30:00.000Z",
            ),
        ];
        let conversation = parse(&lines, Some(base));
        assert_eq!(
            conversation.items,
            [
                Item::Prompt("Fix the parser".into()),
                Item::Text("I will look.".into()),
                tool("Read", Some("src\\a.rs")),
                tool("Bash", Some("cargo test \u{2026}")),
                Item::Text("Done.".into()),
            ]
        );
        assert_eq!(conversation.started_at, Some(1_794_650_400_000));
        assert_eq!(conversation.last_activity, Some(1_794_652_200_000));
        assert_eq!(conversation.branch.as_deref(), Some("feat/x"));
        let all = format!("{:?}", conversation.items);
        assert!(!all.contains("MARKER"), "{all}");
    }

    #[test]
    fn sub_agent_runs_become_one_item_with_their_tokens() {
        let lines = [
            assistant(
                concat!(
                    "{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Agent\",\"input\":{\"description\":\"Find the\\nparser\",\"subagent_type\":\"Explore\",\"prompt\":\"PROMPT-MARKER\"}},",
                    "{\"type\":\"tool_use\",\"id\":\"t2\",\"name\":\"Task\",\"input\":{\"description\":\"Background run\",\"run_in_background\":true}}"
                ),
                "2026-11-14T10:00:00Z",
            ),
            "{\"type\":\"user\",\"timestamp\":\"2026-11-14T10:01:00Z\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"OUTPUT-MARKER\"}]},\"toolUseResult\":{\"status\":\"completed\",\"agentId\":\"a0123456789abcdef\",\"totalTokens\":12345,\"content\":[{\"type\":\"text\",\"text\":\"OUTPUT-MARKER\"}]}}".to_owned(),
            "{\"type\":\"user\",\"timestamp\":\"2026-11-14T10:01:01Z\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t2\",\"content\":\"launched\"}]},\"toolUseResult\":{\"status\":\"async_launched\",\"agentId\":\"a1111111111111111\"}}".to_owned(),
        ];
        let conversation = parse(&lines, None);
        assert_eq!(
            conversation.items,
            [
                Item::SubAgent {
                    description: Some("Find the parser".into()),
                    agent_type: Some("Explore".into()),
                    agent_id: Some("a0123456789abcdef".into()),
                    tokens: Some(12_345),
                },
                Item::SubAgent {
                    description: Some("Background run".into()),
                    agent_type: None,
                    agent_id: Some("a1111111111111111".into()),
                    tokens: None,
                },
            ]
        );
        let all = format!("{:?}", conversation.items);
        assert!(!all.contains("MARKER"), "{all}");
    }

    #[test]
    fn claude_codes_own_records_are_left_out_or_condensed() {
        let lines = [
            user("\"<command-name>/model</command-name>\\n<command-message>model</command-message>\\n<command-args>opus</command-args>\"", "2026-11-14T10:00:00Z"),
            user("\"<local-command-stdout>Set model OUTPUT-MARKER</local-command-stdout>\"", "2026-11-14T10:00:01Z"),
            user("\"<bash-input>git status</bash-input>\"", "2026-11-14T10:00:02Z"),
            user("\"<bash-stdout>OUTPUT-MARKER</bash-stdout><bash-stderr></bash-stderr>\"", "2026-11-14T10:00:03Z"),
            "{\"type\":\"user\",\"isMeta\":true,\"message\":{\"content\":\"<local-command-caveat>META-MARKER</local-command-caveat>\"}}".to_owned(),
            "{\"type\":\"user\",\"origin\":{\"kind\":\"task-notification\"},\"message\":{\"content\":\"<task-notification>NOTICE-MARKER</task-notification>\"}}".to_owned(),
            "{\"type\":\"user\",\"origin\":{\"kind\":\"peer\",\"from\":\"a1\"},\"message\":{\"content\":\"PEER-MARKER\"}}".to_owned(),
            "{\"type\":\"user\",\"isCompactSummary\":true,\"message\":{\"content\":\"SUMMARY-MARKER\"}}".to_owned(),
            "{\"type\":\"user\",\"isSidechain\":true,\"message\":{\"content\":\"SIDECHAIN-MARKER\"}}".to_owned(),
            "{\"type\":\"api-request-blob\",\"message\":{\"content\":\"BLOB-MARKER\"}}".to_owned(),
            "{\"type\":\"ai-title\",\"aiTitle\":\"TITLE-MARKER\"}".to_owned(),
            "not json".to_owned(),
            user("[{\"type\":\"text\",\"text\":\"Look at this\"},{\"type\":\"image\",\"source\":{}}]", "2026-11-14T10:00:04Z"),
        ];
        let conversation = parse(&lines, None);
        assert_eq!(
            conversation.items,
            [
                Item::Command("/model opus".into()),
                tool("!", Some("git status")),
                Item::Compacted,
                Item::Prompt("Look at this".into()),
            ]
        );
        let all = format!("{:?}", conversation.items);
        assert!(!all.contains("MARKER"), "{all}");
    }

    #[test]
    fn escape_sequences_go_and_secrets_are_masked() {
        let lines = [
            user(
                "\"Use \\u001b[31mred\\u001b[0m and \\u001b]0;title\\u0007 API_KEY=sk-ant-FAKE0123456789abcdefFAKE\"",
                "2026-11-14T10:00:00Z",
            ),
            assistant(
                concat!(
                    "{\"type\":\"text\",\"text\":\"token ghp_FAKE0123456789abcdefFAKE0123 \\u0007bell\"},",
                    "{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\",\"input\":{\"command\":\"curl -H 'Authorization: Bearer FAKE0123456789abcdefFAKE' x\"}},",
                    "{\"type\":\"tool_use\",\"id\":\"t2\",\"name\":\"Agent\",\"input\":{\"description\":\"Check password=hunter2hunter2\"}}"
                ),
                "2026-11-14T10:00:01Z",
            ),
        ];
        let conversation = parse(&lines, None);
        let all = format!("{:?}", conversation.items);
        for leaked in ["FAKE0123", "hunter2", "\u{1b}", "\u{7}", "title"] {
            assert!(!all.contains(leaked), "{leaked} in {all}");
        }
        assert!(all.contains("[redacted]"), "{all}");
        assert!(all.contains("Use red and"), "{all}");
    }

    #[test]
    fn whole_files_are_read_without_their_unfinished_last_line() {
        let dir = crate::test_support::TempDir::new("report-read");
        let path = dir.path().join("t.jsonl");
        let mut text = String::new();
        for i in 0..3 {
            text.push_str(&user(&format!("\"prompt {i}\""), "2026-11-14T10:00:00Z"));
            text.push('\n');
        }
        text.push_str("{\"type\":\"user\",\"message\":{\"content\":\"half");
        std::fs::write(&path, text).unwrap();
        let conversation = read(&path, None).unwrap();
        assert_eq!(
            conversation.items,
            [
                Item::Prompt("prompt 0".into()),
                Item::Prompt("prompt 1".into()),
                Item::Prompt("prompt 2".into()),
            ]
        );
        assert!(read(&dir.path().join("missing.jsonl"), None).is_err());
    }
}

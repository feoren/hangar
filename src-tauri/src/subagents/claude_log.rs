//! Claude Code transcript records as lines of the sub-agent's activity log ([`LogEntry`]), the
//! way Claude Code itself shows a sub-agent's conversation: the task, assistant text, each tool
//! call with its key argument and a short result ("Read 120 lines", "Found 3 files", the first
//! lines of a command's output).
//!
//! Privacy: with `excerpts: false` (the frontend setting) only assistant records are parsed and
//! only tool names leave this module; no prompt, text, tool input or result is read. With
//! excerpts, every text is first stripped of terminal sequences, control and invisible format
//! characters, then masked by [`redact`] on the whole text, then capped. Thinking blocks are
//! never shown. Of a structured tool result (`toolUseResult`) only counts are read.

use std::path::Path;

use serde::de::IgnoredAny;
use serde::Deserialize;
use serde_json::Value;

use super::claude::{clean, masked};
use super::redact::{redact, strip_controls};
use super::{LogEntry, ResultSummary};

/// Task prompts and assistant text: enough to read, bounded per entry.
pub const MAX_LOG_TEXT_CHARS: usize = 4000;
/// Key argument of a tool call (a path, a command line, a pattern).
pub const MAX_DETAIL_CHARS: usize = 200;
/// One line of a tool result.
pub const MAX_RESULT_CHARS: usize = 200;
/// Lines of a tool result shown before "+N lines", as Claude Code does for command output.
pub const MAX_RESULT_LINES: usize = 3;
/// Target and note of the activity line in the sidebar.
pub const MAX_ACTIVITY_CHARS: usize = 120;
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

/// The structured result Claude Code stores next to a tool result (measured with 2.1.322, see
/// docs/wiki/pages/sources/subagent-transcripts.md). Read in a parse of its own: a field of
/// another shape (a newer version, an MCP tool) only loses the summary, never the entry.
#[derive(Deserialize)]
struct OutcomeRecord {
    #[serde(rename = "toolUseResult")]
    outcome: Option<Outcome>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Outcome {
    /// `Read`: `text` or `image`; `Write`: `create` or `update`.
    #[serde(rename = "type")]
    kind: Option<String>,
    file: Option<FileOutcome>,
    /// `Write`: the file's new text; `Grep` in content mode: the matching lines.
    content: Option<String>,
    structured_patch: Option<Vec<Hunk>>,
    /// Present on `Glob` and `Grep`; only its presence counts.
    filenames: Option<IgnoredAny>,
    /// `Grep`: `files_with_matches`, `content` or `count`.
    mode: Option<String>,
    num_files: Option<u64>,
    num_lines: Option<u64>,
    num_matches: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileOutcome {
    num_lines: Option<u64>,
}

#[derive(Deserialize)]
struct Hunk {
    lines: Vec<String>,
}

impl Outcome {
    fn summary(self) -> Option<ResultSummary> {
        let kind = self.kind.as_deref();
        if kind == Some("text") {
            return self
                .file?
                .num_lines
                .map(|lines| ResultSummary::Read { lines });
        }
        if kind == Some("create") {
            let lines = self.content?.lines().count();
            return Some(ResultSummary::Wrote {
                lines: u64::try_from(lines).unwrap_or(u64::MAX),
            });
        }
        if let Some(hunks) = self.structured_patch {
            let count = |mark: char| {
                let found = hunks
                    .iter()
                    .flat_map(|hunk| &hunk.lines)
                    .filter(|line| line.starts_with(mark))
                    .count();
                u64::try_from(found).unwrap_or(u64::MAX)
            };
            return Some(ResultSummary::Edited {
                added: count('+'),
                removed: count('-'),
            });
        }
        self.filenames.as_ref()?;
        match self.mode.as_deref() {
            Some("content") => self.num_lines.map(|count| ResultSummary::Lines { count }),
            Some("count") => self
                .num_matches
                .map(|count| ResultSummary::Matches { count }),
            _ => self.num_files.map(|count| ResultSummary::Files { count }),
        }
    }
}

/// Record types that never show in the log and are often large (`api-request-blob` carries
/// whole requests). They start with their `type`, so they are skipped without parsing the line.
pub fn skipped_cheaply(line: &[u8]) -> bool {
    const PREFIX: &[u8] = b"{\"type\":\"";
    let Some(rest) = line.strip_prefix(PREFIX) else {
        return false;
    };
    let kind = rest.split(|b| *b == b'"').next().unwrap_or_default();
    kind != b"user" && kind != b"assistant"
}

/// Turns the lines of one sub-agent's transcript into log entries, in order, and counts its tool
/// calls. One parser per read of a transcript: it keeps the count across lines.
#[derive(Debug, Clone)]
pub struct LogParser {
    excerpts: bool,
    /// The session's working folder: file paths below it are shown relative to it.
    base: Option<String>,
    tool_uses: u32,
}

impl LogParser {
    pub fn new(excerpts: bool, base: Option<&Path>) -> Self {
        Self {
            excerpts,
            base: base
                .map(|path| {
                    path.to_string_lossy()
                        .trim_end_matches(['\\', '/'])
                        .to_owned()
                })
                .filter(|base| !base.is_empty()),
            tool_uses: 0,
        }
    }

    /// Tool calls in every line parsed so far.
    pub fn tool_uses(&self) -> u32 {
        self.tool_uses
    }

    /// The log lines of one transcript line; unparsable lines and other record types yield none.
    pub fn entries(&mut self, line: &[u8]) -> Vec<LogEntry> {
        if skipped_cheaply(line) {
            return Vec::new();
        }
        let Ok(head) = serde_json::from_slice::<Head>(line) else {
            return Vec::new();
        };
        if head.is_meta == Some(true) {
            return Vec::new();
        }
        let found = match (head.kind.as_deref(), self.excerpts) {
            (Some("assistant"), false) => tool_names(line),
            (Some("assistant"), true) => self.assistant(line),
            (Some("user"), true) => user(line),
            _ => Vec::new(),
        };
        let calls = found
            .iter()
            .filter(|entry| matches!(entry, LogEntry::Tool { .. }))
            .count();
        self.tool_uses = self
            .tool_uses
            .saturating_add(u32::try_from(calls).unwrap_or(u32::MAX));
        found
    }

    fn assistant(&self, line: &[u8]) -> Vec<LogEntry> {
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
                        let detail = block.input.as_ref().and_then(|input| {
                            tool_detail(&name, input, self.base.as_deref(), MAX_DETAIL_CHARS)
                        });
                        Some(LogEntry::Tool { name, detail })
                    }
                    _ => None,
                })
                .collect(),
            None => Vec::new(),
        }
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

/// User records are the task (text) and the results of tool calls.
fn user(line: &[u8]) -> Vec<LogEntry> {
    match blocks_of(line) {
        Some(Content::Text(text)) => long_text(&text).map(LogEntry::prompt).into_iter().collect(),
        Some(Content::Blocks(blocks)) => {
            let results = blocks
                .iter()
                .filter(|block| block.kind.as_deref() == Some("tool_result"))
                .count();
            // The structured result belongs to the record, so it is only clear for one result.
            let mut summary = if results == 1 {
                serde_json::from_slice::<OutcomeRecord>(line)
                    .ok()
                    .and_then(|record| record.outcome)
                    .and_then(Outcome::summary)
            } else {
                None
            };
            blocks
                .into_iter()
                .filter_map(|block| match block.kind.as_deref() {
                    Some("text") => block
                        .text
                        .as_deref()
                        .and_then(long_text)
                        .map(LogEntry::prompt),
                    Some("tool_result") => {
                        let error = block.is_error == Some(true);
                        let summary = summary.take().filter(|_| !error);
                        Some(result(block.content, error, summary))
                    }
                    _ => None,
                })
                .collect()
        }
        None => Vec::new(),
    }
}

fn long_text(text: &str) -> Option<String> {
    masked(text, MAX_LOG_TEXT_CHARS)
}

/// A summary when there is one, else the first non-empty lines and how many more there are.
fn result(content: Option<ResultContent>, error: bool, summary: Option<ResultSummary>) -> LogEntry {
    if summary.is_some() {
        return LogEntry::Result {
            lines: Vec::new(),
            more_lines: 0,
            error,
            summary,
        };
    }
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
    let first: Vec<String> = lines
        .by_ref()
        .take(MAX_RESULT_LINES)
        .filter_map(|line| clean(line, MAX_RESULT_CHARS))
        .collect();
    let more = u32::try_from(lines.count()).unwrap_or(u32::MAX);
    LogEntry::Result {
        lines: first,
        more_lines: more,
        error,
        summary: None,
    }
}

/// Fields that name a file.
const PATH_FIELDS: &[&str] = &["file_path", "notebook_path", "path"];

/// The input field Claude Code shows next to a tool's name. Unknown tools (MCP tools, newer
/// ones) show the first of the usual fields they have. File paths below `base` are relative.
fn tool_detail(name: &str, input: &Value, base: Option<&str>, max: usize) -> Option<String> {
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
    let value = match base {
        Some(base) if PATH_FIELDS.contains(&key) => relative(value, base),
        _ => value,
    };
    first_line(value, max)
}

/// The first non-empty line, masked and capped; a following line is marked with an ellipsis.
fn first_line(value: &str, max: usize) -> Option<String> {
    let redacted = redact(&strip_controls(value));
    let mut lines = redacted
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let first = lines.next()?;
    let shown = if lines.next().is_some() {
        format!("{first} \u{2026}")
    } else {
        first.to_owned()
    };
    clean(&shown, max)
}

/// `path` without the leading `base` folder, if it lies below it. Windows paths compare without
/// regard to case, as the file system does.
fn relative<'a>(path: &'a str, base: &str) -> &'a str {
    let Some(head) = path.get(..base.len()) else {
        return path;
    };
    let same = if cfg!(windows) {
        head.eq_ignore_ascii_case(base)
    } else {
        head == base
    };
    match path[base.len()..].strip_prefix(['\\', '/']) {
        Some(rest) if same && !rest.is_empty() => rest,
        _ => path,
    }
}

/// What a tool call works on, for the sidebar's activity line: (`target`, `note`). `target` is a
/// file name instead of its path, else the call's key argument; `note` is the call's own one-line
/// description, which `Bash` and `PowerShell` calls carry and Claude Code shows instead.
pub fn activity(name: &str, input: &Value) -> (Option<String>, Option<String>) {
    let note = match name {
        "Bash" | "PowerShell" => input
            .get("description")
            .and_then(Value::as_str)
            .and_then(|text| first_line(text, MAX_ACTIVITY_CHARS)),
        _ => None,
    };
    let target = tool_detail(name, input, None, MAX_DETAIL_CHARS).map(|detail| {
        let is_path = matches!(
            name,
            "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit"
        );
        let shown = if is_path {
            detail
                .rsplit(['\\', '/'])
                .find(|part| !part.is_empty())
                .unwrap_or(&detail)
                .to_owned()
        } else {
            detail
        };
        clean(&shown, MAX_ACTIVITY_CHARS).unwrap_or_default()
    });
    (target.filter(|t| !t.is_empty()), note)
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

    /// A user record with one tool result and the structured `toolUseResult` beside it.
    fn outcome_line(result: &str, outcome: &str) -> String {
        format!("{{\"parentUuid\":\"p\",\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":{result}}}]}},\"toolUseResult\":{outcome}}}")
    }

    fn entries(line: &str, excerpts: bool) -> Vec<LogEntry> {
        LogParser::new(excerpts, None).entries(line.as_bytes())
    }

    fn tool(name: &str, detail: Option<&str>) -> LogEntry {
        LogEntry::Tool {
            name: name.into(),
            detail: detail.map(Into::into),
        }
    }

    fn lines(shown: &[&str], more_lines: u32, error: bool) -> LogEntry {
        LogEntry::Result {
            lines: shown.iter().map(|line| (*line).to_owned()).collect(),
            more_lines,
            error,
            summary: None,
        }
    }

    fn summary(summary: ResultSummary) -> LogEntry {
        LogEntry::Result {
            lines: Vec::new(),
            more_lines: 0,
            error: false,
            summary: Some(summary),
        }
    }

    #[test]
    fn prompt_text_tools_and_results_in_order() {
        assert_eq!(
            entries(&user_line("\"Find the parser\""), true),
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
        let mut parser = LogParser::new(true, None);
        assert_eq!(
            parser.entries(line.as_bytes()),
            [
                LogEntry::text("Looking around".into()),
                tool("Read", Some("E:\\work space\\a.rs")),
                tool("Bash", Some("cargo test \u{2026}")),
                tool("mcp__x__lookup", Some("term")),
                tool("TodoWrite", None),
            ]
        );
        assert_eq!(parser.tool_uses(), 4);
        let results = user_line(concat!(
            "[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"\\n  line one\\nline two\\n\\nline three\\nfour\\nfive\"},",
            "{\"type\":\"tool_result\",\"tool_use_id\":\"t2\",\"is_error\":true,\"content\":[{\"type\":\"text\",\"text\":\"failed\"},{\"type\":\"image\"}]},",
            "{\"type\":\"tool_result\",\"tool_use_id\":\"t3\"}]"
        ));
        assert_eq!(
            entries(&results, true),
            [
                lines(&["line one", "line two", "line three"], 2, false),
                lines(&["failed"], 0, true),
                lines(&[], 0, false),
            ]
        );
    }

    #[test]
    fn structured_results_become_summaries() {
        let cases = [
            (
                "{\"type\":\"text\",\"file\":{\"filePath\":\"a.rs\",\"content\":\"CONTENT-MARKER\",\"numLines\":120,\"startLine\":1,\"totalLines\":300}}",
                ResultSummary::Read { lines: 120 },
            ),
            (
                "{\"type\":\"create\",\"filePath\":\"a.rs\",\"content\":\"one\\ntwo\\nthree\\n\",\"structuredPatch\":[],\"originalFile\":null}",
                ResultSummary::Wrote { lines: 3 },
            ),
            (
                "{\"filePath\":\"a.rs\",\"oldString\":\"x\",\"newString\":\"y\",\"originalFile\":\"z\",\"structuredPatch\":[{\"oldStart\":1,\"oldLines\":2,\"newStart\":1,\"newLines\":3,\"lines\":[\" keep\",\"-old\",\"+new\",\"+newer\"]}]}",
                ResultSummary::Edited { added: 2, removed: 1 },
            ),
            (
                "{\"type\":\"update\",\"filePath\":\"a.rs\",\"content\":\"x\",\"structuredPatch\":[{\"lines\":[\"-a\",\"-b\",\"+c\"]},{\"lines\":[\"+d\"]}]}",
                ResultSummary::Edited { added: 2, removed: 2 },
            ),
            (
                "{\"mode\":\"files_with_matches\",\"filenames\":[\"a\",\"b\",\"c\"],\"numFiles\":3}",
                ResultSummary::Files { count: 3 },
            ),
            (
                "{\"filenames\":[\"a\"],\"durationMs\":4,\"numFiles\":1,\"truncated\":false}",
                ResultSummary::Files { count: 1 },
            ),
            (
                "{\"mode\":\"content\",\"content\":\"a:1:x\",\"filenames\":[],\"numFiles\":0,\"numLines\":14}",
                ResultSummary::Lines { count: 14 },
            ),
            (
                "{\"mode\":\"count\",\"content\":\"a:5\",\"filenames\":[],\"numFiles\":0,\"numMatches\":5}",
                ResultSummary::Matches { count: 5 },
            ),
        ];
        for (outcome, expected) in cases {
            let line = outcome_line("\"RESULT-MARKER\"", outcome);
            assert_eq!(entries(&line, true), [summary(expected)], "{outcome}");
        }
    }

    #[test]
    fn results_without_a_summary_show_their_first_lines() {
        // Command output (`stdout`), an image read, an error string and an unexpected shape all
        // fall back to the text of the result.
        for outcome in [
            "{\"stdout\":\"x\",\"stderr\":\"\",\"interrupted\":false,\"isImage\":false}",
            "{\"type\":\"image\",\"file\":{\"base64\":\"AAAA\",\"type\":\"image/png\"}}",
            "\"Error: no such file\"",
            "{\"content\":[1,2],\"filenames\":7}",
            "[{\"type\":\"text\",\"text\":\"mcp\"}]",
        ] {
            let line = outcome_line("\"one\\ntwo\\nthree\\nfour\"", outcome);
            assert_eq!(
                entries(&line, true),
                [lines(&["one", "two", "three"], 1, false)],
                "{outcome}"
            );
        }
        // An error never gets a summary, even with a structured result.
        let failed = "{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"is_error\":true,\"content\":\"denied\"}]},\"toolUseResult\":{\"filenames\":[],\"numFiles\":0}}";
        assert_eq!(entries(failed, true), [lines(&["denied"], 0, true)]);
    }

    #[test]
    fn file_paths_below_the_working_folder_are_relative() {
        let line = assistant_line(concat!(
            "{\"type\":\"tool_use\",\"name\":\"Read\",\"input\":{\"file_path\":\"E:\\\\work space\\\\src\\\\a.rs\"}},",
            "{\"type\":\"tool_use\",\"name\":\"Edit\",\"input\":{\"file_path\":\"E:\\\\other\\\\b.rs\"}},",
            "{\"type\":\"tool_use\",\"name\":\"Bash\",\"input\":{\"command\":\"E:\\\\work space\\\\run.cmd\"}}"
        ));
        let mut parser = LogParser::new(true, Some(Path::new("E:\\work space\\")));
        assert_eq!(
            parser.entries(line.as_bytes()),
            [
                tool("Read", Some("src\\a.rs")),
                tool("Edit", Some("E:\\other\\b.rs")),
                tool("Bash", Some("E:\\work space\\run.cmd")),
            ]
        );
        assert_eq!(relative("/home/a/x", "/home/a"), "x");
        assert_eq!(relative("/home/ab/x", "/home/a"), "/home/ab/x");
        assert_eq!(relative("/home/a", "/home/a"), "/home/a");
        assert_eq!(relative("/h\u{e4}", "/home/a"), "/h\u{e4}");
    }

    #[test]
    fn large_other_records_are_skipped_before_parsing() {
        assert!(skipped_cheaply(
            b"{\"type\":\"api-request-blob\",\"message\":{"
        ));
        assert!(skipped_cheaply(b"{\"type\":\"api-request\""));
        assert!(!skipped_cheaply(b"{\"type\":\"user\",\"message\":{}}"));
        assert!(!skipped_cheaply(b"{\"type\":\"assistant\",\"message\":{}}"));
        assert!(!skipped_cheaply(b"{\"parentUuid\":null,\"type\":\"user\"}"));
        // Not even valid JSON: skipped by the prefix alone.
        assert!(entries(
            "{\"type\":\"api-request-blob\",\"message\":{\"content\":\"x",
            true
        )
        .is_empty());
    }

    #[test]
    fn activity_names_the_file_or_the_description() {
        let input = |json: &str| serde_json::from_str::<Value>(json).unwrap();
        assert_eq!(
            activity(
                "Edit",
                &input("{\"file_path\":\"E:\\\\a b\\\\src\\\\git.rs\"}")
            ),
            (Some("git.rs".into()), None)
        );
        assert_eq!(
            activity("Read", &input("{\"file_path\":\"/home/x/styles.css\"}")),
            (Some("styles.css".into()), None)
        );
        assert_eq!(
            activity(
                "Bash",
                &input("{\"command\":\"py -3 inspect.py\",\"description\":\"Run inspect.py on transcripts\\nmore\"}")
            ),
            (
                Some("py -3 inspect.py".into()),
                Some("Run inspect.py on transcripts \u{2026}".into())
            )
        );
        assert_eq!(
            activity("Grep", &input("{\"pattern\":\"fn parse\"}")),
            (Some("fn parse".into()), None)
        );
        assert_eq!(
            activity("TodoWrite", &input("{\"todos\":[]}")),
            (None, None)
        );
        let (target, note) = activity(
            "Bash",
            &input("{\"command\":\"x\",\"description\":\"Use token=FAKEVALUE0123456789\"}"),
        );
        assert_eq!(target.as_deref(), Some("x"));
        assert_eq!(note.as_deref(), Some("Use token=[redacted]"));
        let (long, _) = activity(
            "Grep",
            &input(&format!("{{\"pattern\":\"{}\"}}", "p".repeat(500))),
        );
        assert_eq!(long.map(|t| t.chars().count()), Some(MAX_ACTIVITY_CHARS));
    }

    #[test]
    fn without_excerpts_only_tool_names_leave() {
        let line = assistant_line(concat!(
            "{\"type\":\"text\",\"text\":\"TEXT-MARKER\"},",
            "{\"type\":\"tool_use\",\"name\":\"Grep\",\"input\":{\"pattern\":\"INPUT-MARKER\"}}"
        ));
        let mut parser = LogParser::new(false, None);
        assert_eq!(parser.entries(line.as_bytes()), [tool("Grep", None)]);
        assert_eq!(parser.tool_uses(), 1);
        assert!(entries(&user_line("\"PROMPT-MARKER\""), false).is_empty());
        let result = outcome_line("\"RESULT-MARKER\"", "{\"filenames\":[],\"numFiles\":4}");
        assert!(entries(&result, false).is_empty());
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
            assert!(entries(line, true).is_empty(), "{line}");
        }
    }

    #[test]
    fn texts_are_redacted_cleaned_and_capped() {
        let line = assistant_line(&format!(
            "{{\"type\":\"text\",\"text\":\"API_KEY=FAKEVALUE0123456789 \\u202e{}\"}},{{\"type\":\"tool_use\",\"name\":\"Bash\",\"input\":{{\"command\":\"curl -H 'Authorization: Bearer FAKEVALUE0123456789' {}\"}}}}",
            "z".repeat(5000),
            "y".repeat(500)
        ));
        let found = entries(&line, true);
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
        let found = entries(&result, true);
        let LogEntry::Result { lines, .. } = &found[0] else {
            panic!("{found:?}");
        };
        assert!(lines[0].starts_with("token=[redacted]"), "{lines:?}");
        assert_eq!(lines[0].chars().count(), MAX_RESULT_CHARS);
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
        let mut found = entries(&line, true);
        found.extend(entries(&result, true));
        assert_eq!(
            found,
            [
                LogEntry::text("API_KEY=[redacted]".into()),
                tool("Bash", Some("sshpass -p [redacted] ssh h")),
                lines(&["DB_PASS=[redacted]"], 0, false),
            ]
        );
    }
}

//! A session's conversation as a readable Markdown report (contract:
//! docs/wiki/pages/syntheses/agent-workflow-features.md, "Session report").
//!
//! The source is the agent's own transcript, not the terminal: the prompts, the assistant's
//! text, one line per tool call (name and key argument, never its output) and one line per
//! sub-agent run (description and tokens), then the files changed in the work tree and the
//! commits made during the session. Only programs whose transcript Hangar can find for a
//! session are offered (the same [`SubAgentKind`] as the sub-agent view: Claude Code); Codex
//! rollouts cannot be linked to a session (docs/wiki/pages/sources/context-window-measurement.md).
//!
//! The window sends the labels (its language) and its UTC offset; the backend reads, cleans,
//! masks ([`crate::subagents::redact`]) and renders, then opens the save dialog itself.

pub mod claude;
pub mod markdown;

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::time::SystemTime;

use serde::{Deserialize, Serialize, Serializer};

use crate::export::{self, ExportError};
use crate::git::{self, FileChange, Git};
use crate::git_history::{self, CommitSummary, MAX_LOG_LIMIT};
use crate::manager::ManagerError;
use crate::subagents::claude::{masked, ClaudeSource};
use crate::subagents::{SubAgentKind, SubAgentSource as _, Target};

/// Commits authored up to this long after the last transcript record still count: a commit at
/// the end of a turn is often the last thing the agent does.
const COMMIT_SLACK_SECONDS: i64 = 5 * 60;
const MAX_TITLE_CHARS: usize = 200;
const MAX_LABEL_CHARS: usize = 80;
/// The window's offset from UTC is at most this far off (UTC-12 to UTC+14 in practice).
const MAX_OFFSET_MINUTES: i32 = 18 * 60;

/// One entry of the conversation. Every text is already cleaned, masked and capped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// What the user typed.
    Prompt(String),
    /// A slash command the user ran, with its arguments.
    Command(String),
    /// Assistant text, Markdown.
    Text(String),
    /// A tool call with its key argument (a path, the first line of a command).
    Tool {
        name: String,
        detail: Option<String>,
    },
    /// A sub-agent run.
    SubAgent {
        description: Option<String>,
        agent_type: Option<String>,
        agent_id: Option<String>,
        /// Context size of its newest response, as Claude Code shows it.
        tokens: Option<u64>,
    },
    /// The conversation was compacted here.
    Compacted,
    /// The start of a very long transcript was left out.
    Gap,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Conversation {
    pub items: Vec<Item>,
    /// Unix milliseconds of the first and the last record.
    pub started_at: Option<i64>,
    pub last_activity: Option<i64>,
    /// The branch the newest record names.
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFacts {
    /// The work tree's changes, as the Changes tab lists them.
    pub changed: Vec<FileChange>,
    pub changed_cut: bool,
    /// Commits on HEAD authored during the session, newest first.
    pub commits: Vec<CommitSummary>,
    /// Older commits of the session may exist beyond the page read.
    pub more_commits: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub title: String,
    pub program: String,
    pub folder: String,
    pub conversation: Conversation,
    /// `None` outside a repository or when git is missing.
    pub git: Option<GitFacts>,
}

/// The report's headings in the window's language (`ReportLabels` in api.ts). `tokens` holds
/// `{count}`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Labels {
    pub program: String,
    pub folder: String,
    pub branch: String,
    pub started: String,
    pub last_activity: String,
    pub duration: String,
    pub conversation: String,
    pub sub_agent: String,
    pub tokens: String,
    pub compacted: String,
    pub earlier_left_out: String,
    pub changed_files: String,
    pub commits: String,
    pub none: String,
}

impl Labels {
    /// One line each, without control characters, at most [`MAX_LABEL_CHARS`]; an empty text
    /// falls back to English.
    pub fn cleaned(self) -> Self {
        let clean = |text: String, fallback: &str| {
            let text: String = text
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .filter(|c| !crate::text::is_invisible(*c))
                .take(MAX_LABEL_CHARS)
                .collect();
            let text = text.trim();
            if text.is_empty() {
                fallback.to_owned()
            } else {
                text.to_owned()
            }
        };
        let tokens = clean(self.tokens, "{count} tokens");
        Self {
            program: clean(self.program, "Program"),
            folder: clean(self.folder, "Project folder"),
            branch: clean(self.branch, "Branch"),
            started: clean(self.started, "Started"),
            last_activity: clean(self.last_activity, "Last activity"),
            duration: clean(self.duration, "Duration"),
            conversation: clean(self.conversation, "Conversation"),
            sub_agent: clean(self.sub_agent, "Sub-agent"),
            tokens: if tokens.contains("{count}") {
                tokens
            } else {
                "{count} tokens".to_owned()
            },
            compacted: clean(self.compacted, "Conversation compacted"),
            earlier_left_out: clean(self.earlier_left_out, "Earlier part left out"),
            changed_files: clean(self.changed_files, "Uncommitted changes"),
            commits: clean(self.commits, "Commits in this period"),
            none: clean(self.none, "None"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error(transparent)]
    Session(ManagerError),
    #[error("Hangar cannot read the conversation of this program")]
    Unsupported,
    #[error("The conversation was not found yet; the program writes it with the first prompt")]
    NoTranscript,
    #[error("The conversation cannot be read: {0}")]
    Read(#[source] io::Error),
    #[error(transparent)]
    Export(#[from] ExportError),
    #[error("Background task aborted: {0}")]
    Task(String),
}

impl From<ManagerError> for ReportError {
    fn from(error: ManagerError) -> Self {
        match error {
            ManagerError::NoSubAgents(_) => Self::Unsupported,
            other => Self::Session(other),
        }
    }
}

/// Errors reach the window as text; none contains transcript content.
impl Serialize for ReportError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// What the report is about: the session's transcript target, the program's name and the
/// title the window shows.
#[derive(Debug, Clone)]
pub struct Source {
    pub target: Target,
    pub program: String,
    pub title: String,
}

/// Reads the transcript and git, renders the report and checks its size ([`export::MAX_BYTES`]).
pub fn build(source: &Source, labels: &Labels, offset_minutes: i32) -> Result<String, ReportError> {
    let target = &source.target;
    let mut locator = match target.kind {
        SubAgentKind::Claude => ClaudeSource::new(target, false),
    };
    let path = locator.main_transcript().ok_or(ReportError::NoTranscript)?;
    let mut conversation = claude::read(&path, Some(&target.cwd)).map_err(ReportError::Read)?;
    fill_agent_tokens(&mut conversation, || {
        locator
            .poll(SystemTime::now())
            .agents
            .into_iter()
            .filter_map(|agent| Some((agent.id, agent.tokens?)))
            .collect()
    });
    if conversation.branch.is_none() {
        conversation.branch =
            git::branch_of(&target.cwd, None).and_then(|branch| masked(&branch, MAX_TITLE_CHARS));
    }
    let git = git_facts(&target.cwd, &conversation);
    let report = Report {
        title: masked(&source.title, MAX_TITLE_CHARS).unwrap_or_else(|| source.program.clone()),
        program: masked(&source.program, MAX_TITLE_CHARS).unwrap_or_default(),
        folder: masked(&target.cwd.display().to_string(), usize::MAX).unwrap_or_default(),
        conversation,
        git,
    };
    let text = markdown::render(
        &report,
        labels,
        offset_minutes.clamp(-MAX_OFFSET_MINUTES, MAX_OFFSET_MINUTES),
    );
    export::check_size(&text)?;
    Ok(text)
}

/// A sub-agent run in the background reports no tokens in the main transcript; its own
/// transcript has them. `snapshot` (id to tokens) is only taken when one is missing.
fn fill_agent_tokens(
    conversation: &mut Conversation,
    snapshot: impl FnOnce() -> HashMap<String, u64>,
) {
    let missing = |item: &Item| {
        matches!(
            item,
            Item::SubAgent {
                agent_id: Some(_),
                tokens: None,
                ..
            }
        )
    };
    if !conversation.items.iter().any(missing) {
        return;
    }
    let tokens = snapshot();
    for item in &mut conversation.items {
        if let Item::SubAgent {
            agent_id: Some(id),
            tokens: shown @ None,
            ..
        } = item
        {
            *shown = tokens.get(id.as_str()).copied();
        }
    }
}

/// The work tree's changes and the session's commits; `None` without git or a repository. A git
/// failure leaves the sections out instead of failing the report.
fn git_facts(cwd: &Path, conversation: &Conversation) -> Option<GitFacts> {
    let git = Git::locate().ok()?;
    let (changed, changed_cut) = git::changed_files(&git, cwd).ok()??;
    let changed = changed
        .into_iter()
        .filter_map(|file| {
            Some(FileChange {
                path: masked(&file.path, usize::MAX)?,
                ..file
            })
        })
        .collect();
    let (commits, more_commits) = match (conversation.started_at, conversation.last_activity) {
        (Some(start), Some(end)) => git_history::log(&git, cwd, 0, MAX_LOG_LIMIT)
            .map(|log| session_commits(log.commits, log.has_more, start, end))
            .unwrap_or_default(),
        _ => (Vec::new(), false),
    };
    Some(GitFacts {
        changed,
        changed_cut,
        commits,
        more_commits,
    })
}

/// The commits of one page of the log (newest first) authored between `start` and `end` (Unix
/// milliseconds, plus [`COMMIT_SLACK_SECONDS`]), subjects masked. Whether older ones may follow:
/// the page went on beyond it and its oldest commit is still within the session.
pub fn session_commits(
    page: Vec<CommitSummary>,
    has_more: bool,
    start: i64,
    end: i64,
) -> (Vec<CommitSummary>, bool) {
    let (from, to) = (
        start.div_euclid(1000),
        end.div_euclid(1000) + COMMIT_SLACK_SECONDS,
    );
    let oldest_inside = page.last().is_some_and(|c| c.authored_at >= from);
    let commits = page
        .into_iter()
        .filter(|c| (from..=to).contains(&c.authored_at))
        .map(|c| CommitSummary {
            subject: masked(&c.subject, usize::MAX).unwrap_or_default(),
            ..c
        })
        .collect();
    (commits, has_more && oldest_inside)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::subagents::claude::CONFIG_DIR_VAR;
    use crate::test_support::TempDir;

    const SESSION: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";

    pub(crate) fn labels() -> Labels {
        Labels {
            program: "Program".into(),
            folder: "Project folder".into(),
            branch: "Branch".into(),
            started: "Started".into(),
            last_activity: "Last activity".into(),
            duration: "Duration".into(),
            conversation: "Conversation".into(),
            sub_agent: "Sub-agent".into(),
            tokens: "{count} tokens".into(),
            compacted: "Conversation compacted".into(),
            earlier_left_out: "Earlier part left out".into(),
            changed_files: "Changed files".into(),
            commits: "Commits".into(),
            none: "None".into(),
        }
    }

    fn commit(sha: char, at: i64, subject: &str) -> CommitSummary {
        CommitSummary {
            sha: sha.to_string().repeat(40),
            short_sha: sha.to_string().repeat(7),
            subject: subject.into(),
            author: "A".into(),
            authored_at: at,
            pushed: false,
        }
    }

    #[test]
    fn labels_arrive_as_documented_and_are_cleaned() {
        let json = serde_json::json!({
            "program": "Programm", "folder": "Projektordner", "branch": "Branch",
            "started": "Start", "lastActivity": "Letzte Aktivit\u{e4}t", "duration": "Dauer",
            "conversation": "Unterhaltung", "subAgent": "Sub-Agent", "tokens": "{count} Tokens",
            "compacted": "Verdichtet", "earlierLeftOut": "Fr\u{fc}her", "changedFiles": "Dateien",
            "commits": "Commits", "none": "Keine"
        });
        let parsed: Labels = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(parsed.last_activity, "Letzte Aktivit\u{e4}t");
        let mut extra = json;
        extra["other"] = "x".into();
        assert!(serde_json::from_value::<Labels>(extra).is_err());
        let cleaned = Labels {
            program: " \u{1b}[31mP\nx ".into(),
            tokens: "no placeholder".into(),
            none: String::new(),
            ..labels()
        }
        .cleaned();
        assert_eq!(cleaned.program, "[31mP x");
        assert_eq!(cleaned.tokens, "{count} tokens");
        assert_eq!(cleaned.none, "None");
    }

    #[test]
    fn commits_of_the_session_window() {
        let start = 1_000_000_000;
        let end = 1_000_003_600_000;
        let page = vec![
            commit('d', 1_000_003_600 + 600, "later"),
            commit('c', 1_000_003_600 + 100, "token=FAKEFAKEFAKEFAKE1234"),
            commit('b', 1_000_000_500, "inside"),
            commit('a', 999_999_000, "before"),
        ];
        let (commits, more) = session_commits(page.clone(), true, start * 1000, end);
        let subjects: Vec<&str> = commits.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!(subjects, ["token=[redacted]", "inside"]);
        // The page reached back before the session: nothing older belongs to it.
        assert!(!more);
        let (_, more) = session_commits(page[..3].to_vec(), true, start * 1000, end);
        assert!(more);
    }

    #[test]
    fn background_agents_get_tokens_from_their_own_transcript() {
        let mut conversation = Conversation {
            items: vec![
                Item::SubAgent {
                    description: None,
                    agent_type: None,
                    agent_id: Some("a1".into()),
                    tokens: None,
                },
                Item::SubAgent {
                    description: None,
                    agent_type: None,
                    agent_id: Some("a2".into()),
                    tokens: Some(5),
                },
            ],
            ..Conversation::default()
        };
        fill_agent_tokens(&mut conversation, || [("a1".to_owned(), 42)].into());
        let tokens: Vec<Option<u64>> = conversation
            .items
            .iter()
            .map(|item| match item {
                Item::SubAgent { tokens, .. } => *tokens,
                _ => None,
            })
            .collect();
        assert_eq!(tokens, [Some(42), Some(5)]);
        // Nothing missing: no snapshot is taken.
        fill_agent_tokens(&mut conversation, || panic!("snapshot taken"));
    }

    /// A made-up session: config folder, project folder, main transcript and one sub-agent run
    /// in the background whose transcript holds its tokens.
    #[test]
    fn a_whole_report_from_a_transcript_on_disk() {
        let dir = TempDir::new("report-build");
        let cwd = dir.path().join("work space");
        fs::create_dir_all(&cwd).unwrap();
        let config = dir.path().join("config");
        let project = config
            .join("projects")
            .join(crate::subagents::claude::encode_project(&cwd));
        let subagents = project.join(SESSION).join("subagents");
        fs::create_dir_all(&subagents).unwrap();
        let lines = [
            "{\"type\":\"user\",\"timestamp\":\"2026-11-14T10:00:00Z\",\"gitBranch\":\"feat/report\",\"origin\":{\"kind\":\"human\"},\"message\":{\"content\":\"Explain \\u001b[1mthis\\u001b[0m with key sk-proj-FAKE0123456789abcdefFAKE\"}}",
            "{\"type\":\"assistant\",\"timestamp\":\"2026-11-14T10:00:10Z\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"```rust\\nfn open() {}\"}]}}",
            "{\"type\":\"assistant\",\"timestamp\":\"2026-11-14T10:00:20Z\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Agent\",\"input\":{\"description\":\"Survey the code\",\"subagent_type\":\"Explore\",\"run_in_background\":true}}]}}",
            "{\"type\":\"user\",\"timestamp\":\"2026-11-14T10:00:21Z\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"launched\"}]},\"toolUseResult\":{\"status\":\"async_launched\",\"agentId\":\"a0123456789abcdef\"}}",
            "{\"type\":\"assistant\",\"timestamp\":\"2026-11-14T11:02:20Z\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"id\":\"t2\",\"name\":\"Edit\",\"input\":{\"file_path\":\"src/`odd`.rs\"}}]}}",
        ];
        fs::write(
            project.join(format!("{SESSION}.jsonl")),
            lines.join("\n") + "\n",
        )
        .unwrap();
        fs::write(
            subagents.join("agent-a0123456789abcdef.jsonl"),
            "{\"type\":\"assistant\",\"message\":{\"content\":[],\"usage\":{\"input_tokens\":7000,\"output_tokens\":345}}}\n",
        )
        .unwrap();
        let source = Source {
            target: Target {
                kind: SubAgentKind::Claude,
                session: SESSION.into(),
                cwd: cwd.clone(),
                env: [(CONFIG_DIR_VAR.to_owned(), config.display().to_string())].into(),
                home: dir.path().join("home"),
            },
            program: "Claude Code".into(),
            title: "Report # test".into(),
        };
        let text = build(&source, &labels(), 60).unwrap();
        assert!(
            text.starts_with("# Report \\# test\n\n- **Program:** Claude Code\n"),
            "{text}"
        );
        for expected in [
            "- **Branch:** `feat/report`",
            "- **Started:** 2026-11-14 11:00 UTC+01:00",
            "- **Last activity:** 2026-11-14 12:02 UTC+01:00",
            "- **Duration:** 1 h 02 min",
            "## Conversation",
            "> Explain this with key [redacted]",
            "```rust\nfn open() {}\n```",
            "- **Sub-agent** (Explore): Survey the code, 7345 tokens\n- **Edit** ``src/`odd`.rs``",
        ] {
            assert!(text.contains(expected), "{expected:?} missing in\n{text}");
        }
        assert!(!text.contains("FAKE0123"), "{text}");
        assert!(!text.contains('\u{1b}'), "{text}");

        // Not found yet: a clear error, nothing else.
        let missing = Source {
            target: Target {
                session: "11111111-2222-4333-8444-555555555555".into(),
                ..source.target.clone()
            },
            ..source
        };
        assert!(matches!(
            build(&missing, &labels(), 0),
            Err(ReportError::NoTranscript)
        ));
    }
}

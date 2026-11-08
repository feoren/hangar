//! Sub-agents of Claude Code (measured with 2.1.319, docs/wiki/pages/sources/subagent-transcripts.md):
//!
//! ```text
//! <config>/projects/<encoded cwd>/<session uuid>.jsonl            main transcript
//! <config>/projects/<encoded cwd>/<session uuid>/subagents/
//!     agent-<agentId>.jsonl       records of one sub-agent, appended while it runs
//!     agent-<agentId>.meta.json   agentType, description, spawnDepth, finished
//! ```
//!
//! `<config>` is `CLAUDE_CONFIG_DIR` or `~/.claude`. The project folder is found by looking for
//! the session's transcript, not computed: how Claude encodes long paths is unverified.
//!
//! Privacy: only a whitelist leaves this module ([`SubAgent`]). User records (prompts, tool
//! results) and every other record type are never parsed; of assistant records only the text
//! blocks and the tool names are read, never tool inputs.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use super::redact::redact;
use super::tail::{open_shared, Tail};
use super::{AgentStatus, Snapshot, SubAgent, SubAgentSource, Target};

/// Variable that moves Claude's config folder; read from the profile's `env` first.
pub const CONFIG_DIR_VAR: &str = "CLAUDE_CONFIG_DIR";
/// No writes for this long and not finished: shown as stale (crashed, cancelled or waiting).
pub const STALE_AFTER: Duration = Duration::from_secs(60);
/// While the transcript is not found yet (it appears with the first prompt), the search is
/// repeated after this delay, doubled after every miss up to [`MAX_LOCATE_DELAY`]: 2, 4, 8, 16,
/// 30, 30, ... seconds.
pub const LOCATE_INTERVAL: Duration = Duration::from_secs(2);
pub const MAX_LOCATE_DELAY: Duration = Duration::from_secs(30);
pub const MAX_EXCERPT_CHARS: usize = 400;
const MAX_DESCRIPTION_CHARS: usize = 200;
const MAX_TYPE_CHARS: usize = 64;
const MAX_TOOL_CHARS: usize = 64;
const MAX_META_BYTES: u64 = 64 * 1024;
/// Bounds the work per poll for a session that spawned very many sub-agents; the newest win.
pub const MAX_AGENTS: usize = 256;
/// Bounds the search through the project folders.
const MAX_PROJECT_DIRS: usize = 10_000;

/// `CLAUDE_CONFIG_DIR` from the profile, else from Hangar's environment (the child inherits it),
/// else `~/.claude`. A relative value is taken relative to the session's folder, as the child
/// would resolve it.
pub fn config_dir(target: &Target) -> PathBuf {
    let value = target
        .env
        .get(CONFIG_DIR_VAR)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os(CONFIG_DIR_VAR).map(PathBuf::from))
        .filter(|path| !path.as_os_str().is_empty());
    match value {
        Some(path) if path.is_absolute() => path,
        Some(path) => target.cwd.join(path),
        None => target.home.join(".claude"),
    }
}

/// Claude's folder name for a working folder: every character that is not an ASCII letter or
/// digit becomes `-` (measured: `E:\a\b` gives `E--a-b`).
pub fn encode_project(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Agent ids as they appear in file names (measured: `a` plus 16 hex digits). Anything that could
/// leave the folder or look like another file is rejected before a path is built from it.
pub fn is_agent_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn agent_id_of(file_name: &str) -> Option<(&str, FileKind)> {
    let rest = file_name.strip_prefix("agent-")?;
    let (id, kind) = if let Some(id) = rest.strip_suffix(".meta.json") {
        (id, FileKind::Meta)
    } else {
        (rest.strip_suffix(".jsonl")?, FileKind::Transcript)
    };
    is_agent_id(id).then_some((id, kind))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileKind {
    Transcript,
    Meta,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    agent_type: Option<String>,
    description: Option<String>,
    spawn_depth: Option<u32>,
    finished: Option<bool>,
}

/// Size and modification time; a change means the file must be read again.
type Stamp = (u64, Option<SystemTime>);

#[derive(Debug, Default)]
struct AgentState {
    tail: Tail,
    meta: Meta,
    meta_stamp: Option<Stamp>,
    /// Stamp of the last successful transcript read; see [`ClaudeSource::update_agent`].
    transcript_stamp: Option<Stamp>,
    started_at: Option<SystemTime>,
    last_activity: Option<SystemTime>,
    tool: Option<String>,
    excerpt: Option<String>,
}

pub struct ClaudeSource {
    projects: PathBuf,
    session: String,
    cwd: PathBuf,
    /// `<project>/<session>/subagents`, once the project folder is found.
    subagents: Option<PathBuf>,
    /// The projects folder after resolving links, for the containment check on every poll.
    root: Option<PathBuf>,
    next_locate: Option<Instant>,
    locate_delay: Duration,
    /// Modification time of the projects folder at the last full search (outer `None`: none yet).
    /// A new project folder changes it; until then only the guessed folder is checked.
    scanned: Option<Option<SystemTime>>,
    /// `false`: assistant text is never read, `excerpt` stays `null`.
    excerpts: bool,
    agents: BTreeMap<String, AgentState>,
}

impl ClaudeSource {
    /// `target.session` must already be a validated UUID ([`crate::cli_session::validate`]).
    pub fn new(target: &Target, excerpts: bool) -> Self {
        Self {
            projects: config_dir(target).join("projects"),
            session: target.session.clone(),
            cwd: target.cwd.clone(),
            subagents: None,
            root: None,
            next_locate: None,
            locate_delay: LOCATE_INTERVAL,
            scanned: None,
            excerpts,
            agents: BTreeMap::new(),
        }
    }

    /// The project folder that holds this session, searched first under the encoded working
    /// folder, then (with `full`) in every project folder. Only real folders inside `projects`
    /// count; symbolic links and junctions are skipped, so nothing outside the projects folder is
    /// ever read.
    fn locate(&self, full: bool) -> Option<PathBuf> {
        let root = fs::canonicalize(&self.projects).ok()?;
        let holds_session = |dir: &Path| {
            is_real_dir(dir)
                && (is_real_file(&dir.join(format!("{}.jsonl", self.session)))
                    || is_real_dir(&dir.join(&self.session)))
        };
        let inside = |dir: PathBuf| {
            fs::canonicalize(&dir)
                .ok()
                .filter(|real| real.starts_with(&root) && real != &root)
                .map(|_| dir)
        };
        let guess = self.projects.join(encode_project(&self.cwd));
        if holds_session(&guess) {
            return inside(guess);
        }
        if !full {
            return None;
        }
        fs::read_dir(&self.projects)
            .ok()?
            .take(MAX_PROJECT_DIRS)
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
            .map(|entry| entry.path())
            .find(|dir| holds_session(dir))
            .and_then(inside)
    }

    fn subagents_dir(&mut self, now: Instant) -> Option<PathBuf> {
        if self.subagents.is_none() && self.next_locate.is_none_or(|at| now >= at) {
            // A transcript in an already existing, not guessed project folder (a working folder
            // Claude encodes differently) changes no mtime of the projects folder; the full search
            // at the longest delay still finds it.
            let mtime = fs::metadata(&self.projects).and_then(|m| m.modified()).ok();
            let full = self.scanned != Some(mtime) || self.locate_delay >= MAX_LOCATE_DELAY;
            if full {
                self.scanned = Some(mtime);
            }
            self.subagents = self
                .locate(full)
                .map(|project| project.join(&self.session).join("subagents"));
            self.root = fs::canonicalize(&self.projects).ok();
            if self.subagents.is_none() {
                self.next_locate = Some(now + self.locate_delay);
                self.locate_delay = (self.locate_delay * 2).min(MAX_LOCATE_DELAY);
            }
        }
        self.subagents.clone()
    }

    /// The sub-agent folder, if it exists and does not lead out of the projects folder through
    /// a link in `<session>` or `subagents`.
    fn contained(&self, dir: &Path) -> bool {
        let Some(root) = &self.root else {
            return false;
        };
        fs::canonicalize(dir).is_ok_and(|real| real.starts_with(root))
    }

    /// Agent files in the folder: id to (transcript, meta) presence, newest [`MAX_AGENTS`].
    fn scan(dir: &Path) -> BTreeMap<String, (Option<Stamp>, Option<Stamp>)> {
        let mut found: BTreeMap<String, (Option<Stamp>, Option<Stamp>)> = BTreeMap::new();
        // The folder appears with the first sub-agent; it must not be a link to elsewhere.
        if !is_real_dir(dir) {
            return found;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return found;
        };
        // Two files per agent; the bound keeps a folder flooded with other files cheap.
        for entry in entries.take(4 * MAX_AGENTS).filter_map(Result::ok) {
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            let name = entry.file_name();
            let Some((id, kind)) = name.to_str().and_then(agent_id_of) else {
                continue;
            };
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let stamp = (meta.len(), meta.modified().ok());
            let slot = found.entry(id.to_owned()).or_default();
            match kind {
                FileKind::Transcript => slot.0 = Some(stamp),
                FileKind::Meta => slot.1 = Some(stamp),
            }
        }
        if found.len() > MAX_AGENTS {
            let newest = |(t, m): &(Option<Stamp>, Option<Stamp>)| {
                t.and_then(|s| s.1).max(m.and_then(|s| s.1))
            };
            let mut order: Vec<(Option<SystemTime>, String)> = found
                .iter()
                .map(|(id, v)| (newest(v), id.clone()))
                .collect();
            order.sort_by(|a, b| b.cmp(a));
            let keep: BTreeSet<String> = order
                .into_iter()
                .take(MAX_AGENTS)
                .map(|(_, id)| id)
                .collect();
            found.retain(|id, _| keep.contains(id));
        }
        found
    }

    fn update_agent(
        dir: &Path,
        id: &str,
        state: &mut AgentState,
        files: (Option<Stamp>, Option<Stamp>),
        excerpts: bool,
    ) {
        let (transcript, meta) = files;
        if let Some((_, modified)) = transcript {
            let path = dir.join(format!("agent-{id}.jsonl"));
            if state.started_at.is_none() {
                state.started_at = fs::metadata(&path)
                    .and_then(|m| m.created())
                    .ok()
                    .or(modified);
            }
            state.last_activity = modified;
            // A finished sub-agent writes nothing more: its transcript is only opened again when
            // its size or time changes, so a session with hundreds of finished sub-agents costs one
            // directory listing per poll instead of hundreds of opens. Running ones are always read,
            // because on Windows the directory entry of a file that is held open can lag behind.
            let settled = state.meta.finished == Some(true) && state.transcript_stamp == transcript;
            // A read error (file just deleted, locked for a moment) is retried on the next poll.
            if !settled {
                if let Ok(lines) = state.tail.read_new(&path) {
                    state.transcript_stamp = transcript;
                    for line in lines {
                        apply_record(state, &line, excerpts);
                    }
                }
            }
        }
        if meta.is_some() && meta != state.meta_stamp {
            if let Some(parsed) = read_meta(&dir.join(format!("agent-{id}.meta.json"))) {
                state.meta = parsed;
                state.meta_stamp = meta;
            }
            if state.started_at.is_none() {
                state.started_at = meta.and_then(|m| m.1);
            }
        }
    }
}

impl SubAgentSource for ClaudeSource {
    fn poll(&mut self, now: SystemTime) -> Snapshot {
        let Some(dir) = self.subagents_dir(Instant::now()) else {
            return Snapshot::default();
        };
        let found = if self.contained(&dir) {
            Self::scan(&dir)
        } else {
            BTreeMap::new()
        };
        self.agents.retain(|id, _| found.contains_key(id));
        for (id, files) in found {
            let state = self.agents.entry(id.clone()).or_default();
            Self::update_agent(&dir, &id, state, files, self.excerpts);
        }
        let mut agents: Vec<SubAgent> = self
            .agents
            .iter()
            .map(|(id, state)| view(id, state, now))
            .collect();
        agents.sort_by(|a, b| (a.started_at, &a.id).cmp(&(b.started_at, &b.id)));
        Snapshot {
            located: true,
            agents,
        }
    }
}

fn view(id: &str, state: &AgentState, now: SystemTime) -> SubAgent {
    let last_activity = state.last_activity.or(state.meta_stamp.and_then(|m| m.1));
    let status = if state.meta.finished == Some(true) {
        AgentStatus::Finished
    } else if last_activity
        .and_then(|at| now.duration_since(at).ok())
        .is_some_and(|idle| idle > STALE_AFTER)
    {
        AgentStatus::Stale
    } else {
        AgentStatus::Running
    };
    SubAgent {
        id: id.to_owned(),
        description: state
            .meta
            .description
            .as_deref()
            .and_then(|d| clean(&redact(d), MAX_DESCRIPTION_CHARS)),
        agent_type: state
            .meta
            .agent_type
            .as_deref()
            .and_then(|t| clean(t, MAX_TYPE_CHARS)),
        spawn_depth: state.meta.spawn_depth,
        status,
        started_at: state.started_at.map(millis),
        last_activity: last_activity.map(millis),
        tool: state.tool.clone(),
        excerpt: state.excerpt.clone(),
    }
}

/// Only `type` is read first; everything but assistant records stops there.
#[derive(Deserialize)]
struct Head {
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
struct AssistantRecord {
    message: Option<AssistantMessage>,
}

#[derive(Deserialize)]
struct AssistantMessage {
    content: Option<Content>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Content {
    Text(String),
    Blocks(Vec<Block>),
}

/// `input` of a tool call is never deserialized.
#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
    name: Option<String>,
}

/// Unparsable lines (a CLI mid-write, a future format) are skipped.
fn apply_record(state: &mut AgentState, line: &[u8], excerpts: bool) {
    let Ok(head) = serde_json::from_slice::<Head>(line) else {
        return;
    };
    if head.kind.as_deref() != Some("assistant") {
        return;
    }
    let Ok(record) = serde_json::from_slice::<AssistantRecord>(line) else {
        return;
    };
    let Some(content) = record.message.and_then(|m| m.content) else {
        return;
    };
    match content {
        Content::Text(text) if excerpts => set_excerpt(state, &text),
        Content::Text(_) => {}
        Content::Blocks(blocks) => {
            for block in blocks {
                match block.kind.as_deref() {
                    Some("text") if excerpts => {
                        if let Some(text) = block.text {
                            set_excerpt(state, &text);
                        }
                    }
                    Some("tool_use") => {
                        if let Some(name) =
                            block.name.as_deref().and_then(|n| clean(n, MAX_TOOL_CHARS))
                        {
                            state.tool = Some(name);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Secrets are masked on the whole text, before it is cut, so a cut cannot hide a key's start.
fn set_excerpt(state: &mut AgentState, text: &str) {
    if let Some(excerpt) = clean(&redact(text), MAX_EXCERPT_CHARS) {
        state.excerpt = Some(excerpt);
    }
}

/// Trimmed, without control characters except newline and tab and without invisible format
/// characters ([`crate::text::is_invisible`]: bidi overrides, zero-width characters), at most
/// `max` characters (the last one an ellipsis when cut). `None` when nothing is left.
pub fn clean(text: &str, max: usize) -> Option<String> {
    let text = text.trim();
    let mut out = String::new();
    let mut count = 0;
    let mut chars = text
        .chars()
        .filter(|c| (!c.is_control() || *c == '\n' || *c == '\t') && !crate::text::is_invisible(*c))
        .peekable();
    while let Some(c) = chars.next() {
        if count + 1 == max && chars.peek().is_some() {
            out.push('\u{2026}');
            break;
        }
        out.push(c);
        count += 1;
    }
    (!out.is_empty()).then_some(out)
}

fn read_meta(path: &Path) -> Option<Meta> {
    let file = open_shared(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_META_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_META_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir())
}

fn is_real_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file())
}

fn millis(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;
    use crate::test_support::TempDir;

    const SESSION: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";
    const AGENT: &str = "a0123456789abcdef";

    struct Fixture {
        dir: TempDir,
        cwd: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir = TempDir::new(&format!("subagents-{name}"));
            let cwd = dir.path().join("work space");
            fs::create_dir_all(&cwd).unwrap();
            Self { dir, cwd }
        }

        fn config(&self) -> PathBuf {
            self.dir.path().join("config")
        }

        fn target(&self) -> Target {
            Target {
                kind: super::super::SubAgentKind::Claude,
                session: SESSION.into(),
                cwd: self.cwd.clone(),
                env: [(
                    CONFIG_DIR_VAR.to_owned(),
                    self.config().display().to_string(),
                )]
                .into(),
                home: self.dir.path().join("home"),
            }
        }

        /// Project folder under a name that is NOT the encoded cwd, so the search is exercised.
        fn project(&self, name: &str) -> PathBuf {
            let project = self.config().join("projects").join(name);
            fs::create_dir_all(&project).unwrap();
            project
        }

        fn subagents(&self, project: &Path) -> PathBuf {
            let dir = project.join(SESSION).join("subagents");
            fs::create_dir_all(&dir).unwrap();
            dir
        }
    }

    fn append(path: &Path, data: &str) {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        file.write_all(data.as_bytes()).unwrap();
    }

    fn assistant(blocks: &str) -> String {
        format!(
            "{{\"type\":\"assistant\",\"isSidechain\":true,\"agentId\":\"{AGENT}\",\"message\":{{\"role\":\"assistant\",\"content\":[{blocks}]}}}}\n"
        )
    }

    #[test]
    fn whitelist_only_from_synthetic_transcript() {
        let fx = Fixture::new("whitelist");
        fs::write(fx.project("other").join(format!("{SESSION}.jsonl")), "").unwrap();
        let sub = fx.subagents(&fx.config().join("projects").join("other"));
        let transcript = sub.join(format!("agent-{AGENT}.jsonl"));
        append(
            &transcript,
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"PROMPT-MARKER\"}}\n",
        );
        append(&transcript, &assistant(
            "{\"type\":\"text\",\"text\":\"Looking at the files\"},{\"type\":\"tool_use\",\"name\":\"Grep\",\"input\":{\"pattern\":\"INPUT-MARKER\"}}",
        ));
        append(&transcript, "{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"content\":\"RESULT-MARKER\"}]}}\n");
        append(
            &transcript,
            "{\"type\":\"attachment\",\"text\":\"ATTACHMENT-MARKER\"}\n",
        );
        append(
            &transcript,
            "{\"type\":\"api-request\",\"body\":\"API-MARKER\"}\n",
        );
        append(
            &transcript,
            "not json at all\n{\"type\":\"assistant\",\"message\":{\"content\":\"broken\n",
        );
        fs::write(
            sub.join(format!("agent-{AGENT}.meta.json")),
            "{\"agentType\":\"Explore\",\"isBuiltIn\":true,\"description\":\"Find the parser\",\"toolUseId\":\"t\",\"spawnDepth\":1,\"requestShape\":\"SHAPE-MARKER\"}",
        )
        .unwrap();
        // Files that are not agent files are ignored.
        fs::write(sub.join("notes.jsonl"), "").unwrap();

        let mut source = ClaudeSource::new(&fx.target(), true);
        let snapshot = source.poll(SystemTime::now());
        assert!(snapshot.located);
        assert_eq!(snapshot.agents.len(), 1, "{snapshot:?}");
        let agent = &snapshot.agents[0];
        assert_eq!(agent.id, AGENT);
        assert_eq!(agent.description.as_deref(), Some("Find the parser"));
        assert_eq!(agent.agent_type.as_deref(), Some("Explore"));
        assert_eq!(agent.spawn_depth, Some(1));
        assert_eq!(agent.status, AgentStatus::Running);
        assert_eq!(agent.tool.as_deref(), Some("Grep"));
        assert_eq!(agent.excerpt.as_deref(), Some("Looking at the files"));
        assert!(agent.started_at.is_some() && agent.last_activity.is_some());
        let json = serde_json::to_string(&snapshot).unwrap();
        for marker in [
            "PROMPT",
            "INPUT",
            "RESULT",
            "ATTACHMENT",
            "API",
            "SHAPE",
            "broken",
        ] {
            assert!(!json.contains(marker), "{marker} leaked: {json}");
        }
    }

    #[test]
    fn finished_and_stale_status() {
        let fx = Fixture::new("status");
        let project = fx.project(&encode_project(&fx.cwd));
        let sub = fx.subagents(&project);
        append(
            &sub.join("agent-a1.jsonl"),
            &assistant("{\"type\":\"text\",\"text\":\"one\"}"),
        );
        append(
            &sub.join("agent-a2.jsonl"),
            &assistant("{\"type\":\"text\",\"text\":\"two\"}"),
        );
        fs::write(sub.join("agent-a2.meta.json"), "{\"finished\":true}").unwrap();
        let mut source = ClaudeSource::new(&fx.target(), true);
        let now = SystemTime::now();
        let status = |s: &Snapshot, id: &str| s.agents.iter().find(|a| a.id == id).unwrap().status;
        let snapshot = source.poll(now);
        assert_eq!(status(&snapshot, "a1"), AgentStatus::Running);
        assert_eq!(status(&snapshot, "a2"), AgentStatus::Finished);
        let later = source.poll(now + STALE_AFTER + Duration::from_secs(2));
        assert_eq!(status(&later, "a1"), AgentStatus::Stale);
        assert_eq!(status(&later, "a2"), AgentStatus::Finished);
        // A deleted transcript and meta drop the agent.
        fs::remove_file(sub.join("agent-a1.jsonl")).unwrap();
        let after = source.poll(now);
        assert_eq!(after.agents.len(), 1);
    }

    #[test]
    fn finished_transcripts_are_read_again_only_when_they_change() {
        let fx = Fixture::new("settled");
        let sub = fx.subagents(&fx.project("p"));
        let path = sub.join("agent-a1.jsonl");
        append(&path, &assistant("{\"type\":\"text\",\"text\":\"one\"}"));
        fs::write(sub.join("agent-a1.meta.json"), "{\"finished\":true}").unwrap();
        let mut source = ClaudeSource::new(&fx.target(), true);
        assert_eq!(
            source.poll(SystemTime::now()).agents[0].excerpt.as_deref(),
            Some("one")
        );
        // The stamp of the read is kept; an unchanged stamp skips the next open. The skip itself
        // has no observable effect besides fewer system calls, so the test guards the risky part:
        // a late write to a finished transcript is still picked up.
        let size = fs::metadata(&path).unwrap().len();
        assert_eq!(
            source.agents["a1"].transcript_stamp.map(|(len, _)| len),
            Some(size)
        );
        // A late write changes the stamp and is picked up.
        append(&path, &assistant("{\"type\":\"text\",\"text\":\"two\"}"));
        assert_eq!(
            source.poll(SystemTime::now()).agents[0].excerpt.as_deref(),
            Some("two")
        );
    }

    #[test]
    fn not_located_until_the_transcript_appears() {
        let fx = Fixture::new("locate");
        let mut source = ClaudeSource::new(&fx.target(), true);
        assert_eq!(source.poll(SystemTime::now()), Snapshot::default());
        // The encoded folder is found at once on the next search.
        let project = fx.project(&encode_project(&fx.cwd));
        fs::write(project.join(format!("{SESSION}.jsonl")), "").unwrap();
        source.next_locate = None;
        let snapshot = source.poll(SystemTime::now());
        assert!(snapshot.located);
        assert!(snapshot.agents.is_empty());
    }

    #[test]
    fn excerpt_is_short_clean_and_latest() {
        let fx = Fixture::new("excerpt");
        let sub = fx.subagents(&fx.project("p"));
        let path = sub.join("agent-a1.jsonl");
        append(&path, &assistant("{\"type\":\"text\",\"text\":\"first\"}"));
        let long = "z".repeat(5000);
        append(&path, &assistant(&format!(
            "{{\"type\":\"thinking\",\"thinking\":\"hidden\"}},{{\"type\":\"text\",\"text\":\"  \\u001b[31m{long}\"}}"
        )));
        let mut source = ClaudeSource::new(&fx.target(), true);
        let excerpt = source.poll(SystemTime::now()).agents[0]
            .excerpt
            .clone()
            .unwrap();
        assert_eq!(excerpt.chars().count(), MAX_EXCERPT_CHARS);
        assert!(excerpt.starts_with("[31mzzz"));
        assert!(excerpt.ends_with('\u{2026}'));
        // String content (older format) counts as text too.
        append(
            &path,
            "{\"type\":\"assistant\",\"message\":{\"content\":\"plain\"}}\n",
        );
        assert_eq!(
            source.poll(SystemTime::now()).agents[0].excerpt.as_deref(),
            Some("plain")
        );
    }

    #[test]
    fn excerpt_is_redacted_and_can_be_switched_off() {
        let fx = Fixture::new("redact");
        let sub = fx.subagents(&fx.project("p"));
        let path = sub.join("agent-a1.jsonl");
        append(&path, &assistant(
            "{\"type\":\"text\",\"text\":\"Found API_KEY=FAKEVALUE0123456789 in .env and \\u202eevil\\u200b\"}",
        ));
        fs::write(
            sub.join("agent-a1.meta.json"),
            "{\"description\":\"Check token=FAKEVALUE0123456789\"}",
        )
        .unwrap();
        let mut source = ClaudeSource::new(&fx.target(), true);
        let agent = source.poll(SystemTime::now()).agents.remove(0);
        assert_eq!(
            agent.excerpt.as_deref(),
            Some("Found API_KEY=[redacted] in .env and evil")
        );
        assert_eq!(agent.description.as_deref(), Some("Check token=[redacted]"));
        let mut quiet = ClaudeSource::new(&fx.target(), false);
        let agent = quiet.poll(SystemTime::now()).agents.remove(0);
        assert_eq!(agent.excerpt, None);
        assert!(agent.description.is_some());
    }

    #[test]
    fn locate_backs_off_and_searches_all_folders_only_on_change() {
        let fx = Fixture::new("backoff");
        let mut source = ClaudeSource::new(&fx.target(), true);
        let mut delays = Vec::new();
        for _ in 0..6 {
            source.next_locate = None;
            assert!(!source.poll(SystemTime::now()).located);
            delays.push(source.next_locate.unwrap() - Instant::now());
        }
        let seconds: Vec<u64> = delays
            .iter()
            .map(|d| (d.as_millis() as u64 + 500) / 1000)
            .collect();
        assert_eq!(seconds, [2, 4, 8, 16, 30, 30]);

        // A project folder under another name while the projects mtime is unchanged: only the
        // guess is checked, until the longest delay forces a full search.
        let mut source = ClaudeSource::new(&fx.target(), true);
        fs::write(fx.project("other").join(format!("{SESSION}.jsonl")), "").unwrap();
        let mtime = fs::metadata(fx.config().join("projects"))
            .and_then(|m| m.modified())
            .ok();
        source.scanned = Some(mtime);
        assert!(!source.poll(SystemTime::now()).located);
        source.next_locate = None;
        source.locate_delay = MAX_LOCATE_DELAY;
        assert!(source.poll(SystemTime::now()).located);
        // A changed mtime searches all folders at once.
        let mut source = ClaudeSource::new(&fx.target(), true);
        source.scanned = Some(None);
        assert!(source.poll(SystemTime::now()).located);
    }

    #[test]
    fn ids_and_file_names_are_strict() {
        assert!(is_agent_id(AGENT));
        for bad in ["", "..", "a/b", "a\\b", "a.b", "a b", &"x".repeat(65)] {
            assert!(!is_agent_id(bad), "{bad:?}");
        }
        assert_eq!(
            agent_id_of("agent-a1.jsonl"),
            Some(("a1", FileKind::Transcript))
        );
        assert_eq!(
            agent_id_of("agent-a1.meta.json"),
            Some(("a1", FileKind::Meta))
        );
        for bad in [
            "agent-.jsonl",
            "agent-a.b.jsonl",
            "x-a1.jsonl",
            "agent-a1.json",
            "agent-a1.jsonl.bak",
        ] {
            assert_eq!(agent_id_of(bad), None, "{bad}");
        }
        assert_eq!(
            encode_project(Path::new(r"E:\Work\Local\hangar")),
            "E--Work-Local-hangar"
        );
        assert_eq!(encode_project(Path::new("/home/a b/x.y")), "-home-a-b-x-y");
    }

    #[test]
    fn config_dir_follows_profile_then_default() {
        let fx = Fixture::new("config");
        let mut target = fx.target();
        assert_eq!(config_dir(&target), fx.config());
        target.env.insert(CONFIG_DIR_VAR.into(), "rel".into());
        assert_eq!(config_dir(&target), fx.cwd.join("rel"));
        target.env.clear();
        if std::env::var_os(CONFIG_DIR_VAR).is_none() {
            assert_eq!(config_dir(&target), target.home.join(".claude"));
        }
    }

    /// A symbolic link (Unix) or junction (Windows, no admin rights needed) to `target`.
    fn link_dir(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        {
            let status = std::process::Command::new("cmd")
                .args(["/d", "/c", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .unwrap()
                .status;
            assert!(status.success());
        }
    }

    #[test]
    fn links_out_of_the_projects_folder_are_not_followed() {
        let fx = Fixture::new("links");
        let outside = fx.dir.path().join("outside");
        let outside_agents = outside.join(SESSION).join("subagents");
        fs::create_dir_all(&outside_agents).unwrap();
        append(
            &outside_agents.join("agent-a1.jsonl"),
            &assistant("{\"type\":\"text\",\"text\":\"OUTSIDE-MARKER\"}"),
        );
        fs::write(outside.join(format!("{SESSION}.jsonl")), "").unwrap();
        let projects = fx.config().join("projects");
        fs::create_dir_all(&projects).unwrap();
        // The project folder itself is a link, under the guessed name and under another one.
        link_dir(&outside, &projects.join(encode_project(&fx.cwd)));
        link_dir(&outside, &projects.join("other"));
        let mut source = ClaudeSource::new(&fx.target(), true);
        assert!(!source.poll(SystemTime::now()).located);
        // A real project folder whose session folder links elsewhere: found, nothing read.
        let project = fx.project("real");
        fs::write(project.join(format!("{SESSION}.jsonl")), "").unwrap();
        link_dir(&outside.join(SESSION), &project.join(SESSION));
        source.next_locate = None;
        source.scanned = None;
        let snapshot = source.poll(SystemTime::now());
        assert!(snapshot.located);
        assert!(snapshot.agents.is_empty(), "{snapshot:?}");
    }
}

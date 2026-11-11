//! The sub-agent watcher against real files: the transcript folder appears after the watch
//! started, a sub-agent writes, finishes, and the watch stops. Synthetic fixtures only.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use hangar_lib::subagents::{
    self, claude, AgentStatus, LogBatch, LogEntry, Snapshot, SubAgentEvent, SubAgentKind, Target,
    Watchers,
};

const SESSION: &str = "7c9e6679-7425-40de-944b-e07fc1f90ae7";
const TIMEOUT: Duration = Duration::from_secs(15);

/// Under `<repo>/tests/.tmp/`, never in the system temp folder.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests")
            .join(".tmp")
            .join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
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

/// Waits for a snapshot that satisfies `done`.
fn wait_for(rx: &Receiver<SubAgentEvent>, done: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(SubAgentEvent::Snapshot(snapshot)) if done(&snapshot) => return snapshot,
            Ok(_) => {}
            Err(error) => panic!("expected snapshot did not arrive: {error:?}"),
        }
    }
}

#[test]
fn watcher_follows_a_session_from_first_prompt_to_finished_sub_agent() {
    let scratch = Scratch::new("subagents-watch");
    let cwd = scratch.0.join("project dir");
    fs::create_dir_all(&cwd).unwrap();
    let config = scratch.0.join("claude config");
    let target = Target {
        kind: SubAgentKind::Claude,
        session: SESSION.into(),
        cwd: cwd.clone(),
        env: [(
            claude::CONFIG_DIR_VAR.to_owned(),
            config.display().to_string(),
        )]
        .into(),
        home: scratch.0.join("home"),
    };
    let watchers = Watchers::default();
    let (tx, rx) = channel();
    let sent = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&sent);
    watchers
        .start(1, subagents::source_for(&target, true), move |event| {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            tx.send(event).is_ok()
        })
        .unwrap();
    let started = Instant::now();

    // Nothing exists yet: the first prompt has not been sent.
    let first = wait_for(&rx, |_| true);
    assert_eq!(first, Snapshot::default());

    // Claude writes the transcript under the encoded folder name, then the first sub-agent.
    let project = config.join("projects").join(claude::encode_project(&cwd));
    fs::create_dir_all(&project).unwrap();
    append(
        &project.join(format!("{SESSION}.jsonl")),
        "{\"type\":\"user\"}\n",
    );
    wait_for(&rx, |s| s.located);
    let folder = project.join(SESSION).join("subagents");
    fs::create_dir_all(&folder).unwrap();
    let transcript = folder.join("agent-a00000000000000aa.jsonl");
    append(
        &transcript,
        "{\"type\":\"user\",\"isSidechain\":true,\"message\":{\"content\":\"PROMPT-MARKER\"}}\n",
    );
    // A record that arrives in two writes.
    append(
        &transcript,
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"name\":\"Read\",",
    );
    fs::write(
        folder.join("agent-a00000000000000aa.meta.json"),
        "{\"agentType\":\"general-purpose\",\"description\":\"Check the tests\",\"spawnDepth\":1}",
    )
    .unwrap();
    let running = wait_for(&rx, |s| {
        s.agents.first().is_some_and(|a| a.description.is_some())
    });
    assert_eq!(running.agents[0].status, AgentStatus::Running);
    assert_eq!(running.agents[0].tool, None);
    append(
        &transcript,
        "\"input\":{\"file_path\":\"src/INPUT/lib.rs\",\"limit\":5}},{\"type\":\"text\",\"text\":\"Reading now\"}]}}\n",
    );
    let working = wait_for(&rx, |s| s.agents.first().is_some_and(|a| a.tool.is_some()));
    let agent = &working.agents[0];
    assert_eq!(agent.id, "a00000000000000aa");
    assert_eq!(agent.tool.as_deref(), Some("Read"));
    // Of the input only the file name, the activity's target (excerpts are on here).
    assert_eq!(agent.target.as_deref(), Some("lib.rs"));
    assert_eq!(agent.excerpt.as_deref(), Some("Reading now"));
    assert_eq!(agent.agent_type.as_deref(), Some("general-purpose"));

    fs::write(
        folder.join("agent-a00000000000000aa.meta.json"),
        "{\"agentType\":\"general-purpose\",\"description\":\"Check the tests\",\"spawnDepth\":1,\"finished\":true}",
    )
    .unwrap();
    let finished = wait_for(&rx, |s| {
        s.agents
            .first()
            .is_some_and(|a| a.status == AgentStatus::Finished)
    });
    let json = serde_json::to_string(&SubAgentEvent::Snapshot(finished)).unwrap();
    assert!(
        !json.contains("MARKER") && !json.contains("INPUT"),
        "{json}"
    );

    // Rate limit: one event per poll at most.
    let elapsed = started.elapsed();
    let polls = elapsed.as_millis() / subagents::POLL_INTERVAL.as_millis() + 2;
    assert!(
        sent.load(std::sync::atomic::Ordering::Relaxed) as u128 <= polls,
        "too many events"
    );

    assert!(watchers.stop(1));
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Err(RecvTimeoutError::Disconnected) => break,
            _ if Instant::now() > deadline => panic!("watch did not stop"),
            _ => {}
        }
    }
}

/// Waits for a log batch that satisfies `done`.
fn wait_for_log(rx: &Receiver<SubAgentEvent>, done: impl Fn(&LogBatch) -> bool) -> LogBatch {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(SubAgentEvent::Log(batch)) if done(&batch) => return batch,
            Ok(_) => {}
            Err(error) => panic!("expected log batch did not arrive: {error:?}"),
        }
    }
}

#[test]
fn focused_sub_agent_streams_its_log_as_it_grows() {
    let scratch = Scratch::new("subagents-log");
    let cwd = scratch.0.join("project dir");
    fs::create_dir_all(&cwd).unwrap();
    let config = scratch.0.join("claude config");
    let project = config.join("projects").join(claude::encode_project(&cwd));
    let folder = project.join(SESSION).join("subagents");
    fs::create_dir_all(&folder).unwrap();
    append(
        &project.join(format!("{SESSION}.jsonl")),
        "{\"type\":\"user\"}\n",
    );
    let transcript = folder.join("agent-a00000000000000bb.jsonl");
    append(
        &transcript,
        "{\"type\":\"user\",\"message\":{\"content\":\"Count the tests\"}}\n",
    );
    let target = Target {
        kind: SubAgentKind::Claude,
        session: SESSION.into(),
        cwd,
        env: [(
            claude::CONFIG_DIR_VAR.to_owned(),
            config.display().to_string(),
        )]
        .into(),
        home: scratch.0.join("home"),
    };
    let watchers = Watchers::default();
    let (tx, rx) = channel();
    watchers
        .start(2, subagents::source_for(&target, true), move |event| {
            tx.send(event).is_ok()
        })
        .unwrap();
    wait_for(&rx, |s| s.agents.len() == 1);

    assert!(watchers.focus(2, Some("a00000000000000bb".into())));
    let first = wait_for_log(&rx, |b| b.reset);
    assert_eq!(first.agent_id, "a00000000000000bb");
    assert_eq!(first.entries, [LogEntry::prompt("Count the tests".into())]);
    append(
        &transcript,
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"name\":\"Glob\",\"input\":{\"pattern\":\"**/*.rs\"}}]}}\n",
    );
    let next = wait_for_log(&rx, |b| !b.entries.is_empty());
    assert!(!next.reset);
    assert_eq!(
        next.entries,
        [LogEntry::Tool {
            name: "Glob".into(),
            detail: Some("**/*.rs".into()),
        }]
    );
    assert!(watchers.focus(2, None));
    watchers.stop_all();
}

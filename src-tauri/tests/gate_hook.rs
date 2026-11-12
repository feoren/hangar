//! The helper `hangar gate-hook` end to end: the real Hangar executable, started the way each
//! agent CLI starts its hook (Codex through cmd.exe or `sh -c` with the command from its `-c`
//! value, Gemini CLI through PowerShell or `sh -c` with the command from its settings file,
//! Copilot CLI as a program with arguments), talks to a running gate endpoint and answers in the
//! agent's format. No agent CLI is needed; what the agents themselves do with the answer is
//! documented, not measured here (docs/wiki/pages/syntheses/agent-gate-adapters.md).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use hangar_lib::agent_catalog::{self, Agent};
use hangar_lib::claims::{ChangeKind, Claim, LineRange, WorktreeClaims};
use hangar_lib::gate::{Answer, GateSnapshot};
use hangar_lib::gate_service::GateService;
use hangar_lib::hook_recipes::{self, HELPER_ENV};
use hangar_lib::manager::{GateLaunch, GatePlan, LaunchGate};

const HELPER: &str = env!("CARGO_BIN_EXE_hangar");

/// How long a helper run may take before the test kills it and fails.
const RUN_LIMIT: Duration = Duration::from_secs(30);

/// Text that only a build with the `gate-hook` helper contains (`HelperError::Usage`). A stale
/// `hangar.exe` from a shared target dir lacks it, and starting that would launch the whole app
/// against the user's real workspace.
const HELPER_MARKER: &str = "usage: hangar gate-hook <gemini before-tool";

/// Fails unless the executable at `path` contains the helper marker; spawns nothing.
fn require_helper_marker(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let needle = HELPER_MARKER.as_bytes();
    let mut from = 0;
    while let Some(i) = bytes[from..].iter().position(|b| *b == needle[0]) {
        let at = from + i;
        if bytes[at..].starts_with(needle) {
            return Ok(());
        }
        from = at + 1;
    }
    Err(format!(
        "stale hangar binary without the gate helper (shared target dir?): {}",
        path.display()
    ))
}

/// Checks the real binary once per test process, before anything is spawned.
fn guard_helper() {
    static CHECKED: OnceLock<Result<(), String>> = OnceLock::new();
    if let Err(message) = CHECKED.get_or_init(|| require_helper_marker(Path::new(HELPER))) {
        panic!("{message}");
    }
}

/// A scratch home under `tests/.tmp` for spawned helpers, so that even a binary that turned out
/// to be the full app finds an empty profile. Hangar has no data-dir override variable; these
/// are the variables the platform's folder lookups honor on Unix and the common ones on Windows
/// (the Windows known-folder API itself ignores them, so the marker check is the real guard).
fn sandbox_env(command: &mut Command) {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    let home = HOME.get_or_init(|| {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests")
            .join(".tmp")
            .join("gate-hook-sandbox-home");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    });
    command
        .env("APPDATA", home.join("roaming"))
        .env("LOCALAPPDATA", home.join("local"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("HOME", home)
        .env("USERPROFILE", home);
}

/// Kills `child` and everything it started (`cmd.exe`/`powershell.exe` run the helper below them).
fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Test folder under `<repo>/tests/.tmp/`, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let dir = manifest
            .parent()
            .unwrap()
            .join("tests")
            .join(".tmp")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Fixture {
    service: GateService,
    events: Receiver<GateSnapshot>,
    root: PathBuf,
    _settings: TempDir,
}

/// Session 1 works in `root`; session 2, in another worktree of the same repository, changes
/// `src/shared.rs`.
fn fixture(name: &str) -> Fixture {
    guard_helper();
    let root = std::env::current_dir().unwrap().join("repo-under-test");
    let worktrees = Arc::new(vec![
        WorktreeClaims {
            repo: "repo".into(),
            root_key: hangar_lib::git::path_key(&root),
            name: "repo".into(),
            root_commit: None,
            base: Some("b".into()),
            sessions: vec![1],
            claims: Vec::new(),
            truncated: false,
        },
        WorktreeClaims {
            repo: "repo".into(),
            root_key: "elsewhere/feature".into(),
            name: "feature".into(),
            root_commit: None,
            base: Some("b".into()),
            sessions: vec![2],
            claims: vec![Claim {
                path: "src/shared.rs".into(),
                kind: ChangeKind::Modified,
                whole_file: false,
                ranges: vec![LineRange { start: 1, end: 3 }],
                area: Some("src".into()),
            }],
            truncated: false,
        },
    ]);
    let (tx, events) = channel();
    let tx = Mutex::new(tx);
    let settings = TempDir::new(name);
    let service = GateService::new(
        settings.0.clone(),
        move || Arc::clone(&worktrees),
        || [(2, "Feature session".to_owned())].into_iter().collect(),
        move |snapshot: &GateSnapshot| {
            let _ = tx.lock().unwrap().send(snapshot.clone());
        },
    );
    service.configure(true);
    service.set_helper(PathBuf::from(HELPER));
    Fixture {
        service,
        events,
        root,
        _settings: settings,
    }
}

/// The catalog's entry with its recipe switched on: Gemini CLI and Copilot CLI wait for a live
/// measurement before Hangar offers them, but their recipes are tested here already.
fn catalog_agent(id: &str) -> Agent {
    let mut entry = agent_catalog::builtin()
        .unwrap()
        .into_iter()
        .find(|a| a.id == id)
        .unwrap();
    entry.capabilities.measured = true;
    Agent::from_catalog(entry)
}

fn launched(plan: GatePlan) -> GateLaunch {
    match plan {
        GatePlan::Launch(launch) => launch,
        GatePlan::Skip => panic!("skipped"),
        GatePlan::Unavailable(status) => panic!("unavailable: {status:?}"),
    }
}

fn gate_env(launch: &GateLaunch) -> String {
    launch
        .env
        .iter()
        .find(|(name, _)| name == HELPER_ENV)
        .map(|(_, value)| value.to_string())
        .unwrap()
}

/// Runs `command` with `input` on stdin and the gate's variable set, like the agent would.
/// Fails after `RUN_LIMIT` and kills the whole process tree.
fn run(mut command: Command, gate: &str, input: &str) -> Output {
    guard_helper();
    sandbox_env(&mut command);
    command
        .env(HELPER_ENV, gate)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_owned();
    // Threads, so a child that never reads or closes its pipes cannot block the poll below.
    std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let stdout = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let stderr = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + RUN_LIMIT;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            kill_tree(&mut child);
            panic!("the hook command did not finish within {RUN_LIMIT:?}; killed it");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Output {
        status,
        stdout: stdout.join().unwrap(),
        stderr: stderr.join().unwrap(),
    }
}

/// The command line a hook runs through the shell, the way Codex (`cmd.exe /C "<command>"`) and
/// Gemini CLI (`powershell -NoProfile -NonInteractive -Command <command>`) start it on Windows,
/// `sh -c` elsewhere.
#[cfg(windows)]
fn through_shell(command_line: &str, powershell: bool) -> Command {
    use std::os::windows::process::CommandExt;
    if powershell {
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command", command_line]);
        command
    } else {
        let mut command = Command::new("cmd.exe");
        command.raw_arg(format!("/D /S /C \"{command_line}\""));
        command
    }
}

#[cfg(not(windows))]
fn through_shell(command_line: &str, _powershell: bool) -> Command {
    let mut command = Command::new("sh");
    command.args(["-c", command_line]);
    command
}

/// Answers the next held edit with `answer` once it shows up in the window's snapshot.
fn answer_next(fixture: &Fixture, answer: Answer) -> std::thread::JoinHandle<()> {
    let service = fixture.service.clone();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let snapshot = loop {
        let snapshot = fixture
            .events
            .recv_timeout(Duration::from_secs(30))
            .unwrap();
        if !snapshot.pending.is_empty() {
            break snapshot;
        }
        assert!(std::time::Instant::now() < deadline, "no held edit");
    };
    let id = snapshot.pending[0].id;
    std::thread::spawn(move || assert!(service.answer(id, answer)))
}

fn patch(files: &[(&str, &str)]) -> String {
    let mut text = String::from("*** Begin Patch\n");
    for (marker, path) in files {
        text.push_str(&format!("*** {marker}: {path}\n+x\n"));
    }
    text.push_str("*** End Patch\n");
    text
}

#[test]
fn codex_hook_command_round_trips_through_the_shell() {
    let fixture = fixture("gate-hook-codex");
    let launch = launched(fixture.service.prepare(
        1,
        &catalog_agent("codex"),
        Path::new("codex.exe"),
        &[],
    ));
    // The command exactly as Codex reads it from the `-c` value.
    let value = &launch.prepend[2];
    let (_, toml_value) = value.split_once('=').unwrap();
    let parsed: toml::Table = toml::from_str(&format!("v = {toml_value}")).unwrap();
    let command_line = parsed["v"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_owned();
    let gate = gate_env(&launch);
    let input = |text: &str| {
        serde_json::json!({
            "session_id": "c", "cwd": fixture.root, "hook_event_name": "PreToolUse",
            "tool_name": "apply_patch", "tool_input": { "command": text },
        })
        .to_string()
    };

    // A patch of files nobody else changes goes ahead at once, silently.
    let output = run(
        through_shell(&command_line, false),
        &gate,
        &input(&patch(&[
            ("Add File", "src/new.rs"),
            ("Update File", "src/own.rs"),
        ])),
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");

    // One file of several, named with the other separator, collides: the person denies.
    let other_separator = if cfg!(windows) {
        "src\\shared.rs"
    } else {
        "src/shared.rs"
    };
    let text = patch(&[("Update File", "src/own.rs"), ("Update File", "src/old.rs")]).replace(
        "+x\n*** End",
        &format!("*** Move to: {other_separator}\n+x\n*** End"),
    );
    let answering = {
        let gate = gate.clone();
        let input = input(&text);
        let command_line = command_line.clone();
        std::thread::spawn(move || run(through_shell(&command_line, false), &gate, &input))
    };
    answer_next(&fixture, Answer::Deny).join().unwrap();
    let output = answering.join().unwrap();
    // Exit 0 with the JSON deny: the answer Codex 0.167.2 was measured to block on.
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let decision = &stdout["hookSpecificOutput"];
    assert_eq!(decision["permissionDecision"], "deny");
    let reason = decision["permissionDecisionReason"].as_str().unwrap();
    assert!(
        reason.contains("src/shared.rs") && reason.contains("Feature session"),
        "{reason}"
    );
    assert!(output.stderr.is_empty(), "{output:?}");

    // Once the session ended, its token is dead and the helper lets the edit through.
    fixture.service.ended(1);
    let output = run(through_shell(&command_line, false), &gate, &input(&text));
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("401"),
        "{output:?}"
    );
}

#[test]
fn gemini_hook_command_round_trips_through_the_shell() {
    let fixture = fixture("gate-hook-gemini");
    let launch = launched(fixture.service.prepare(
        1,
        &catalog_agent("gemini"),
        Path::new("gemini.exe"),
        &[],
    ));
    let file = launch
        .env
        .iter()
        .find(|(name, _)| name == hook_recipes::GEMINI_SETTINGS_ENV)
        .map(|(_, value)| PathBuf::from(value.as_str()))
        .unwrap();
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    let command_line = settings["hooks"]["BeforeTool"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_owned();
    let gate = gate_env(&launch);
    let input = serde_json::json!({
        "session_id": "g", "cwd": fixture.root, "hook_event_name": "BeforeTool",
        "timestamp": "2026-11-12T00:00:00Z", "tool_name": "replace",
        "tool_input": { "file_path": "src/shared.rs", "old_string": "a", "new_string": "b",
                        "instruction": "x" },
    })
    .to_string();
    let answering = {
        let gate = gate.clone();
        std::thread::spawn(move || run(through_shell(&command_line, true), &gate, &input))
    };
    answer_next(&fixture, Answer::Deny).join().unwrap();
    let output = answering.join().unwrap();
    // Exit 0 whichever build PowerShell starts; the JSON on stdout carries the deny.
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|e| panic!("{e}: {output:?}"));
    assert_eq!(stdout["decision"], "deny");
    assert!(stdout["reason"].as_str().unwrap().contains("src/shared.rs"));
}

#[test]
fn copilot_hook_runs_the_helper_as_a_program() {
    let fixture = fixture("gate-hook-copilot");
    let launch = launched(fixture.service.prepare(
        1,
        &catalog_agent("copilot"),
        Path::new("copilot.exe"),
        &[],
    ));
    let plugin = PathBuf::from(&launch.prepend[1]);
    let hooks: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(plugin.join("hooks.json")).unwrap()).unwrap();
    let entry = &hooks["hooks"]["preToolUse"][0];
    let mut command = Command::new(entry["exec"].as_str().unwrap());
    command.args(
        entry["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap()),
    );
    let gate = gate_env(&launch);
    // toolArgs as a JSON string, as Copilot may send it.
    let input = serde_json::json!({
        "sessionId": "p", "timestamp": 1, "cwd": fixture.root, "toolName": "edit",
        "toolArgs": serde_json::json!({ "path": fixture.root.join("src").join("shared.rs") })
            .to_string(),
    })
    .to_string();
    let answering = std::thread::spawn(move || run(command, &gate, &input));
    answer_next(&fixture, Answer::AllowOnce).join().unwrap();
    let output = answering.join().unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    // Removed with the session's end.
    fixture.service.ended(1);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while plugin.exists() {
        assert!(std::time::Instant::now() < deadline, "plugin folder stayed");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn droid_hook_command_round_trips_through_the_shell() {
    let fixture = fixture("gate-hook-droid");
    let launch = launched(fixture.service.prepare(
        1,
        &catalog_agent("droid"),
        Path::new("droid.exe"),
        &[],
    ));
    assert_eq!(launch.prepend[0], "--settings");
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&launch.prepend[1]).unwrap()).unwrap();
    let command_line = settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_owned();
    let gate = gate_env(&launch);
    let input = serde_json::json!({
        "session_id": "d", "transcript_path": "t", "cwd": fixture.root,
        "permission_mode": "default", "hook_event_name": "PreToolUse",
        "tool_name": "Edit", "tool_input": { "file_path": "src/shared.rs" },
    })
    .to_string();
    let answering =
        std::thread::spawn(move || run(through_shell(&command_line, false), &gate, &input));
    answer_next(&fixture, Answer::Deny).join().unwrap();
    let output = answering.join().unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(stdout["hookSpecificOutput"]["permissionDecision"], "deny");
}

/// Loads a gate extension or plugin the way the agent does (an ES module whose handler gets the
/// tool call) and prints what it returned; the agent itself is not needed.
const PLUGIN_DRIVER: &str = r#"
import { pathToFileURL } from "node:url";
const [kind, file, toolName, argsJson, cwd] = process.argv.slice(2);
const mod = await import(pathToFileURL(file).href);
const args = JSON.parse(argsJson);
if (kind === "pi") {
  let handler;
  mod.default({ on: (name, h) => { if (name === "tool_call") handler = h; } });
  const result = await handler({ toolName, input: args }, { cwd, hasUI: false });
  console.log(JSON.stringify({ result: result ?? null }));
} else {
  const hooks = await mod.HangarGate({ directory: cwd });
  try {
    await hooks["tool.execute.before"]({ tool: toolName }, { args });
    console.log(JSON.stringify({ result: null }));
  } catch (error) {
    console.log(JSON.stringify({ thrown: String(error.message) }));
  }
}
"#;

/// Whether Node can run the plugin tests. A machine without Node skips them; CI installs Node in
/// the same job, so there a missing Node fails instead of passing unnoticed.
fn node_available() -> bool {
    let found = Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    assert!(
        found || std::env::var_os("CI").is_none(),
        "node not found, but CI must run the plugin tests"
    );
    found
}

/// Runs the plugin text `source` under Node as an ES module with one tool call.
fn run_plugin(
    dir: &TempDir,
    kind: &str,
    source: &Path,
    tool: &str,
    args: serde_json::Value,
    cwd: &Path,
    gate: &str,
) -> Output {
    let module = dir.0.join(format!("{kind}-gate.mjs"));
    std::fs::copy(source, &module).unwrap();
    let driver = dir.0.join("driver.mjs");
    std::fs::write(&driver, PLUGIN_DRIVER).unwrap();
    let mut command = Command::new("node");
    command
        .arg(&driver)
        .arg(kind)
        .arg(&module)
        .arg(tool)
        .arg(args.to_string())
        .arg(cwd);
    run(command, gate, "")
}

fn plugin_answer(output: &Output) -> serde_json::Value {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| panic!("{e}: {output:?}"))
}

#[test]
fn pi_extension_asks_the_endpoint_and_blocks_on_a_deny() {
    if !node_available() {
        eprintln!("node not found; the Pi extension test is skipped");
        return;
    }
    let fixture = fixture("gate-plugin-pi");
    let dir = TempDir::new("gate-plugin-pi-run");
    let launch = launched(fixture.service.prepare(
        1,
        &catalog_agent("pi"),
        Path::new("pi.exe"),
        &[],
    ));
    let file = PathBuf::from(&launch.prepend[1]);
    let gate = gate_env(&launch);
    // A file nobody else changes, named relative to the session's folder: no decision.
    let answer = plugin_answer(&run_plugin(
        &dir,
        "pi",
        &file,
        "write",
        serde_json::json!({ "path": "src/own.rs", "content": "x" }),
        &fixture.root,
        &gate,
    ));
    assert_eq!(answer["result"], serde_json::Value::Null);
    // A read is never asked about.
    let answer = plugin_answer(&run_plugin(
        &dir,
        "pi",
        &file,
        "read",
        serde_json::json!({ "path": "src/shared.rs" }),
        &fixture.root,
        &gate,
    ));
    assert_eq!(answer["result"], serde_json::Value::Null);
    // A patch in Pi's `write` content names no file, and `__proto__` is no tool.
    for (tool, args) in [
        (
            "write",
            serde_json::json!({ "path": "src/own.rs",
                                "content": "*** Begin Patch\n*** Add File: src/shared.rs\n" }),
        ),
        ("__proto__", serde_json::json!({ "path": "src/shared.rs" })),
    ] {
        let answer = plugin_answer(&run_plugin(
            &dir,
            "pi",
            &file,
            tool,
            args,
            &fixture.root,
            &gate,
        ));
        assert_eq!(answer["result"], serde_json::Value::Null, "{tool}");
    }
    // The other session's file, named the way Pi's tools accept it (a leading "@"), waits for
    // the person, who denies.
    let answering = {
        let (file, root, gate) = (file.clone(), fixture.root.clone(), gate.clone());
        let dir = TempDir::new("gate-plugin-pi-held");
        std::thread::spawn(move || {
            run_plugin(
                &dir,
                "pi",
                &file,
                "edit",
                serde_json::json!({ "path": "@src/shared.rs", "edits": [] }),
                &root,
                &gate,
            )
        })
    };
    answer_next(&fixture, Answer::Deny).join().unwrap();
    let answer = plugin_answer(&answering.join().unwrap());
    assert_eq!(answer["result"]["block"], true);
    let reason = answer["result"]["reason"].as_str().unwrap();
    assert!(reason.contains("src/shared.rs"), "{reason}");
    // Once the session ended, its token is dead and the edit goes ahead (fail open).
    fixture.service.ended(1);
    let answer = plugin_answer(&run_plugin(
        &dir,
        "pi",
        &file_copy_after_end(&dir, "pi"),
        "edit",
        serde_json::json!({ "path": "src/shared.rs", "edits": [] }),
        &fixture.root,
        &gate,
    ));
    assert_eq!(answer["result"], serde_json::Value::Null);
}

/// The module copied into the test folder before the session's own folder went away.
fn file_copy_after_end(dir: &TempDir, kind: &str) -> PathBuf {
    let kept = dir.0.join(format!("{kind}-kept.mjs"));
    std::fs::copy(dir.0.join(format!("{kind}-gate.mjs")), &kept).unwrap();
    kept
}

#[test]
fn opencode_plugin_asks_the_endpoint_and_throws_on_a_deny() {
    if !node_available() {
        eprintln!("node not found; the OpenCode plugin test is skipped");
        return;
    }
    let fixture = fixture("gate-plugin-opencode");
    let dir = TempDir::new("gate-plugin-opencode-run");
    let launch = launched(fixture.service.prepare(
        1,
        &catalog_agent("opencode"),
        Path::new("opencode.exe"),
        &[],
    ));
    let config: serde_json::Value = launch
        .env
        .iter()
        .find(|(name, _)| name == hook_recipes::OPENCODE_CONFIG_CONTENT_ENV)
        .map(|(_, value)| serde_json::from_str(value.as_str()).unwrap())
        .unwrap();
    let file = url::Url::parse(config["plugin"][0].as_str().unwrap())
        .unwrap()
        .to_file_path()
        .unwrap();
    let gate = gate_env(&launch);
    // Each of these goes ahead at once; one that were asked about would wait for the person and
    // run into the run limit.
    let patch_in_content = "*** Begin Patch\n*** Add File: src/shared.rs\n*** End Patch\n";
    for (tool, args) in [
        (
            "write",
            serde_json::json!({ "filePath": fixture.root.join("src").join("own.rs"), "content": "x" }),
        ),
        // File content that looks like a patch names no file.
        (
            "write",
            serde_json::json!({ "filePath": fixture.root.join("src").join("own.rs"),
                                "content": patch_in_content }),
        ),
        // OpenCode takes a path as given: "@src/shared.rs" is another file than src/shared.rs.
        (
            "edit",
            serde_json::json!({ "filePath": "@src/shared.rs", "oldString": "a", "newString": "b" }),
        ),
        // Only the plugin's own tool table counts, not what an object inherits.
        (
            "constructor",
            serde_json::json!({ "filePath": fixture.root.join("src").join("shared.rs") }),
        ),
    ] {
        let answer = plugin_answer(&run_plugin(
            &dir,
            "opencode",
            &file,
            tool,
            args.clone(),
            &fixture.root,
            &gate,
        ));
        assert_eq!(answer["result"], serde_json::Value::Null, "{tool} {args}");
    }
    let answering = {
        let (file, root, gate) = (file.clone(), fixture.root.clone(), gate.clone());
        let dir = TempDir::new("gate-plugin-opencode-held");
        std::thread::spawn(move || {
            run_plugin(
                &dir,
                "opencode",
                &file,
                "edit",
                serde_json::json!({ "filePath": root.join("src").join("shared.rs"),
                                    "oldString": "a", "newString": "b" }),
                &root,
                &gate,
            )
        })
    };
    answer_next(&fixture, Answer::Deny).join().unwrap();
    let answer = plugin_answer(&answering.join().unwrap());
    let thrown = answer["thrown"].as_str().unwrap();
    assert!(thrown.contains("src/shared.rs"), "{thrown}");
}

#[test]
fn a_binary_without_the_helper_marker_is_refused_without_spawning() {
    let dir = TempDir::new("gate-hook-marker");
    let stale = dir.0.join("stale.bin");
    std::fs::write(&stale, b"MZ   some other build, usage: hangar nothing").unwrap();
    let error = require_helper_marker(&stale).unwrap_err();
    assert!(
        error.contains("stale hangar binary without the gate helper"),
        "{error}"
    );

    let current = dir.0.join("current.bin");
    std::fs::write(&current, format!("MZ  {HELPER_MARKER}>  ")).unwrap();
    assert!(require_helper_marker(&current).is_ok());
    assert!(require_helper_marker(&dir.0.join("missing.bin")).is_err());
}

#[test]
fn the_real_binary_carries_the_helper_marker() {
    assert_eq!(require_helper_marker(Path::new(HELPER)), Ok(()));
}

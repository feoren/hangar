//! Integration tests with a real PTY (ConPTY on Windows, Unix PTY elsewhere):
//! echo, exit code, size, kill, restart, start from the agent catalog.

use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hangar_lib::agent_catalog::{self, CatalogAgent};
use hangar_lib::install_source::{Packages, Reason, SelfUpdate};
use hangar_lib::manager::{
    GateLaunch, GatePlan, LaunchGate, ManagerError, SessionEvent, SessionManager, TermSize,
};
use hangar_lib::profile::Profile;
use hangar_lib::secrets::{self, SecretError, SecretStore};
use hangar_lib::session::{GateStatus, SessionInfo, SessionKind, Status};
use hangar_lib::updates::{CommandError, PlatformHint};
use zeroize::Zeroizing;

const TIMEOUT: Duration = Duration::from_secs(20);
const SIZE: TermSize = TermSize { cols: 97, rows: 31 };

fn profile(id: &str, command: &str, args: &[&str]) -> Profile {
    Profile {
        id: id.into(),
        name: id.into(),
        command: command.into(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        cwd: None,
        env: Default::default(),
        secret_env: Vec::new(),
        icon: None,
        version_args: None,
        resume_args: None,
        session_id_args: None,
        resume_id_args: None,
        sub_agents: None,
        latest: None,
        update_hint: None,
        packages: None,
        self_update: None,
    }
}

#[cfg(windows)]
fn profiles() -> Vec<Profile> {
    vec![
        profile("echo", "cmd", &["/d", "/c", "echo hangar-echo-ok & exit 3"]),
        profile(
            "size",
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "Start-Sleep -Milliseconds 1500; $s = $Host.UI.RawUI.WindowSize; \"SIZE=$($s.Width)x$($s.Height)\"",
            ],
        ),
        profile("sleep", "ping", &["-n", "60", "127.0.0.1"]),
        profile("cat", "cmd", &["/d", "/q", "/k"]),
        profile(
            "env",
            "cmd",
            &[
                "/d",
                "/c",
                "echo PATH_OK=%PATH:~0,3% & if defined CLAUDE_CODE_SESSION_ID (echo ENV=leak) else (echo ENV=clean)",
            ],
        ),
    ]
}

#[cfg(unix)]
fn profiles() -> Vec<Profile> {
    vec![
        profile("echo", "sh", &["-c", "echo hangar-echo-ok; exit 3"]),
        profile(
            "size",
            "sh",
            &["-c", "sleep 1.5; echo SIZE=$(tput cols)x$(tput lines)"],
        ),
        profile("sleep", "sleep", &["60"]),
        profile("cat", "cat", &[]),
        profile(
            "env",
            "sh",
            &[
                "-c",
                "[ -z \"$CLAUDE_CODE_SESSION_ID\" ] && echo ENV=clean || echo ENV=leak; echo PATH_OK=${PATH%%:*}",
            ],
        ),
    ]
}

fn manager() -> SessionManager {
    SessionManager::new(profiles(), Vec::new(), std::env::temp_dir())
}

/// Plays the terminal: at startup ConPTY asks for the cursor position with `ESC[6n` and passes
/// no output on until it gets an answer. In the app, xterm.js answers on its own.
fn next_event(
    manager: &SessionManager,
    id: u64,
    rx: &Receiver<SessionEvent>,
    deadline: Instant,
) -> Option<SessionEvent> {
    let event = rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .ok()?;
    if let SessionEvent::Output { data, .. } = &event {
        for _ in data.matches("\x1b[6n") {
            // After the exit, writing may fail.
            let _ = manager.write(id, "\x1b[1;1R");
        }
    }
    Some(event)
}

/// Marks a fallback in the collected output so tests can check the order.
fn fallback_marker(code: Option<u32>) -> String {
    format!("[[resume-fallback {code:?}]]")
}

/// Collects output until the exit event arrives.
fn collect_until_exit(
    manager: &SessionManager,
    id: u64,
    rx: &Receiver<SessionEvent>,
) -> (String, Option<u32>) {
    let deadline = Instant::now() + TIMEOUT;
    let mut output = String::new();
    loop {
        match next_event(manager, id, rx, deadline) {
            Some(SessionEvent::Output { data, .. }) => output.push_str(&data),
            Some(SessionEvent::ResumeFallback { code, .. }) => {
                output.push_str(&fallback_marker(code));
            }
            Some(SessionEvent::Exit { code }) => return (output, code),
            None => {
                panic!("no exit event within {TIMEOUT:?}; output so far: {output:?}")
            }
        }
    }
}

fn wait_for_output(
    manager: &SessionManager,
    id: u64,
    rx: &Receiver<SessionEvent>,
    needle: &str,
) -> String {
    let deadline = Instant::now() + TIMEOUT;
    let mut output = String::new();
    while !output.contains(needle) {
        match next_event(manager, id, rx, deadline) {
            Some(SessionEvent::Output { data, .. }) => output.push_str(&data),
            Some(SessionEvent::ResumeFallback { code, .. }) => {
                output.push_str(&fallback_marker(code));
            }
            Some(SessionEvent::Exit { code }) => {
                panic!("exited early ({code:?}): {output:?}")
            }
            None => panic!("'{needle}' never arrived; output so far: {output:?}"),
        }
    }
    output
}

fn wait_for_status(manager: &SessionManager, id: u64, status: Status) {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if manager
            .list()
            .iter()
            .any(|s| s.id == id && s.status == status)
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("session {id} never reaches status {status:?}");
}

/// A Store app starts in ConPTY through its execution alias (an empty reparse-point file), the
/// way PATH resolution hands it over. Skipped where pwsh is no alias.
#[cfg(windows)]
#[test]
fn execution_alias_starts_in_a_pty() {
    let path = std::env::var_os("PATH");
    let pathext = std::env::var_os("PATHEXT");
    let Some(pwsh) =
        hangar_lib::resolve::resolve_program("pwsh", path.as_deref(), pathext.as_deref())
    else {
        return;
    };
    if hangar_lib::exec_alias::read(&pwsh).is_none() {
        return;
    }
    let command = pwsh.to_string_lossy().into_owned();
    let alias = profile(
        "alias",
        &command,
        &[
            "-NoProfile",
            "-Command",
            "Write-Output ('alias-' + 'ok'); exit 4",
        ],
    );
    let manager = SessionManager::new(vec![alias], Vec::new(), std::env::temp_dir());
    let (tx, rx) = channel();
    let info = manager
        .create("alias", None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    let (output, code) = collect_until_exit(&manager, info.id, &rx);
    assert!(output.contains("alias-ok"), "Output: {output:?}");
    assert_eq!(code, Some(4));
}

#[test]
fn echo_and_exit_code() {
    let manager = manager();
    let (tx, rx) = channel();
    let info = manager
        .create("echo", None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    let (output, code) = collect_until_exit(&manager, info.id, &rx);
    assert!(output.contains("hangar-echo-ok"), "Output: {output:?}");
    assert_eq!(code, Some(3));
    wait_for_status(&manager, info.id, Status::Exited);
    let listed = manager.list();
    assert_eq!(listed[0].exit_code, Some(3));
}

#[test]
fn initial_size_and_resize_reach_the_process() {
    let manager = manager();
    let (tx, rx) = channel();
    let info = manager
        .create("size", None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    manager
        .resize(
            info.id,
            TermSize {
                cols: 111,
                rows: 33,
            },
        )
        .unwrap();
    let (output, _) = collect_until_exit(&manager, info.id, &rx);
    assert!(output.contains("SIZE=111x33"), "Output: {output:?}");
}

#[test]
fn input_reaches_the_process() {
    let manager = manager();
    let (tx, rx) = channel();
    let info = manager
        .create("cat", None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    // cmd /k prints the input as a command; cat mirrors it through the PTY echo.
    let line = if cfg!(windows) {
        "echo hangar-input-ok\r"
    } else {
        "hangar-input-ok\n"
    };
    manager.write(info.id, line).unwrap();
    wait_for_output(&manager, info.id, &rx, "hangar-input-ok");
    manager.close(info.id).unwrap();
}

#[test]
fn kill_and_restart() {
    let manager = manager();
    let (tx, rx) = channel();
    let info = manager
        .create("sleep", None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    assert_eq!(info.status, Status::Running);
    assert!(matches!(
        manager.restart(info.id, SIZE, |_| {}),
        Err(ManagerError::StillRunning(_))
    ));

    let started = Instant::now();
    manager.kill(info.id).unwrap();
    collect_until_exit(&manager, info.id, &rx);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "kill took too long"
    );
    wait_for_status(&manager, info.id, Status::Exited);
    assert!(matches!(
        manager.write(info.id, "x"),
        Err(ManagerError::NotRunning(_))
    ));

    let (tx2, rx2) = channel();
    let restarted = manager
        .restart(info.id, SIZE, move |e| tx2.send(e).unwrap_or(()))
        .unwrap();
    assert_eq!(restarted.id, info.id);
    assert_eq!(restarted.status, Status::Running);
    manager.close(info.id).unwrap();
    collect_until_exit(&manager, info.id, &rx2);
    assert!(manager.list().is_empty());
}

#[test]
fn errors_are_reported_clearly() {
    let mut list = profiles();
    list.push(profile("missing", "hangar-does-not-exist", &[]));
    let manager = SessionManager::new(list, Vec::new(), std::env::temp_dir());
    assert!(matches!(
        manager.create("missing", None, SIZE, |_| {}),
        Err(ManagerError::CommandNotFound(_))
    ));
    assert!(matches!(
        manager.create("unknown", None, SIZE, |_| {}),
        Err(ManagerError::UnknownProfile(_))
    ));
    let bad_cwd = std::env::temp_dir().join("hangar-folder-does-not-exist");
    assert!(matches!(
        manager.create("echo", Some(bad_cwd), SIZE, |_| {}),
        Err(ManagerError::BadCwd(_))
    ));
    // `src` exists relative to the test's working folder; relative is refused all the same.
    assert!(std::path::Path::new("src").is_dir());
    assert!(matches!(
        manager.create("echo", Some("src".into()), SIZE, |_| {}),
        Err(ManagerError::BadCwd(_))
    ));
    let mut relative = profile("relative", "hangar-does-not-exist", &[]);
    relative.cwd = Some("src".into());
    let manager = SessionManager::new(vec![relative], Vec::new(), std::env::temp_dir());
    assert!(matches!(
        manager.create("relative", None, SIZE, |_| {}),
        Err(ManagerError::BadCwd(_))
    ));
    assert!(manager.list().is_empty());
}

#[test]
fn markers_of_a_calling_agent_session_are_not_inherited() {
    // If someone starts Hangar from an agent session, its identifier (token included) must not
    // travel into every session; the child would otherwise take itself for a sub-session.
    std::env::set_var("CLAUDE_CODE_SESSION_ID", "hangar-test-marker");
    let manager = manager();
    let (tx, rx) = channel();
    let info = manager
        .create("env", None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    let (output, _) = collect_until_exit(&manager, info.id, &rx);
    assert!(output.contains("ENV=clean"), "Output: {output:?}");
    // The rest of the environment is kept.
    assert!(output.contains("PATH_OK="), "Output: {output:?}");
    assert!(!output.contains("PATH_OK=\r"), "PATH missing: {output:?}");
}

fn catalog_agent(id: &str, commands: &[&str], args: &[&str]) -> CatalogAgent {
    let toml = format!(
        "[[agent]]
id = {id:?}
name = {id:?}
commands = {commands:?}
args = {args:?}
"
    );
    agent_catalog::parse(&toml).unwrap().remove(0)
}

#[test]
fn catalog_agent_starts_with_first_found_command_and_restarts() {
    // Like a real catalog entry: the first name is missing, the second is found and started.
    let (command, args): (&str, &[&str]) = if cfg!(windows) {
        ("cmd", &["/d", "/c", "echo hangar-catalog-ok & exit 4"])
    } else {
        ("sh", &["-c", "echo hangar-catalog-ok; exit 4"])
    };
    let catalog = vec![
        catalog_agent("catalog", &["hangar-does-not-exist", command], args),
        catalog_agent("missing", &["hangar-does-not-exist-either"], &[]),
        // A user profile with the same id wins; this entry must never start.
        catalog_agent("echo", &["hangar-does-not-exist"], &[]),
    ];
    let manager = SessionManager::new(profiles(), catalog, std::env::temp_dir());

    let listed = manager.profiles();
    let entry = listed.iter().find(|p| p.id == "catalog").unwrap();
    assert!(entry.installed);
    assert_eq!(entry.command, command);
    assert!(!listed.iter().find(|p| p.id == "missing").unwrap().installed);
    assert_eq!(listed.iter().filter(|p| p.id == "echo").count(), 1);

    let (tx, rx) = channel();
    let info = manager
        .create("catalog", None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    let (output, code) = collect_until_exit(&manager, info.id, &rx);
    assert!(output.contains("hangar-catalog-ok"), "Output: {output:?}");
    assert_eq!(code, Some(4));
    wait_for_status(&manager, info.id, Status::Exited);

    let (tx2, rx2) = channel();
    manager
        .restart(info.id, SIZE, move |e| tx2.send(e).unwrap_or(()))
        .unwrap();
    let (output, code) = collect_until_exit(&manager, info.id, &rx2);
    assert!(output.contains("hangar-catalog-ok"), "Output: {output:?}");
    assert_eq!(code, Some(4));

    match manager.create("missing", None, SIZE, |_| {}) {
        Err(ManagerError::CommandNotFound(name)) => {
            assert_eq!(name, "hangar-does-not-exist-either")
        }
        other => panic!("expected CommandNotFound, got {other:?}"),
    }
    // The overridden catalog entry does not start; the user profile runs instead.
    let (tx3, rx3) = channel();
    let info = manager
        .create("echo", None, SIZE, move |e| tx3.send(e).unwrap_or(()))
        .unwrap();
    let (output, _) = collect_until_exit(&manager, info.id, &rx3);
    assert!(output.contains("hangar-echo-ok"), "Output: {output:?}");
}

// --- Update sessions (create_update) ---

fn hint(command: &str) -> PlatformHint {
    PlatformHint {
        windows: Some(command.into()),
        unix: Some(command.into()),
    }
}

#[test]
fn update_session_runs_update_hint_and_restart_repeats_it() {
    let echo = if cfg!(windows) {
        "cmd /d /c echo hangar-update-ok"
    } else {
        "echo hangar-update-ok"
    };
    // The program itself would run for 60 s; only the update command may be started.
    let mut agent = profiles().into_iter().find(|p| p.id == "sleep").unwrap();
    agent.id = "prog".into();
    agent.name = "Prog".into();
    agent.update_hint = Some(hint(echo));
    let mut pipeline = profile("pipe", "hangar-does-not-exist", &[]);
    pipeline.update_hint = Some(hint("irm https://example.invalid/x.ps1 | iex"));
    let without = profile("without", "hangar-does-not-exist", &[]);
    let mut relative = profile("relative", "hangar-does-not-exist", &[]);
    relative.update_hint = Some(hint("./tools/x upgrade"));
    let manager = SessionManager::new(
        vec![agent, pipeline, without, relative],
        Vec::new(),
        std::env::temp_dir(),
    );

    let (tx, rx) = channel();
    let info = manager
        .create_update("prog", SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    assert_eq!(info.kind, SessionKind::Update);
    assert_eq!(info.title, "Update: Prog");
    assert_eq!(info.profile_id, "prog");
    // Without a fixed folder in the profile, the update runs in the user's home folder (here: temp).
    assert_eq!(info.cwd, std::env::temp_dir());
    let (output, code) = collect_until_exit(&manager, info.id, &rx);
    assert!(output.contains("hangar-update-ok"), "Output: {output:?}");
    assert_eq!(code, Some(0));
    wait_for_status(&manager, info.id, Status::Exited);
    assert_eq!(manager.kind(info.id), Some(SessionKind::Update));

    let (tx2, rx2) = channel();
    let restarted = manager
        .restart(info.id, SIZE, move |e| tx2.send(e).unwrap_or(()))
        .unwrap();
    assert_eq!(restarted.kind, SessionKind::Update);
    let (output, code) = collect_until_exit(&manager, info.id, &rx2);
    assert!(output.contains("hangar-update-ok"), "Output: {output:?}");
    assert_eq!(code, Some(0));

    for (id, reason) in [
        ("pipe", CommandError::ShellSyntax('|')),
        ("without", CommandError::Missing),
        (
            "relative",
            CommandError::RelativeProgram("./tools/x".into()),
        ),
    ] {
        match manager.create_update(id, SIZE, |_| {}) {
            Err(ManagerError::NoUpdateCommand { reason: got, .. }) => assert_eq!(got, reason),
            other => panic!("{id}: expected NoUpdateCommand, got {other:?}"),
        }
    }
    assert!(matches!(
        manager.create_update("unknown", SIZE, |_| {}),
        Err(ManagerError::UnknownProfile(_))
    ));
    // Normal sessions stay agent sessions.
    let normal = manager.create("prog", None, SIZE, |_| {}).unwrap();
    assert_eq!(normal.kind, SessionKind::Agent);
    manager.close(normal.id).unwrap();
    assert_eq!(manager.list().len(), 1);
}

/// Without `update_hint` the plan decides: an installation-aware self-updater runs as the resolved
/// absolute program with its arguments; a program whose installer is unknown gets no run plan.
#[test]
fn planned_update_runs_self_updater_of_resolved_program() {
    let (command, args): (&str, &[&str]) = if cfg!(windows) {
        ("cmd", &["/d", "/c", "echo", "hangar-self-update"])
    } else {
        ("echo", &["hangar-self-update"])
    };
    let mut prog = profile("selfup", command, &["--never-started"]);
    prog.self_update = Some(SelfUpdate {
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        aware: true,
        only: None,
    });
    let mut unknown = profile("unknown", command, &[]);
    unknown.packages = Some(Packages {
        npm: Some("some-cli".into()),
        ..Packages::default()
    });
    let manager = SessionManager::new(vec![prog, unknown], Vec::new(), std::env::temp_dir());

    let (_, plan) = manager.update_plan("selfup").unwrap();
    let command_text = plan.command().unwrap();
    assert!(
        std::path::Path::new(command_text.trim_matches('"').split(".exe").next().unwrap())
            .is_absolute()
            || command_text.starts_with('/'),
        "{command_text}"
    );
    // A plan the user did not see is refused; the one shown starts.
    assert!(matches!(
        manager.create_update_checked("selfup", Some("0000000000000000"), SIZE, |_| {}),
        Err(ManagerError::PlanChanged(_))
    ));
    let (tx, rx) = channel();
    let info = manager
        .create_update_checked("selfup", Some(&plan.fingerprint()), SIZE, move |e| {
            tx.send(e).unwrap_or(())
        })
        .unwrap();
    assert_eq!(info.kind, SessionKind::Update);
    let (output, code) = collect_until_exit(&manager, info.id, &rx);
    assert!(output.contains("hangar-self-update"), "Output: {output:?}");
    assert_eq!(code, Some(0));
    // The exact argv comes first, dimmed, before anything the program writes.
    let shown = format!("\x1b[2m{command_text}\x1b[0m\r\n");
    assert!(output.starts_with(&shown), "Output: {output:?}");

    match manager.create_update("unknown", SIZE, |_| {}) {
        Err(ManagerError::NoUpdateCommand { reason, .. }) => {
            assert_eq!(reason, CommandError::NoPlan(Reason::UnknownInstaller));
        }
        other => panic!("expected NoUpdateCommand, got {other:?}"),
    }
}

#[test]
fn second_update_of_same_program_is_rejected() {
    let long = if cfg!(windows) {
        "ping -n 60 127.0.0.1"
    } else {
        "sleep 60"
    };
    let mut prog = profiles().into_iter().find(|p| p.id == "sleep").unwrap();
    prog.id = "slow".into();
    prog.name = "Slow".into();
    prog.update_hint = Some(hint(long));
    let manager = SessionManager::new(vec![prog], Vec::new(), std::env::temp_dir());

    let (tx, rx) = channel();
    let first = manager
        .create_update("slow", SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    assert!(matches!(
        manager.create_update("slow", SIZE, |_| {}),
        Err(ManagerError::UpdateRunning(name)) if name == "Slow"
    ));
    // The periodic version query leaves a program alone while it is being updated.
    assert!(manager.updating_profiles().contains("slow"));
    // The agent itself may start alongside; only duplicate updates are blocked.
    let agent = manager.create("slow", None, SIZE, |_| {}).unwrap();
    manager.close(agent.id).unwrap();

    // Once the first update has ended, a new one is possible; restarting the old one is then blocked.
    manager.kill(first.id).unwrap();
    collect_until_exit(&manager, first.id, &rx);
    wait_for_status(&manager, first.id, Status::Exited);
    assert!(manager.updating_profiles().is_empty());
    let second = manager.create_update("slow", SIZE, |_| {}).unwrap();
    assert!(matches!(
        manager.restart(first.id, SIZE, |_| {}),
        Err(ManagerError::UpdateRunning(_))
    ));
    // Rejected means the old session stays exited, unchanged.
    wait_for_status(&manager, first.id, Status::Exited);

    manager.close(second.id).unwrap();
    let (tx2, rx2) = channel();
    manager
        .restart(first.id, SIZE, move |e| tx2.send(e).unwrap_or(()))
        .unwrap();
    manager.close(first.id).unwrap();
    collect_until_exit(&manager, first.id, &rx2);
    assert!(manager.list().is_empty());
}

/// Otherwise cmd.exe on Windows (and with it `npm.cmd`) looks for programs in the working folder
/// first. An update session always sets `NoDefaultCurrentDirectoryInExePath`, a session with a shell
/// (here `cmd.exe`) does not; it serves as the counter check that the folder's `probe` would be
/// found at all. `.cmd` launcher scripts: `cmd_launcher_does_not_search_node_in_working_folder`.
#[cfg(windows)]
#[test]
fn update_session_does_not_search_programs_in_working_folder() {
    // Some environments (Claude Code, for one) set the variable themselves; then the counter
    // check would prove nothing. No other test depends on it.
    std::env::remove_var("NoDefaultCurrentDirectoryInExePath");
    let dir = std::env::temp_dir().join(format!("hangar-cwd-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("probe.cmd"), "@echo HANGAR-FOLDER-PROBE\r\n").unwrap();
    let mut prog = profile("probe", "cmd", &["/d", "/c", "probe"]);
    prog.cwd = Some(dir.clone());
    prog.update_hint = Some(hint("cmd /d /c probe"));
    let manager = SessionManager::new(vec![prog], Vec::new(), std::env::temp_dir());

    let (tx, rx) = channel();
    let agent = manager
        .create("probe", None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    let (output, _) = collect_until_exit(&manager, agent.id, &rx);
    assert!(
        output.contains("HANGAR-FOLDER-PROBE"),
        "counter check: {output:?}"
    );

    let (tx, rx) = channel();
    let update = manager
        .create_update("probe", SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    assert_eq!(update.cwd, dir);
    let (output, code) = collect_until_exit(&manager, update.id, &rx);
    assert!(
        !output.contains("HANGAR-FOLDER-PROBE"),
        "Output: {output:?}"
    );
    assert_ne!(code, Some(0));
    std::fs::remove_dir_all(&dir).unwrap();
}

// --- Resume (resume_args) and fallback to the normal start ---

#[cfg(windows)]
fn shell_profile(id: &str, normal: &str, resume: Option<&str>) -> Profile {
    let mut p = profile(id, "cmd", &["/d", "/c", normal]);
    p.resume_args = resume.map(|script| vec!["/d".into(), "/c".into(), script.into()]);
    p
}

#[cfg(unix)]
fn shell_profile(id: &str, normal: &str, resume: Option<&str>) -> Profile {
    let mut p = profile(id, "sh", &["-c", normal]);
    p.resume_args = resume.map(|script| vec!["-c".into(), script.into()]);
    p
}

/// On Windows `&` separates commands, on Unix `;`.
fn script(parts: &[&str]) -> String {
    parts.join(if cfg!(windows) { " & " } else { "; " })
}

fn resume_manager() -> SessionManager {
    let normal_fail = script(&["echo hangar-normal-start", "exit 3"]);
    let normal_ok = script(&["echo hangar-normal-start", "exit 0"]);
    let slow_wait = if cfg!(windows) {
        "ping -n 7 127.0.0.1 >nul"
    } else {
        "sleep 6"
    };
    let long_wait = if cfg!(windows) {
        "ping -n 60 127.0.0.1 >nul"
    } else {
        "sleep 60"
    };
    let profiles = vec![
        shell_profile(
            "r-fail",
            &normal_fail,
            Some(&script(&["echo hangar-resume-attempt", "exit 2"])),
        ),
        shell_profile(
            "r-ok",
            &normal_ok,
            Some(&script(&["echo hangar-resume-ok", "exit 0"])),
        ),
        shell_profile(
            "r-slow",
            &normal_ok,
            Some(&script(&[slow_wait, "echo hangar-resume-slow", "exit 5"])),
        ),
        shell_profile("r-sleep", &normal_ok, Some(long_wait)),
        shell_profile("r-none", &normal_ok, None),
    ];
    SessionManager::new(profiles, Vec::new(), std::env::temp_dir())
}

fn start(
    manager: &SessionManager,
    profile_id: &str,
    resume: bool,
) -> (SessionInfo, Receiver<SessionEvent>) {
    let (tx, rx) = channel();
    let info = manager
        .create_with(profile_id, None, SIZE, resume, move |e| {
            tx.send(e).unwrap_or(())
        })
        .unwrap();
    (info, rx)
}

fn resumed_flag(manager: &SessionManager, id: u64) -> bool {
    manager.list().iter().find(|s| s.id == id).unwrap().resumed
}

#[test]
fn resume_fails_fast_then_exactly_one_normal_start() {
    let manager = resume_manager();
    let gate = Arc::new(RecordingGate::default());
    manager.set_launch_gate(gate.clone());
    let (info, rx) = start(&manager, "r-fail", true);
    assert!(info.resumed);
    let (output, code) = collect_until_exit(&manager, info.id, &rx);
    let position = |needle: &str| {
        output
            .find(needle)
            .unwrap_or_else(|| panic!("'{needle}' missing: {output:?}"))
    };
    let attempt = position("hangar-resume-attempt");
    let fallback = position(&fallback_marker(Some(2)));
    let normal = position("hangar-normal-start");
    assert!(
        attempt < fallback && fallback < normal,
        "Output: {output:?}"
    );
    // The normal start fails quickly too (exit 3), but there is no second fallback.
    assert_eq!(output.matches("[[resume-fallback").count(), 1);
    assert_eq!(code, Some(3));
    wait_for_status(&manager, info.id, Status::Exited);
    let listed = manager.list();
    assert_eq!(listed[0].exit_code, Some(3));
    assert!(!listed[0].resumed);
    // The gate hears of both processes: the failed resume ends before the fallback starts, so
    // the fallback gets a fresh registration, and the fallback's own end follows.
    let count = |list: &std::sync::Mutex<Vec<u64>>| {
        list.lock()
            .unwrap()
            .iter()
            .filter(|id| **id == info.id)
            .count()
    };
    assert_eq!(count(&gate.prepared), 2);
    assert_eq!(count(&gate.ended), 2);
}

#[test]
fn resume_succeeds_without_fallback() {
    let manager = resume_manager();
    let (info, rx) = start(&manager, "r-ok", true);
    assert!(info.resumed);
    let (output, code) = collect_until_exit(&manager, info.id, &rx);
    assert!(output.contains("hangar-resume-ok"), "Output: {output:?}");
    assert!(!output.contains("[[resume-fallback"), "Output: {output:?}");
    assert!(
        !output.contains("hangar-normal-start"),
        "Output: {output:?}"
    );
    assert_eq!(code, Some(0));
    wait_for_status(&manager, info.id, Status::Exited);
    assert!(resumed_flag(&manager, info.id));
}

#[test]
fn without_resume_args_or_resume_starts_normally() {
    let manager = resume_manager();
    for (profile_id, resume) in [("r-none", true), ("r-fail", false)] {
        let (info, rx) = start(&manager, profile_id, resume);
        assert!(!info.resumed, "{profile_id}");
        let (output, _) = collect_until_exit(&manager, info.id, &rx);
        assert!(output.contains("hangar-normal-start"), "Output: {output:?}");
        assert!(!output.contains("hangar-resume"), "Output: {output:?}");
        assert!(!output.contains("[[resume-fallback"), "Output: {output:?}");
    }
    // Existing calls without resume behave as before.
    let (tx, rx) = channel();
    let info = manager
        .create("r-fail", None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    let (_, code) = collect_until_exit(&manager, info.id, &rx);
    assert_eq!(code, Some(3));
}

#[test]
fn late_failure_after_the_window_is_no_fallback() {
    let manager = resume_manager();
    let (info, rx) = start(&manager, "r-slow", true);
    let (output, code) = collect_until_exit(&manager, info.id, &rx);
    assert!(output.contains("hangar-resume-slow"), "Output: {output:?}");
    assert!(!output.contains("[[resume-fallback"), "Output: {output:?}");
    assert_eq!(code, Some(5));
}

#[test]
fn resume_killed_by_user_is_no_fallback() {
    let manager = resume_manager();
    let (info, rx) = start(&manager, "r-sleep", true);
    std::thread::sleep(Duration::from_millis(300));
    manager.kill(info.id).unwrap();
    let (output, _) = collect_until_exit(&manager, info.id, &rx);
    assert!(!output.contains("[[resume-fallback"), "Output: {output:?}");
    assert!(
        !output.contains("hangar-normal-start"),
        "Output: {output:?}"
    );
    wait_for_status(&manager, info.id, Status::Exited);
}

// --- .cmd/.bat launcher scripts (npm shims such as codex.cmd) ---

/// Empty folder tree under temp that disappears when dropped.
#[cfg(windows)]
struct Scratch(std::path::PathBuf);

#[cfg(windows)]
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("hangar-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self, relative: &str, content: &str) -> std::path::PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }
}

#[cfg(windows)]
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(windows)]
fn run_to_exit(manager: &SessionManager, profile_id: &str) -> (String, Option<u32>) {
    let (tx, rx) = channel();
    let info = manager
        .create(profile_id, None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    collect_until_exit(manager, info.id, &rx)
}

/// Like an npm shim without node.exe next to it, the launcher script calls `node` through the
/// program search of cmd.exe. A foreign project as working folder must not be able to plant
/// its own `node.cmd`. A shell session in the same folder still looks there (counter check).
#[cfg(windows)]
#[test]
fn cmd_launcher_does_not_search_node_in_working_folder() {
    // See update_session_does_not_search_programs_in_working_folder.
    std::env::remove_var("NoDefaultCurrentDirectoryInExePath");
    let scratch = Scratch::new("cmd-node");
    let launcher = scratch.file(
        r"bin\agent.cmd",
        "@echo off\r\nnode --hangar-probe 2>nul\r\necho HANGAR-LAUNCHER-END\r\n",
    );
    scratch.file(
        r"work\node.cmd",
        "@echo HANGAR-FOREIGN-NODE\r\n@type nul > node-ran.txt\r\n",
    );
    let work = scratch.0.join("work");
    let mut agent = profile("agent", launcher.to_str().unwrap(), &[]);
    agent.cwd = Some(work.clone());
    let mut shell = profile("shell", "cmd", &["/d", "/c", "node --hangar-probe"]);
    shell.cwd = Some(work.clone());
    // An empty value in the profile turns the protection off on purpose (any casing).
    let mut opt_out = agent.clone();
    opt_out.id = "unprotected".into();
    opt_out
        .env
        .insert("nodefaultcurrentdirectoryinexepath".into(), String::new());
    let manager = SessionManager::new(
        vec![agent, shell, opt_out],
        Vec::new(),
        std::env::temp_dir(),
    );

    let (output, _) = run_to_exit(&manager, "agent");
    // If one .cmd calls another without CALL, it does not return; without the protection the
    // final line is therefore missing as well.
    assert!(
        !output.contains("HANGAR-FOREIGN-NODE"),
        "foreign node.cmd ran: {output:?}"
    );
    assert!(output.contains("HANGAR-LAUNCHER-END"), "Output: {output:?}");
    assert!(!work.join("node-ran.txt").exists());

    for id in ["shell", "unprotected"] {
        let (output, _) = run_to_exit(&manager, id);
        assert!(
            output.contains("HANGAR-FOREIGN-NODE"),
            "counter check {id}: {output:?}"
        );
    }
}

/// portable-pty quotes by MSVC rules, but cmd.exe interprets `&`, `|`, `%` … itself
/// (BatBadBut). Dangerous characters are rejected before a process starts; spaces,
/// parentheses in quoted parts and a working folder containing `&` keep working.
#[cfg(windows)]
#[test]
fn cmd_launcher_rejects_cmd_special_characters() {
    let scratch = Scratch::new("cmd-args");
    let echo = "@echo off\r\necho HANGAR-ARGS=[%*]\r\n";
    let launcher = scratch.file(r"bin\echoargs.cmd", echo);
    let amp_launcher = scratch.file(r"bin&x\echoargs.cmd", echo);
    let amp_cwd = scratch.0.join("wo&rk");
    std::fs::create_dir_all(&amp_cwd).unwrap();
    let launcher = launcher.to_str().unwrap();

    let mut ok = profile("ok", launcher, &["fix (this) now", "--model=x", ""]);
    ok.cwd = Some(amp_cwd.clone());
    let manager = SessionManager::new(
        vec![
            ok,
            profile("amp", launcher, &["x&echo.HANGAR-INJECTED"]),
            profile("quote", launcher, &["a \"b\" c"]),
            profile("percent", launcher, &["%PATH%"]),
            profile("paren", launcher, &["a)"]),
            profile("path", amp_launcher.to_str().unwrap(), &[]),
        ],
        Vec::new(),
        std::env::temp_dir(),
    );

    let (output, code) = run_to_exit(&manager, "ok");
    assert!(
        output.contains(r#"HANGAR-ARGS=["fix (this) now" --model=x ""]"#),
        "Output: {output:?}"
    );
    assert_eq!(code, Some(0));

    for (id, needle) in [
        ("amp", "'&'"),
        ("quote", "'\"'"),
        ("percent", "'%'"),
        ("paren", "')'"),
        ("path", "'&'"),
    ] {
        match manager.create(id, None, SIZE, |_| {}) {
            Err(error @ ManagerError::UnsafeBatch { .. }) => {
                let text = error.to_string();
                assert!(text.contains(needle), "{id}: {text}");
            }
            other => panic!("{id}: expected UnsafeBatch, got {other:?}"),
        }
    }
    assert_eq!(manager.list().len(), 1, "rejected means no session");
}

#[test]
fn reloaded_profiles_leave_running_sessions_untouched() {
    let manager = manager();
    let (tx, rx) = channel();
    let info = manager
        .create("cat", None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    let ids =
        |m: &SessionManager| -> Vec<String> { m.profiles().into_iter().map(|p| p.id).collect() };
    assert!(ids(&manager).contains(&"cat".to_owned()));

    // The new file no longer knows "cat", but has "new" with the command of "echo".
    let mut renamed = profiles().remove(0);
    renamed.id = "new".into();
    manager.set_profiles(vec![renamed]);
    assert_eq!(ids(&manager), ["new"]);
    assert!(matches!(
        manager.create("cat", None, SIZE, |_| {}),
        Err(ManagerError::UnknownProfile(_))
    ));

    // The running session keeps working with its old profile.
    let line = if cfg!(windows) {
        "echo hangar-reload-ok\r"
    } else {
        "hangar-reload-ok\n"
    };
    manager.write(info.id, line).unwrap();
    wait_for_output(&manager, info.id, &rx, "hangar-reload-ok");
    assert_eq!(manager.list()[0].profile_id, "cat");
    manager.close(info.id).unwrap();

    let (tx2, rx2) = channel();
    let started = manager
        .create("new", None, SIZE, move |e| tx2.send(e).unwrap_or(()))
        .unwrap();
    let (output, code) = collect_until_exit(&manager, started.id, &rx2);
    assert!(output.contains("hangar-echo-ok"), "Output: {output:?}");
    assert_eq!(code, Some(3));
}

// --- Secret variables from the keychain, WSL-style working folder argument ---

const SECRET_VALUE: &str = "hangar-secret-marker-7f3a";

/// In-memory secrets; the app's store is the OS keychain, which a test must not touch.
#[derive(Default)]
struct MemoryStore(std::sync::Mutex<std::collections::HashMap<String, String>>);

impl MemoryStore {
    fn values(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<String, String>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, SecretError> {
        Ok(self.values().get(account).cloned().map(Zeroizing::new))
    }

    fn exists(&self, account: &str) -> Result<bool, SecretError> {
        Ok(self.values().contains_key(account))
    }

    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        self.values().insert(account.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.values().remove(account);
        Ok(())
    }
}

#[cfg(windows)]
fn secret_profile(id: &str) -> Profile {
    let mut p = profile(
        id,
        "cmd",
        &["/d", "/c", "echo SECRET=[%HANGAR_TEST_SECRET%]"],
    );
    p.secret_env = vec!["HANGAR_TEST_SECRET".into()];
    p
}

#[cfg(unix)]
fn secret_profile(id: &str) -> Profile {
    let mut p = profile(
        id,
        "sh",
        &["-c", "echo \"SECRET=[${HANGAR_TEST_SECRET-unset}]\""],
    );
    p.secret_env = vec!["HANGAR_TEST_SECRET".into()];
    p
}

#[test]
fn secret_variables_reach_the_child_on_start_and_restart() {
    let store = Arc::new(MemoryStore::default());
    secrets::set(
        store.as_ref(),
        "with-secret",
        "HANGAR_TEST_SECRET",
        SECRET_VALUE,
    )
    .unwrap();
    let manager = SessionManager::new(
        vec![secret_profile("with-secret"), secret_profile("no-secret")],
        Vec::new(),
        std::env::temp_dir(),
    )
    .with_secret_store(store);

    let (tx, rx) = channel();
    let info = manager
        .create("with-secret", None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    let (output, _) = collect_until_exit(&manager, info.id, &rx);
    assert!(
        output.contains(&format!("SECRET=[{SECRET_VALUE}]")),
        "Output: {output:?}"
    );
    wait_for_status(&manager, info.id, Status::Exited);
    let (tx, rx) = channel();
    manager
        .restart(info.id, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    let (output, _) = collect_until_exit(&manager, info.id, &rx);
    assert!(
        output.contains(SECRET_VALUE),
        "Output after restart: {output:?}"
    );

    // A profile without a stored value starts without the variable.
    let (output, _) = {
        let (tx, rx) = channel();
        let info = manager
            .create("no-secret", None, SIZE, move |e| tx.send(e).unwrap_or(()))
            .unwrap();
        collect_until_exit(&manager, info.id, &rx)
    };
    assert!(!output.contains(SECRET_VALUE), "Output: {output:?}");
    assert!(output.contains("SECRET=["), "Output: {output:?}");
}

#[test]
fn working_folder_argument_follows_the_profile_arguments() {
    let manager = SessionManager::new(Vec::new(), Vec::new(), std::env::temp_dir());
    let mut agent = agent_catalog::Agent::from_user(if cfg!(windows) {
        profile("folder-arg", "cmd", &["/d", "/c", "echo", "ARGS"])
    } else {
        profile("folder-arg", "echo", &["ARGS"])
    });
    agent.cwd_flag = Some("--cd");
    agent.source = agent_catalog::Source::Catalog;
    manager.set_wsl(vec![agent]);
    let cwd = std::env::temp_dir();
    let (tx, rx) = channel();
    let info = manager
        .create("folder-arg", Some(cwd.clone()), SIZE, move |e| {
            tx.send(e).unwrap_or(())
        })
        .unwrap();
    let (output, _) = collect_until_exit(&manager, info.id, &rx);
    let flat: String = output.split_whitespace().collect::<Vec<_>>().join(" ");
    let expected = format!("ARGS --cd {}", cwd.display());
    let expected: String = expected.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains(&expected), "Output: {output:?}");
    // Entries found on this machine are not user profiles.
    let view = manager
        .profiles()
        .into_iter()
        .find(|p| p.id == "folder-arg")
        .unwrap();
    assert!(!view.editable);
    assert_eq!(manager.user_profile("folder-arg"), None);
}

// --- Conversation ids (session_id_args, resume_id_args) ---

const STORED_ID: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";

/// Prints its arguments like an agent CLI would receive them: normal start with the appended
/// `session_id_args`, resume with `resume_id_args` that fails quickly (no such conversation).
fn id_profile() -> Profile {
    let mut p = if cfg!(windows) {
        let mut p = profile("ids", "cmd", &["/d", "/c", "echo hangar-normal"]);
        p.resume_id_args = Some(vec![
            "/d".into(),
            "/c".into(),
            "echo hangar-resume-id {id} & exit 2".into(),
        ]);
        p
    } else {
        let mut p = profile("ids", "sh", &["-c", "echo hangar-normal \"$0\""]);
        p.resume_id_args = Some(vec![
            "-c".into(),
            "echo hangar-resume-id {id}; exit 2".into(),
        ]);
        p
    };
    p.session_id_args = Some(vec!["hangar-sid={id}".into()]);
    p.sub_agents = Some(hangar_lib::subagents::SubAgentKind::Claude);
    p
}

#[test]
fn fresh_start_gets_a_conversation_id_and_restart_a_new_one() {
    let manager = SessionManager::new(vec![id_profile()], Vec::new(), std::env::temp_dir());
    let (tx, rx) = channel();
    let info = manager
        .create("ids", None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    let id = info.cli_session_id.clone().expect("conversation id");
    assert_eq!(id.len(), 36);
    assert!(info.sub_agents);
    let (output, _) = collect_until_exit(&manager, info.id, &rx);
    assert!(
        output.contains(&format!("hangar-normal hangar-sid={id}")),
        "Output: {output:?}"
    );
    let target = manager.sub_agent_target(info.id).unwrap();
    assert_eq!(target.session, id);

    wait_for_status(&manager, info.id, Status::Exited);
    let (tx, rx) = channel();
    let restarted = manager
        .restart(info.id, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    let new_id = restarted.cli_session_id.clone().unwrap();
    assert_ne!(new_id, id);
    let (output, _) = collect_until_exit(&manager, info.id, &rx);
    assert!(output.contains(&new_id), "Output: {output:?}");
}

#[test]
fn restore_resumes_the_stored_id_and_falls_back_under_the_same_id() {
    let manager = SessionManager::new(vec![id_profile()], Vec::new(), std::env::temp_dir());
    let (tx, rx) = channel();
    let upper = STORED_ID.to_uppercase();
    let info = manager
        .create_resuming("ids", None, SIZE, true, Some(&upper), move |e| {
            tx.send(e).unwrap_or(())
        })
        .unwrap();
    assert!(info.resumed);
    assert_eq!(info.cli_session_id.as_deref(), Some(STORED_ID));
    let deadline = Instant::now() + TIMEOUT;
    let mut output = String::new();
    let mut fallback_id = None;
    loop {
        match next_event(&manager, info.id, &rx, deadline) {
            Some(SessionEvent::Output { data, .. }) => output.push_str(&data),
            Some(SessionEvent::ResumeFallback {
                code,
                cli_session_id,
                sub_agents,
            }) => {
                assert_eq!(code, Some(2));
                assert!(sub_agents);
                output.push_str("[[fallback]]");
                fallback_id = cli_session_id;
            }
            Some(SessionEvent::Exit { .. }) => break,
            None => panic!("no exit; output so far: {output:?}"),
        }
    }
    assert_eq!(fallback_id.as_deref(), Some(STORED_ID));
    let resume = output
        .find(&format!("hangar-resume-id {STORED_ID}"))
        .unwrap_or_else(|| panic!("resume attempt missing: {output:?}"));
    let fallback = output.find("[[fallback]]").unwrap();
    let normal = output
        .find(&format!("hangar-normal hangar-sid={STORED_ID}"))
        .unwrap_or_else(|| panic!("normal start missing: {output:?}"));
    assert!(resume < fallback && fallback < normal, "Output: {output:?}");
    wait_for_status(&manager, info.id, Status::Exited);
    let listed = manager.list();
    assert_eq!(listed[0].cli_session_id.as_deref(), Some(STORED_ID));
    assert!(!listed[0].resumed);
}

#[test]
fn a_conversation_id_in_use_is_not_resumed_twice() {
    let mut long = id_profile();
    long.id = "ids-long".into();
    long.resume_id_args = Some(if cfg!(windows) {
        vec![
            "/d".into(),
            "/c".into(),
            "echo hangar-resume-id {id} & ping -n 30 127.0.0.1 >nul".into(),
        ]
    } else {
        vec!["-c".into(), "echo hangar-resume-id {id}; sleep 30".into()]
    });
    let manager = SessionManager::new(vec![long], Vec::new(), std::env::temp_dir());
    let first = manager
        .create_resuming("ids-long", None, SIZE, true, Some(STORED_ID), |_| {})
        .unwrap();
    assert_eq!(first.cli_session_id.as_deref(), Some(STORED_ID));
    let second = manager
        .create_resuming("ids-long", None, SIZE, true, Some(STORED_ID), |_| {})
        .unwrap();
    assert!(!second.resumed);
    let other = second.cli_session_id.clone().unwrap();
    assert_ne!(other, STORED_ID);
    manager.close(first.id).unwrap();
    manager.close(second.id).unwrap();
}

#[test]
fn invalid_conversation_id_is_refused_before_anything_starts() {
    let manager = SessionManager::new(vec![id_profile()], Vec::new(), std::env::temp_dir());
    for bad in ["", "../x", "0f8fad5b-d9cb-469f-a165-70867728950e x"] {
        let result = manager.create_resuming("ids", None, SIZE, true, Some(bad), |_| {});
        assert!(
            matches!(result, Err(ManagerError::CliSessionId(_))),
            "{bad:?}"
        );
    }
    assert!(manager.list().is_empty());
    // Profiles without a sub-agent source have no target.
    let manager = self::manager();
    let (tx, rx) = channel();
    let info = manager
        .create("echo", None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    assert_eq!(info.cli_session_id, None);
    assert!(!info.sub_agents);
    assert!(matches!(
        manager.sub_agent_target(info.id),
        Err(ManagerError::NoSubAgents(_))
    ));
    let _ = collect_until_exit(&manager, info.id, &rx);
}

/// Stands in for the agent gate: hands one session start an argument and a token, and records
/// which sessions it was told about.
#[derive(Default)]
struct RecordingGate {
    prepared: std::sync::Mutex<Vec<u64>>,
    ended: std::sync::Mutex<Vec<u64>>,
}

impl LaunchGate for RecordingGate {
    fn applies(&self, agent: &hangar_lib::agent_catalog::Agent) -> bool {
        agent.id() != "plain"
    }

    fn prepare(
        &self,
        session: u64,
        agent: &hangar_lib::agent_catalog::Agent,
        _program: &std::path::Path,
    ) -> GatePlan {
        self.prepared.lock().unwrap().push(session);
        match agent.id() {
            "gated" | "own-settings" => GatePlan::Launch(GateLaunch {
                args: vec!["gate-arg-marker".into()],
                env: vec![(
                    hangar_lib::gate::TOKEN_ENV.into(),
                    Zeroizing::new("gate-token-marker".into()),
                )],
            }),
            "ungated" => GatePlan::Unavailable(GateStatus::Launcher),
            _ => GatePlan::Skip,
        }
    }

    fn ended(&self, session: u64) {
        self.ended.lock().unwrap().push(session);
    }
}

/// Prints the gate's token and every argument after its own.
fn gate_probe(id: &str) -> Profile {
    let mut probe = if cfg!(windows) {
        profile(
            id,
            "cmd",
            &["/d", "/c", "echo", "TOKEN=%HANGAR_HOOK_TOKEN%", "ARGS="],
        )
    } else {
        profile(
            id,
            "sh",
            &[
                "-c",
                "echo TOKEN=${HANGAR_HOOK_TOKEN:-none} ARGS=$0",
                "none",
            ],
        )
    };
    // A profile cannot replace the gate's token.
    probe
        .env
        .insert("HANGAR_HOOK_TOKEN".into(), "profile-value".into());
    probe
}

#[test]
fn the_gate_adds_arguments_and_token_and_hears_of_every_end() {
    let gated = gate_probe("gated");
    let mut gated_unix = gated.clone();
    if cfg!(unix) {
        // `sh -c script $0`: the gate's argument replaces the placeholder `$0`.
        gated_unix.args.pop();
    }
    let manager = SessionManager::new(
        vec![gated_unix, gate_probe("plain"), gate_probe("ungated")],
        Vec::new(),
        std::env::temp_dir(),
    );
    let gate = Arc::new(RecordingGate::default());
    manager.set_launch_gate(gate.clone());

    let (tx, rx) = channel();
    let info = manager
        .create("gated", None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    // The session tells the window that the gate watches it.
    assert_eq!(info.gate, GateStatus::Active);
    let (output, _) = collect_until_exit(&manager, info.id, &rx);
    assert!(output.contains("TOKEN=gate-token-marker"), "{output:?}");
    assert!(output.contains("gate-arg-marker"), "{output:?}");
    wait_for_status(&manager, info.id, Status::Exited);
    assert!(gate.ended.lock().unwrap().contains(&info.id));

    // A restart asks the gate again.
    let (tx, rx) = channel();
    manager
        .restart(info.id, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    collect_until_exit(&manager, info.id, &rx);
    assert_eq!(
        gate.prepared
            .lock()
            .unwrap()
            .iter()
            .filter(|id| **id == info.id)
            .count(),
        2
    );

    // Without the gate's answer, the profile's own variable stays.
    let (tx, rx) = channel();
    let plain = manager
        .create("plain", None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    assert_eq!(plain.gate, GateStatus::None);
    let (output, _) = collect_until_exit(&manager, plain.id, &rx);
    assert!(output.contains("TOKEN=profile-value"), "{output:?}");
    assert!(!output.contains("gate-arg-marker"), "{output:?}");

    // A program that could take the gate but does not get it says why.
    let (tx, rx) = channel();
    let ungated = manager
        .create("ungated", None, SIZE, move |e| tx.send(e).unwrap_or(()))
        .unwrap();
    assert_eq!(ungated.gate, GateStatus::Launcher);
    collect_until_exit(&manager, ungated.id, &rx);

    // Closing tells the gate as well.
    manager.close(plain.id).unwrap();
    assert!(gate.ended.lock().unwrap().contains(&plain.id));
}

#[test]
fn a_profile_with_its_own_settings_gets_no_second_one() {
    // Both spellings Claude Code accepts; the probe prints every argument after its own.
    let spaced = with_args(gate_probe("own-settings"), &["--settings", "user.json"]);
    let joined = {
        let mut probe = with_args(gate_probe("own-settings"), &["--settings=user.json"]);
        probe.id = "own-settings-joined".into();
        probe
    };
    let manager = SessionManager::new(vec![spaced, joined], Vec::new(), std::env::temp_dir());
    let gate = Arc::new(RecordingGate::default());
    manager.set_launch_gate(gate.clone());

    for id in ["own-settings", "own-settings-joined"] {
        let (tx, rx) = channel();
        let info = manager
            .create(id, None, SIZE, move |e| tx.send(e).unwrap_or(()))
            .unwrap();
        assert_eq!(info.gate, GateStatus::ProfileSettings, "{id}");
        let (output, _) = collect_until_exit(&manager, info.id, &rx);
        // Neither the gate's argument nor its token reach the child; the profile's value stays.
        assert!(!output.contains("gate-arg-marker"), "{id}: {output:?}");
        assert!(output.contains("TOKEN=profile-value"), "{id}: {output:?}");
        assert!(output.contains("user.json"), "{id}: {output:?}");
        // Nothing was registered for this session.
        assert!(!gate.prepared.lock().unwrap().contains(&info.id), "{id}");
    }
}

/// Appends `extra` to the probe's arguments so the probe prints them (on Unix `sh -c` takes the
/// first one as `$0`, so the placeholder goes).
fn with_args(mut probe: Profile, extra: &[&str]) -> Profile {
    if cfg!(unix) {
        probe.args.pop();
        let script = probe.args.pop().unwrap_or_default();
        probe.args.push(script.replace("ARGS=$0", "ARGS=\"$0 $*\""));
    }
    probe.args.extend(extra.iter().map(|arg| (*arg).to_owned()));
    probe
}

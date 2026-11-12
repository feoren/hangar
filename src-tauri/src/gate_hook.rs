//! `hangar gate-hook <agent> <event>`: the agent gate's command hook for agent CLIs without HTTP
//! hooks (Gemini CLI, Copilot CLI, Codex CLI; recipes in `hook_recipes.rs`).
//!
//! The agent starts the Hangar executable in this mode with the hook's JSON on stdin. The helper
//! reads the paths the tool is about to change, asks the running Hangar's endpoint
//! (`gate_service.rs`, the same request Claude Code sends) once per path and prints the answer in
//! the agent's own format: nothing for "go ahead", a deny as JSON on stdout (Codex and Copilot
//! CLI also get the reason on stderr and exit code 2; Gemini CLI exit 0, see [`reply`]), which
//! each of the three reads as a block. Everything that goes wrong
//! (no Hangar, the session ended, malformed input) lets the edit through, as a failing HTTP hook
//! does for Claude Code: the gate coordinates the user's own sessions, it is not a security
//! boundary.
//!
//! The helper never starts the app: `main.rs` branches here before Tauri, the single instance
//! lock or any window. On Windows the release build is a GUI-subsystem program; measured on
//! Windows 11 with such a program, cmd.exe (`/C`, Codex) and PowerShell (`-Command`, Gemini CLI)
//! wait for it and pass stdin and stdout, but PowerShell reports exit code 0 instead of 2 (1 for
//! a console build), so the deny is on stdout as JSON (docs/wiki/pages/syntheses/agent-gate-adapters.md).

use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::gate::{DECISION_TIMEOUT, MAX_FIELD_BYTES, ROUTE_PRE_TOOL_USE};
use crate::hook_recipes::{
    AGENT_CODEX, AGENT_COPILOT, AGENT_GEMINI, EVENT_BEFORE_TOOL, EVENT_PRE_TOOL_USE, HELPER_ENV,
};

/// How long the helper waits in total, below the hooks' own limit (660 s,
/// [`crate::gate::HOOK_TIMEOUT_SECS`]) and above Hangar's decision time, so Hangar's answer
/// always comes first and the agent never runs into its own timeout.
pub const DEADLINE: Duration = Duration::from_secs(DECISION_TIMEOUT.as_secs() + 30);
/// Largest hook input read; a bigger one is let through unread.
const MAX_INPUT: u64 = 16 * 1024 * 1024;
/// Largest answer read from the endpoint.
const MAX_ANSWER: u64 = 1024 * 1024;
/// Paths of one tool call asked about; a patch touching more checks the first ones.
pub const MAX_PATHS: usize = 64;
/// Connecting and sending: the endpoint answers at once unless a person decides.
const CONNECT: Duration = Duration::from_secs(5);

/// The agent CLIs the helper speaks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookAgent {
    Gemini,
    Copilot,
    Codex,
}

impl HookAgent {
    /// The agent and event as the hook command names them; `None` for anything else.
    pub fn parse(agent: &str, event: &str) -> Option<Self> {
        match (agent, event) {
            (AGENT_GEMINI, EVENT_BEFORE_TOOL) => Some(HookAgent::Gemini),
            (AGENT_COPILOT, EVENT_PRE_TOOL_USE) => Some(HookAgent::Copilot),
            (AGENT_CODEX, EVENT_PRE_TOOL_USE) => Some(HookAgent::Codex),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HelperError {
    #[error(
        "usage: hangar gate-hook <gemini before-tool | copilot pre-tool-use | codex pre-tool-use>"
    )]
    Usage,
    #[error("the hook input could not be read: {0}")]
    Read(#[source] io::Error),
    #[error("the hook input is larger than the helper reads")]
    TooLarge,
    #[error("the hook input is not JSON of the expected shape")]
    Shape,
    #[error("{HELPER_ENV} is missing or malformed; the session was not started by Hangar's gate")]
    Env,
    #[error("Hangar's gate endpoint is not reachable: {0}")]
    Connect(#[source] io::Error),
    #[error("Hangar's gate endpoint answered with status {0}")]
    Status(u16),
    #[error("Hangar's gate endpoint sent an answer the helper cannot read")]
    Answer,
}

/// One tool call as the helper reads it from an agent's hook input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub tool: String,
    /// As the agent named them: absolute or relative to `cwd`, in either separator style.
    pub paths: Vec<String>,
    pub cwd: Option<String>,
}

/// What the gate said about the whole call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    /// Hangar cannot hold one more edit and hands the question to the agent's own prompt.
    Ask(String),
    Deny(String),
}

/// What the helper writes and returns for a verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

/// Exit code all three agents read as "block this tool call".
pub const DENY_CODE: i32 = 2;

/// Lines of an `apply_patch` text that name a file (Codex `apply-patch/src/parser.rs`): each one
/// is a file the patch adds, changes, deletes or moves to.
const PATCH_MARKERS: &[&str] = &[
    "*** Add File: ",
    "*** Update File: ",
    "*** Delete File: ",
    "*** Move to: ",
];

/// Every file a patch touches, in order and without repeats.
/// Every file a patch touches, in order and without repeats; at most `MAX_PATHS + 1`, which is
/// enough to know the call names too many.
pub fn patch_paths(text: &str) -> Vec<String> {
    let mut paths = Paths::default();
    paths.add_patch(text);
    paths.list
}

/// Paths in order without repeats, collected up to one more than [`MAX_PATHS`].
#[derive(Default)]
struct Paths {
    list: Vec<String>,
    seen: HashSet<String>,
}

impl Paths {
    fn full(&self) -> bool {
        self.list.len() > MAX_PATHS
    }

    fn add(&mut self, path: &str) {
        let path = path.trim();
        if !self.full() && !path.is_empty() && self.seen.insert(path.to_owned()) {
            self.list.push(path.to_owned());
        }
    }

    fn add_patch(&mut self, text: &str) {
        for line in text.lines() {
            if self.full() {
                return;
            }
            let line = line.trim();
            if let Some(path) = PATCH_MARKERS
                .iter()
                .find_map(|marker| line.strip_prefix(marker.trim_end()))
            {
                self.add(path);
            }
        }
    }
}

/// Keys the agents use for the file of a write or edit tool.
const PATH_KEYS: &[&str] = &["file_path", "path", "filePath", "absolute_path"];

/// The files named in a tool's arguments: a path field, or patch text in any string field (or
/// the arguments as a whole). Copilot's `toolArgs` may also come as a JSON string.
fn argument_paths(args: &Value) -> Vec<String> {
    let mut paths = Paths::default();
    collect_arguments(args, &mut paths);
    paths.list
}

fn collect_arguments(args: &Value, paths: &mut Paths) {
    match args {
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(inner @ Value::Object(_)) => collect_arguments(&inner, paths),
            _ => paths.add_patch(text),
        },
        Value::Object(fields) => {
            for key in PATH_KEYS {
                if let Some(Value::String(path)) = fields.get(*key) {
                    paths.add(path);
                }
            }
            for value in fields.values() {
                if let Value::String(text) = value {
                    if text.contains("*** Begin Patch") {
                        paths.add_patch(text);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Why a call that names more files than the helper asks about does not simply go ahead.
pub fn too_many_reason() -> String {
    format!(
        "Hangar: this edit touches more than {MAX_PATHS} files, more than the gate checks one by \
         one. Split it into smaller edits, or ask the user to confirm."
    )
}

fn string_field(input: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| input.get(*key).and_then(Value::as_str))
        .map(str::to_owned)
}

/// Reads one agent's hook input. Gemini CLI and Codex send `tool_name`, `tool_input` and `cwd`
/// (Codex's `apply_patch` input is `{"command": "<patch>"}`); Copilot CLI sends `toolName`,
/// `toolArgs` (object or JSON string) and `cwd`, or the snake case shape for its PascalCase events.
pub fn parse_call(agent: HookAgent, input: &[u8]) -> Result<ToolCall, HelperError> {
    let json: Value = serde_json::from_slice(input).map_err(|_| HelperError::Shape)?;
    if !json.is_object() {
        return Err(HelperError::Shape);
    }
    let (tool_keys, args_keys): (&[&str], &[&str]) = match agent {
        HookAgent::Copilot => (&["toolName", "tool_name"], &["toolArgs", "tool_input"]),
        HookAgent::Gemini | HookAgent::Codex => (&["tool_name"], &["tool_input"]),
    };
    let tool = string_field(&json, tool_keys).ok_or(HelperError::Shape)?;
    let args = args_keys
        .iter()
        .find_map(|key| json.get(*key))
        .unwrap_or(&Value::Null);
    Ok(ToolCall {
        tool,
        paths: argument_paths(args),
        cwd: string_field(&json, &["cwd"]),
    })
}

/// The request Claude Code's HTTP hook sends, which the endpoint already reads.
pub fn gate_request(tool: &str, path: &str, cwd: Option<&str>) -> String {
    serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": tool,
        "tool_input": { "file_path": path },
        "cwd": cwd,
    })
    .to_string()
}

/// The endpoint's answer body: empty is no decision, otherwise Claude Code's
/// `hookSpecificOutput`.
pub fn read_verdict(body: &str) -> Result<Verdict, HelperError> {
    if body.trim().is_empty() {
        return Ok(Verdict::Allow);
    }
    let json: Value = serde_json::from_str(body).map_err(|_| HelperError::Answer)?;
    let output = &json["hookSpecificOutput"];
    let reason = output["permissionDecisionReason"]
        .as_str()
        .unwrap_or("Hangar: another session is changing this file.")
        .to_owned();
    Ok(match output["permissionDecision"].as_str() {
        Some("deny") => Verdict::Deny(reason),
        Some("ask") => Verdict::Ask(reason),
        _ => Verdict::Allow,
    })
}

/// The verdict in the agent's own words. Gemini CLI and Codex have no usable "ask" (Codex fails
/// open on it), so for them Hangar's "please confirm" becomes a deny that says so.
pub fn reply(agent: HookAgent, verdict: &Verdict) -> Reply {
    let (reason, ask) = match verdict {
        Verdict::Allow => {
            return Reply {
                stdout: String::new(),
                stderr: String::new(),
                code: 0,
            }
        }
        Verdict::Ask(reason) => (reason, true),
        Verdict::Deny(reason) => (reason, false),
    };
    if ask && agent == HookAgent::Copilot {
        return Reply {
            stdout: serde_json::json!({
                "permissionDecision": "ask",
                "permissionDecisionReason": reason,
            })
            .to_string(),
            stderr: String::new(),
            code: 0,
        };
    }
    let json_only = |stdout: Value| Reply {
        stdout: stdout.to_string(),
        stderr: String::new(),
        code: 0,
    };
    match agent {
        // Gemini CLI runs the helper through PowerShell, which turns a native exit code 2 into 1
        // (a console build) or 0 (the GUI-subsystem release build); 1 is a warning and the tool
        // would proceed. Exit 0 with the JSON deny reads the same through every start.
        HookAgent::Gemini => json_only(serde_json::json!({ "decision": "deny", "reason": reason })),
        // Measured live with Codex 0.167.2: this JSON with exit 0 blocks the patch; the same JSON
        // with exit 2 and the reason on stderr counts as a failed hook and the patch goes ahead
        // (docs/wiki/pages/sources/codex-gate-live-measurement.md).
        HookAgent::Codex => json_only(serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            }
        })),
        // Copilot CLI reads exit 2 as a deny even when stdout says otherwise (docs, not measured).
        HookAgent::Copilot => Reply {
            stdout: serde_json::json!({
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            })
            .to_string(),
            stderr: reason.clone(),
            code: DENY_CODE,
        },
    }
}

/// `<port>:<token>` from [`HELPER_ENV`].
fn endpoint(value: Option<&str>) -> Result<(u16, &str), HelperError> {
    let (port, token) = value
        .and_then(|v| v.split_once(':'))
        .ok_or(HelperError::Env)?;
    let port: u16 = port.parse().map_err(|_| HelperError::Env)?;
    if port == 0 || token.is_empty() || !token.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(HelperError::Env);
    }
    Ok((port, token))
}

/// One POST to the endpoint on 127.0.0.1; the answer's status and body. Waits until `deadline`
/// at most; a held decision keeps the connection open until the person answers.
fn post(
    port: u16,
    token: &str,
    body: &str,
    deadline: Instant,
) -> Result<(u16, String), HelperError> {
    let left = || deadline.saturating_duration_since(Instant::now());
    let timed_out = || HelperError::Connect(io::ErrorKind::TimedOut.into());
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream =
        TcpStream::connect_timeout(&address, CONNECT.min(left()).max(Duration::from_millis(1)))
            .map_err(HelperError::Connect)?;
    stream
        .set_write_timeout(Some(CONNECT))
        .map_err(HelperError::Connect)?;
    let request = format!(
        "POST {ROUTE_PRE_TOOL_USE} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
         Authorization: Bearer {token}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.flush())
        .map_err(HelperError::Connect)?;
    let mut answer = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let rest = left();
        if rest.is_zero() {
            return Err(timed_out());
        }
        stream
            .set_read_timeout(Some(rest))
            .map_err(HelperError::Connect)?;
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                answer.extend_from_slice(&buf[..n]);
                if answer.len() as u64 > MAX_ANSWER {
                    return Err(HelperError::Answer);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(timed_out())
            }
            // Windows may reset after the server's close; what arrived is the whole answer.
            Err(_) if !answer.is_empty() => break,
            Err(error) => return Err(HelperError::Connect(error)),
        }
    }
    let text = String::from_utf8(answer).map_err(|_| HelperError::Answer)?;
    let (head, body) = text.split_once("\r\n\r\n").ok_or(HelperError::Answer)?;
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or(HelperError::Answer)?;
    Ok((status, body.to_owned()))
}

/// Asks the gate about every path of `call`, one after the other; the first deny or ask decides.
/// A path the endpoint cannot answer for counts as allowed (reported on `notes`).
fn decide(
    call: &ToolCall,
    port: u16,
    token: &str,
    deadline: Instant,
    notes: &mut impl Write,
) -> Verdict {
    for path in call.paths.iter().take(MAX_PATHS) {
        if path.len() > MAX_FIELD_BYTES || path.contains('\0') {
            continue;
        }
        let body = gate_request(&call.tool, path, call.cwd.as_deref());
        let verdict = post(port, token, &body, deadline).and_then(|(status, body)| {
            if status == 200 {
                read_verdict(&body)
            } else {
                Err(HelperError::Status(status))
            }
        });
        match verdict {
            Ok(Verdict::Allow) => {}
            Ok(verdict) => return verdict,
            Err(error) => {
                let _ = writeln!(notes, "hangar gate-hook: {error}; the edit goes ahead");
            }
        }
    }
    Verdict::Allow
}

/// The whole helper, with its streams and environment passed in: `args` after `gate-hook`.
pub fn run(
    args: &[OsString],
    input: impl Read + Send + 'static,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    gate: Option<&str>,
) -> i32 {
    match run_inner(args, input, stderr, gate) {
        Ok(reply) => {
            let _ = stdout.write_all(reply.stdout.as_bytes());
            let _ = stdout.flush();
            if !reply.stderr.is_empty() {
                let _ = stderr.write_all(reply.stderr.as_bytes());
            }
            let _ = stderr.flush();
            reply.code
        }
        Err(error) => {
            let _ = writeln!(stderr, "hangar gate-hook: {error}; the edit goes ahead");
            0
        }
    }
}

fn run_inner(
    args: &[OsString],
    input: impl Read + Send + 'static,
    notes: &mut impl Write,
    gate: Option<&str>,
) -> Result<Reply, HelperError> {
    let deadline = Instant::now() + DEADLINE;
    let [agent, event] = args else {
        return Err(HelperError::Usage);
    };
    let agent = HookAgent::parse(
        agent.to_str().ok_or(HelperError::Usage)?,
        event.to_str().ok_or(HelperError::Usage)?,
    )
    .ok_or(HelperError::Usage)?;
    let text = read_input(input, DEADLINE)?;
    let call = parse_call(agent, &text)?;
    if call.paths.is_empty() {
        return Ok(reply(agent, &Verdict::Allow));
    }
    if call.paths.len() > MAX_PATHS {
        return Ok(reply(agent, &Verdict::Ask(too_many_reason())));
    }
    let (port, token) = endpoint(gate)?;
    let verdict = decide(&call, port, token, deadline, notes);
    Ok(reply(agent, &verdict))
}

/// Reads the hook input on a thread of its own, so an agent that never closes stdin cannot
/// keep the helper past `limit`; the thread is left behind when the process exits.
fn read_input(input: impl Read + Send + 'static, limit: Duration) -> Result<Vec<u8>, HelperError> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("gate-hook-stdin".to_owned())
        .spawn(move || {
            let mut text = Vec::new();
            let read = input
                .take(MAX_INPUT + 1)
                .read_to_end(&mut text)
                .map(|_| text);
            let _ = tx.send(read);
        })
        .map_err(HelperError::Read)?;
    let text = rx
        .recv_timeout(limit)
        .map_err(|_| HelperError::Read(io::ErrorKind::TimedOut.into()))?
        .map_err(HelperError::Read)?;
    if text.len() as u64 > MAX_INPUT {
        return Err(HelperError::TooLarge);
    }
    Ok(text)
}

/// Entry from `main.rs` with the process's own streams and environment.
pub fn main(args: &[OsString]) -> i32 {
    let gate = std::env::var(HELPER_ENV).ok();
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    run(args, io::stdin(), &mut stdout, &mut stderr, gate.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "*** Begin Patch\n\
        *** Add File: src/new.rs\n+fn main() {}\n\
        *** Update File: C:\\repo\\src\\lib.rs\n*** Move to: src/moved.rs\n@@\n-a\n+b\n\
        *** Delete File: docs/old.md\n\
        *** Update File: src/new.rs\n@@\n-x\n+y\n\
        *** End Patch\n";

    #[test]
    fn a_patch_names_every_file_once_in_both_separator_styles() {
        assert_eq!(
            patch_paths(PATCH),
            [
                "src/new.rs",
                "C:\\repo\\src\\lib.rs",
                "src/moved.rs",
                "docs/old.md"
            ]
        );
        // CRLF line ends and indentation do not hide a file.
        let crlf = "*** Begin Patch\r\n  *** Update File: /abs/a.rs\r\n*** End Patch\r\n";
        assert_eq!(patch_paths(crlf), ["/abs/a.rs"]);
        // Content lines that merely mention a marker are not markers.
        assert!(patch_paths("+*** Add File: fake.rs\n").is_empty());
        assert!(patch_paths("no patch at all").is_empty());
    }

    #[test]
    fn codex_input_is_read_from_the_patch() {
        let input = serde_json::json!({
            "session_id": "s", "cwd": "C:\\repo", "hook_event_name": "PreToolUse",
            "tool_name": "apply_patch", "tool_use_id": "t",
            "tool_input": { "command": PATCH },
        });
        let call = parse_call(HookAgent::Codex, input.to_string().as_bytes()).unwrap();
        assert_eq!(call.tool, "apply_patch");
        assert_eq!(call.cwd.as_deref(), Some("C:\\repo"));
        assert_eq!(call.paths.len(), 4);
    }

    #[test]
    fn gemini_input_names_its_file() {
        for tool in ["write_file", "replace"] {
            let input = serde_json::json!({
                "session_id": "s", "cwd": "/repo", "hook_event_name": "BeforeTool",
                "timestamp": "x", "tool_name": tool,
                "tool_input": { "file_path": "src/a.rs", "content": "secret body" },
            });
            let call = parse_call(HookAgent::Gemini, input.to_string().as_bytes()).unwrap();
            assert_eq!(call.tool, tool);
            assert_eq!(call.paths, ["src/a.rs"]);
            assert_eq!(call.cwd.as_deref(), Some("/repo"));
        }
    }

    #[test]
    fn copilot_arguments_come_as_object_or_string() {
        let object = serde_json::json!({
            "sessionId": "s", "timestamp": 1, "cwd": "/repo", "toolName": "edit",
            "toolArgs": { "path": "/repo/a.rs", "old_str": "x", "new_str": "y" },
        });
        let call = parse_call(HookAgent::Copilot, object.to_string().as_bytes()).unwrap();
        assert_eq!(
            (call.tool.as_str(), call.paths.clone()),
            ("edit", vec!["/repo/a.rs".to_owned()])
        );
        let string = serde_json::json!({
            "sessionId": "s", "cwd": "/repo", "toolName": "create",
            "toolArgs": "{\"file_path\":\"b.rs\",\"file_text\":\"x\"}",
        });
        let call = parse_call(HookAgent::Copilot, string.to_string().as_bytes()).unwrap();
        assert_eq!(call.paths, ["b.rs"]);
        // apply_patch with the patch as the whole argument or in a field.
        for args in [
            serde_json::json!(PATCH),
            serde_json::json!({ "input": PATCH }),
        ] {
            let input =
                serde_json::json!({ "cwd": "/repo", "toolName": "apply_patch", "toolArgs": args });
            let call = parse_call(HookAgent::Copilot, input.to_string().as_bytes()).unwrap();
            assert_eq!(call.paths.len(), 4, "{args}");
        }
        // The PascalCase event's shape works too.
        let pascal = serde_json::json!({
            "hook_event_name": "PreToolUse", "cwd": "/repo", "tool_name": "Edit",
            "tool_input": { "file_path": "/repo/c.rs" },
        });
        let call = parse_call(HookAgent::Copilot, pascal.to_string().as_bytes()).unwrap();
        assert_eq!(call.paths, ["/repo/c.rs"]);
    }

    #[test]
    fn malformed_input_is_an_error_not_a_panic() {
        for input in [
            "",
            "[]",
            "{\"tool_input\":{}}",
            "not json",
            "{\"tool_name\":3}",
        ] {
            assert!(
                parse_call(HookAgent::Codex, input.as_bytes()).is_err(),
                "{input}"
            );
        }
    }

    #[test]
    fn each_agent_gets_its_own_deny() {
        let deny = Verdict::Deny("Hangar: no".into());
        // Gemini: exit 0 with the JSON deny, as PowerShell does not pass exit code 2 on.
        let gemini = reply(HookAgent::Gemini, &deny);
        assert_eq!((gemini.code, gemini.stderr.as_str()), (0, ""));
        let json: Value = serde_json::from_str(&gemini.stdout).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "decision": "deny", "reason": "Hangar: no" })
        );

        let copilot = reply(HookAgent::Copilot, &deny);
        let json: Value = serde_json::from_str(&copilot.stdout).unwrap();
        assert_eq!(json["permissionDecision"], "deny");
        assert_eq!(json["permissionDecisionReason"], "Hangar: no");
        assert_eq!(copilot.code, 2);

        let codex = reply(HookAgent::Codex, &deny);
        let json: Value = serde_json::from_str(&codex.stdout).unwrap();
        assert_eq!(json["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
        // Codex: exit 0 with the JSON deny, measured live; exit 2 counts as a failed hook.
        assert_eq!((codex.code, codex.stderr.as_str()), (0, ""));

        // "Ask" stays a question only where the agent can ask.
        let ask = Verdict::Ask("confirm".into());
        let copilot = reply(HookAgent::Copilot, &ask);
        assert_eq!(copilot.code, 0);
        assert!(copilot.stdout.contains("\"ask\""));
        for (agent, code) in [(HookAgent::Gemini, 0), (HookAgent::Codex, 0)] {
            let answer = reply(agent, &ask);
            assert_eq!(answer.code, code);
            assert!(answer.stdout.contains("deny"));
        }
        for agent in [HookAgent::Gemini, HookAgent::Copilot, HookAgent::Codex] {
            let allow = reply(agent, &Verdict::Allow);
            assert_eq!((allow.code, allow.stdout.as_str()), (0, ""));
        }
    }

    #[test]
    fn endpoint_answers_are_read() {
        assert_eq!(read_verdict("").unwrap(), Verdict::Allow);
        assert_eq!(read_verdict("{}").unwrap(), Verdict::Allow);
        let deny = crate::gate::Outcome::Deny("r".into()).body();
        assert_eq!(read_verdict(&deny).unwrap(), Verdict::Deny("r".into()));
        let ask = crate::gate::Outcome::Ask("q".into()).body();
        assert_eq!(read_verdict(&ask).unwrap(), Verdict::Ask("q".into()));
        assert!(read_verdict("<html>").is_err());
    }

    #[test]
    fn the_gate_variable_needs_port_and_plain_token() {
        assert_eq!(endpoint(Some("4711:abc123")).unwrap(), (4711, "abc123"));
        for bad in [
            None,
            Some(""),
            Some("4711"),
            Some("0:abc"),
            Some("x:abc"),
            Some("4711:"),
            Some("4711:a\r\nX: y"),
        ] {
            assert!(endpoint(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_request_is_claude_codes_shape() {
        let body = gate_request("apply_patch", "src/a.rs", Some("/repo"));
        let parsed = crate::gate::parse_pre_tool_use(body.as_bytes()).unwrap();
        assert_eq!(parsed.tool, "apply_patch");
        assert_eq!(parsed.path.as_deref(), Some("src/a.rs"));
        assert_eq!(parsed.cwd.as_deref(), Some("/repo"));
    }

    fn os(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn anything_wrong_lets_the_edit_through_with_a_note() {
        let input = serde_json::json!({
            "cwd": "/repo", "tool_name": "write_file", "tool_input": { "file_path": "a.rs" },
        })
        .to_string();
        let run_with = |args: &[&str], gate: Option<&str>, input: &str| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let input = std::io::Cursor::new(input.as_bytes().to_vec());
            let code = run(&os(args), input, &mut out, &mut err, gate);
            (
                code,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        };
        // Unknown agent or event, no variable, nothing listening, broken input.
        let (code, out, err) = run_with(&["gemini", "after-tool"], Some("1:a"), &input);
        assert_eq!((code, out.as_str()), (0, ""));
        assert!(err.contains("usage"), "{err}");
        let (code, _, err) = run_with(&["gemini", "before-tool"], None, &input);
        assert_eq!(code, 0);
        assert!(err.contains("HANGAR_GATE"), "{err}");
        let closed = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let (code, out, err) = run_with(
            &["gemini", "before-tool"],
            Some(&format!("{port}:abc")),
            &input,
        );
        assert_eq!((code, out.as_str()), (0, ""));
        assert!(err.contains("not reachable"), "{err}");
        let (code, _, _) = run_with(&["codex", "pre-tool-use"], Some("1:a"), "{");
        assert_eq!(code, 0);
        // A call without a file needs no endpoint at all.
        let none =
            serde_json::json!({ "tool_name": "apply_patch", "tool_input": { "command": "x" } });
        let (code, out, err) = run_with(&["codex", "pre-tool-use"], None, &none.to_string());
        assert_eq!((code, out.as_str(), err.as_str()), (0, "", ""));
    }

    #[test]
    fn a_huge_patch_is_cut_quickly_and_not_let_through() {
        let mut text = String::from("*** Begin Patch\n");
        for n in 0..10_000 {
            text.push_str(&format!("*** Add File: src/f{n}.rs\n+x\n"));
        }
        text.push_str("*** End Patch\n");
        let started = Instant::now();
        let paths = patch_paths(&text);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(paths.len(), MAX_PATHS + 1);
        // Repeats count once.
        let same = "*** Update File: a.rs\n".repeat(10_000);
        assert_eq!(patch_paths(&same), ["a.rs"]);
        // More files than the gate checks: no endpoint asked, and no silent "go ahead".
        let input = serde_json::json!({
            "cwd": "/repo", "tool_name": "apply_patch", "tool_input": { "command": text },
        })
        .to_string();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(
            &os(&["codex", "pre-tool-use"]),
            std::io::Cursor::new(input.into_bytes()),
            &mut out,
            &mut err,
            None,
        );
        assert_eq!(code, 0);
        let json: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
        assert!(json["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .contains("more than 64 files"));
    }

    /// A reader that never ends, like an agent that keeps stdin open.
    struct Endless;

    impl Read for Endless {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            std::thread::sleep(Duration::from_secs(3600));
            Ok(0)
        }
    }

    #[test]
    fn stdin_that_never_ends_is_given_up_on() {
        let started = Instant::now();
        let read = read_input(Endless, Duration::from_millis(200));
        assert!(matches!(read, Err(HelperError::Read(_))), "{read:?}");
        assert!(started.elapsed() < Duration::from_secs(5));
        let read = read_input(std::io::Cursor::new(b"{}".to_vec()), Duration::from_secs(5));
        assert_eq!(read.unwrap(), b"{}");
    }
}

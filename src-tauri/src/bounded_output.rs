//! The output of a short system tool (`pmset`, `ioreg` on macOS) within a time limit: a tool that
//! hangs must not hold up the power thread or a GPU reading. Started as an argument list, stdin
//! and stderr closed; past the limit the tool is killed and nothing is returned.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// How long the system tools get.
pub const TOOL_LIMIT: Duration = Duration::from_secs(2);
/// Output read at most; a tool that prints more is cut off and runs into the time limit.
const MAX_OUTPUT: u64 = 4 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(10);

/// Standard output of `program` with `args` when it ends successfully within `limit`; `None`
/// when it cannot start, fails, or takes longer (then it is killed).
pub fn output_within(program: &str, args: &[&str], limit: Duration) -> Option<Vec<u8>> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + limit;
    let (sender, receiver) = mpsc::channel();
    // Read on a thread of its own, so a tool that fills the pipe cannot block while this one
    // waits for it to end.
    let reader = child.stdout.take().and_then(|stdout| {
        thread::Builder::new()
            .name("tool-output".into())
            .spawn(move || {
                let mut output = Vec::new();
                let read = stdout.take(MAX_OUTPUT).read_to_end(&mut output);
                let _ = sender.send(read.map(|_| output));
            })
            .ok()
    });
    if reader.is_none() {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                // The tool has ended; its output follows at once unless a process it started
                // still holds the pipe.
                let left = deadline.saturating_duration_since(Instant::now());
                return receiver.recv_timeout(left.max(POLL)).ok()?.ok();
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quick_tool_gives_its_output() {
        let output = if cfg!(windows) {
            output_within("cmd", &["/C", "echo hangar-marker"], TOOL_LIMIT)
        } else {
            output_within("/bin/echo", &["hangar-marker"], TOOL_LIMIT)
        };
        let text = String::from_utf8(output.expect("output")).unwrap();
        assert!(text.contains("hangar-marker"), "{text}");
    }

    #[test]
    fn a_failing_or_missing_tool_gives_nothing() {
        let failed = if cfg!(windows) {
            output_within("cmd", &["/C", "exit 3"], TOOL_LIMIT)
        } else {
            output_within("/bin/sh", &["-c", "exit 3"], TOOL_LIMIT)
        };
        assert_eq!(failed, None);
        assert_eq!(output_within("hangar-no-such-tool", &[], TOOL_LIMIT), None);
    }

    #[test]
    fn a_slow_tool_is_killed_at_the_limit() {
        let started = Instant::now();
        let output = if cfg!(windows) {
            output_within(
                "ping",
                &["-n", "10", "127.0.0.1"],
                Duration::from_millis(300),
            )
        } else {
            output_within("/bin/sleep", &["10"], Duration::from_millis(300))
        };
        assert_eq!(output, None);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
    }
}

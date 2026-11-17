//! `hangar-send`: sends a cross-project message (ADR 0012) from an agent session in Hangar to an
//! agent session of a partner project. Hangar puts it on the `PATH` of sessions whose project has
//! partners; the message then waits in Hangar until the person there pastes or discards it.
//!
//! ```text
//! hangar-send <partner> <message...>
//! hangar-send <partner> < message.txt
//! ```
//!
//! It posts to the send route of Hangar's endpoint on 127.0.0.1, with the port and token from the
//! session's environment (`HANGAR_SEND=<port>:<token>`). Hangar checks everything (partnership,
//! length, control characters); this program only carries the text and reports the answer.
//!
//! Its own binary, not a mode of `hangar`, so it stays small (the standard library and
//! serde_json, none of Hangar's code) and has a name no shell alias shadows. The names it shares
//! with Hangar (`partners.rs`: `SEND_ENV`, `ROUTE_SEND`, `MAX_SEND_BODY_BYTES`) are repeated here
//! on purpose; an integration test (`tests/send.rs`) runs this binary against the real endpoint.

use std::io::{self, IsTerminal, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::process::ExitCode;
use std::time::Duration;

const SEND_ENV: &str = "HANGAR_SEND";
const ROUTE: &str = "/v1/send";
/// The endpoint refuses larger bodies.
const MAX_BODY_BYTES: usize = 256 * 1024;
/// Hangar's answers are one sentence.
const MAX_ANSWER_BYTES: u64 = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);

const USAGE: &str = "\
Usage: hangar-send <partner> <message...>
       hangar-send <partner> < file

Sends a message to an agent session of a partner project in Hangar. The message waits
there until the person pastes or discards it. Partners are the short names Hangar lists
for this project.

Exit codes: 0 waiting, 1 failed, 2 usage, 3 no Hangar session, 4 unknown partner,
5 too many waiting messages, 6 message refused.";

/// The exit codes, documented in [`USAGE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exit {
    Waiting = 0,
    Failed = 1,
    Usage = 2,
    NoHangar = 3,
    NotPartner = 4,
    Full = 5,
    Refused = 6,
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    /// `text: None`: read it from stdin.
    Send {
        partner: String,
        text: Option<String>,
    },
}

/// The arguments after the program name. `--` ends the options, so a message may start with a
/// dash; the words of the message are joined with single blanks.
fn parse_args(args: &[String]) -> Result<Command, String> {
    let mut words = Vec::new();
    let mut options = true;
    for arg in args {
        if options && arg == "--" {
            options = false;
        } else if options && (arg == "-h" || arg == "--help") {
            return Ok(Command::Help);
        } else if options && arg.starts_with('-') && arg.len() > 1 && words.is_empty() {
            return Err(format!("unknown option {arg}"));
        } else {
            words.push(arg.as_str());
        }
    }
    let Some((partner, text)) = words.split_first() else {
        return Err("name the partner project".to_owned());
    };
    Ok(Command::Send {
        partner: (*partner).to_owned(),
        text: (!text.is_empty()).then(|| text.join(" ")),
    })
}

/// Text from stdin: UTF-8 with or without a byte order mark, or UTF-16 with one (what Windows
/// PowerShell 5 may pipe to a program).
fn decode(bytes: &[u8]) -> Result<String, String> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(rest.to_vec()).map_err(|_| "the message is not UTF-8".into());
    }
    let utf16 = |rest: &[u8], little: bool| {
        let (pairs, odd) = rest.as_chunks::<2>();
        if !odd.is_empty() {
            return Err("the message is not UTF-16".to_owned());
        }
        let units: Vec<u16> = pairs
            .iter()
            .map(|pair| {
                if little {
                    u16::from_le_bytes(*pair)
                } else {
                    u16::from_be_bytes(*pair)
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|_| "the message is not UTF-16".to_owned())
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| "the message is not UTF-8".into())
}

/// `<port>:<token>` of [`SEND_ENV`].
fn parse_endpoint(value: &str) -> Option<(u16, String)> {
    let (port, token) = value.split_once(':')?;
    let port: u16 = port.parse().ok().filter(|port| *port != 0)?;
    let visible = !token.is_empty() && token.bytes().all(|b| b.is_ascii_alphanumeric());
    visible.then(|| (port, token.to_owned()))
}

fn request(port: u16, token: &str, body: &str) -> String {
    format!(
        "POST {ROUTE} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Status code, and `code` and `message` of the JSON body when there is one.
fn parse_response(raw: &[u8]) -> Option<(u16, Option<(String, String)>)> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n")?;
    let status: u16 = head.split(' ').nth(1)?.parse().ok()?;
    let answer = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            let code = value.get("code")?.as_str()?.to_owned();
            let message = value.get("message")?.as_str()?.to_owned();
            Some((code, message))
        });
    Some((status, answer))
}

/// What the answer means for the agent: the exit code and the sentence to print.
fn outcome(status: u16, answer: Option<(String, String)>) -> (Exit, String) {
    if let Some((code, message)) = answer {
        let exit = match code.as_str() {
            "waiting" => Exit::Waiting,
            "not_partner" => Exit::NotPartner,
            "full" => Exit::Full,
            "refused" => Exit::Refused,
            "no_session" | "not_agent" => Exit::NoHangar,
            _ => Exit::Failed,
        };
        return (exit, message);
    }
    match status {
        401 | 403 => (
            Exit::NoHangar,
            "Hangar does not know this session's link any more; restart the session in Hangar."
                .to_owned(),
        ),
        413 => (Exit::Refused, "The message is too long.".to_owned()),
        _ => (
            Exit::Failed,
            format!("Hangar did not take the message (HTTP {status})."),
        ),
    }
}

fn send(port: u16, token: &str, body: &str) -> io::Result<Vec<u8>> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, TIMEOUT)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    stream.write_all(request(port, token, body).as_bytes())?;
    stream.flush()?;
    let mut raw = Vec::new();
    stream.take(MAX_ANSWER_BYTES).read_to_end(&mut raw)?;
    Ok(raw)
}

fn run(args: &[String]) -> (Exit, String) {
    let (partner, text) = match parse_args(args) {
        Ok(Command::Help) => return (Exit::Waiting, USAGE.to_owned()),
        Ok(Command::Send { partner, text }) => (partner, text),
        Err(problem) => return (Exit::Usage, format!("{problem}\n\n{USAGE}")),
    };
    let Some((port, token)) = std::env::var(SEND_ENV)
        .ok()
        .as_deref()
        .and_then(parse_endpoint)
    else {
        return (
            Exit::NoHangar,
            format!(
                "This is no Hangar session that can send ({SEND_ENV} is not set). Hangar sets it \
                 in agent sessions of projects with partner projects."
            ),
        );
    };
    let text = match text {
        Some(text) => text,
        None if io::stdin().is_terminal() => {
            return (Exit::Usage, format!("no message given\n\n{USAGE}"));
        }
        None => {
            let mut bytes = Vec::new();
            let limit = MAX_BODY_BYTES as u64 + 1;
            if let Err(error) = io::stdin().take(limit).read_to_end(&mut bytes) {
                return (Exit::Failed, format!("cannot read the message: {error}"));
            }
            match decode(&bytes) {
                Ok(text) => text,
                Err(problem) => return (Exit::Refused, problem),
            }
        }
    };
    let body = serde_json::json!({ "partner": partner, "text": text }).to_string();
    if body.len() > MAX_BODY_BYTES {
        return (Exit::Refused, "The message is too long.".to_owned());
    }
    match send(port, &token, &body) {
        Ok(raw) => match parse_response(&raw) {
            Some((status, answer)) => outcome(status, answer),
            None => (Exit::Failed, "Hangar sent no readable answer.".to_owned()),
        },
        Err(error) => (
            Exit::NoHangar,
            format!("Hangar is not reachable ({error}); it may have been closed."),
        ),
    }
}

/// `text` safe for the agent's terminal: every control character but the line break becomes
/// U+FFFD, so an answer can never carry an escape sequence or a carriage return that overwrites
/// what came before.
fn printable(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() && c != '\n' {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let (exit, message) = run(&args);
    let message = printable(&message);
    if exit == Exit::Waiting {
        println!("{message}");
    } else {
        eprintln!("hangar-send: {message}");
    }
    ExitCode::from(exit as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn send_cmd(partner: &str, text: Option<&str>) -> Command {
        Command::Send {
            partner: partner.into(),
            text: text.map(Into::into),
        }
    }

    #[test]
    fn the_partner_comes_first_and_the_words_make_the_message() {
        assert_eq!(
            parse_args(&args(&["web", "show", "the  text"])),
            Ok(send_cmd("web", Some("show the  text")))
        );
        assert_eq!(parse_args(&args(&["web"])), Ok(send_cmd("web", None)));
        // After the partner, words with a dash belong to the message.
        assert_eq!(
            parse_args(&args(&["web", "-v", "flag"])),
            Ok(send_cmd("web", Some("-v flag")))
        );
        assert_eq!(
            parse_args(&args(&["--", "web", "--", "x"])),
            Ok(send_cmd("web", Some("-- x")))
        );
        assert_eq!(parse_args(&args(&["--help"])), Ok(Command::Help));
        assert_eq!(parse_args(&args(&["-h", "web"])), Ok(Command::Help));
        assert!(parse_args(&args(&[])).is_err());
        assert!(parse_args(&args(&["--force", "web"])).is_err());
        assert!(parse_args(&args(&["--"])).is_err());
    }

    #[test]
    fn stdin_may_be_utf8_or_utf16_with_a_mark() {
        assert_eq!(decode("f\u{fc}r".as_bytes()).unwrap(), "f\u{fc}r");
        assert_eq!(decode(b"\xEF\xBB\xBFhi").unwrap(), "hi");
        assert_eq!(decode(b"\xFF\xFEh\0i\0").unwrap(), "hi");
        assert_eq!(decode(b"\xFE\xFF\0h\0i").unwrap(), "hi");
        assert!(decode(b"\xFF\xFEh").is_err());
        assert!(decode(b"\xC3").is_err());
    }

    #[test]
    fn the_endpoint_variable_is_port_and_token() {
        assert_eq!(
            parse_endpoint("4711:abc123"),
            Some((4711, "abc123".to_owned()))
        );
        for bad in [
            "",
            "4711",
            "0:abc",
            "x:abc",
            "4711:",
            "4711:a b",
            "4711:a\r\nX: y",
        ] {
            assert_eq!(parse_endpoint(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_request_is_a_plain_post_to_the_send_route() {
        let text = request(4711, "abc", "{}");
        assert!(text.starts_with("POST /v1/send HTTP/1.1\r\n"), "{text}");
        assert!(text.contains("Host: 127.0.0.1:4711\r\n"));
        assert!(text.contains("Authorization: Bearer abc\r\n"));
        assert!(text.ends_with("Content-Length: 2\r\nConnection: close\r\n\r\n{}"));
    }

    #[test]
    fn answers_become_exit_codes() {
        let raw = b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 9\r\n\r\n\
                    {\"code\":\"full\",\"message\":\"5 wait\"}";
        let (status, answer) = parse_response(raw).unwrap();
        assert_eq!(status, 429);
        assert_eq!(outcome(status, answer), (Exit::Full, "5 wait".to_owned()));
        for (code, exit) in [
            ("waiting", Exit::Waiting),
            ("not_partner", Exit::NotPartner),
            ("refused", Exit::Refused),
            ("no_session", Exit::NoHangar),
            ("other", Exit::Failed),
        ] {
            assert_eq!(outcome(200, Some((code.into(), "m".into()))).0, exit);
        }
        assert_eq!(outcome(401, None).0, Exit::NoHangar);
        assert_eq!(outcome(403, None).0, Exit::NoHangar);
        assert_eq!(outcome(413, None).0, Exit::Refused);
        assert_eq!(outcome(503, None).0, Exit::Failed);
        assert_eq!(parse_response(b"garbage"), None);
        assert_eq!(
            parse_response(b"HTTP/1.1 401 Unauthorized\r\n\r\n"),
            Some((401, None))
        );
    }

    #[test]
    fn answers_reach_the_terminal_without_control_characters() {
        assert_eq!(
            printable("ok\nnext\u{1b}[2J\r\u{7}\t\u{9b}x"),
            "ok\nnext\u{FFFD}[2J\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}x"
        );
        assert_eq!(printable("f\u{fc}r"), "f\u{fc}r");
        assert_eq!(
            outcome(403, Some(("not_agent".into(), "m".into()))).0,
            Exit::NoHangar
        );
    }

    #[test]
    fn the_exit_codes_match_the_usage_text() {
        for (exit, number) in [
            (Exit::Waiting, 0),
            (Exit::Failed, 1),
            (Exit::Usage, 2),
            (Exit::NoHangar, 3),
            (Exit::NotPartner, 4),
            (Exit::Full, 5),
            (Exit::Refused, 6),
        ] {
            assert_eq!(exit as u8, number);
        }
        assert!(USAGE.contains("0 waiting, 1 failed, 2 usage, 3 no Hangar session"));
    }
}

//! The smallest HTTP/1.1 the agent gate's hook endpoint needs (`gate_service.rs`): one `POST`
//! with `Content-Length` per connection, answered with `Connection: close`.
//!
//! Hand-written rather than a server crate: two routes on 127.0.0.1, one client (Claude Code's
//! http hooks send a plain `Content-Length` body, measured), and every limit explicit here. hyper
//! or axum would add an async runtime path, routing and keep-alive handling for no gain; tiny_http
//! is not in the tree. Anything unusual is refused rather than interpreted: chunked bodies, a
//! second `Content-Length`, folded headers, control characters, oversized heads.

use std::io::{self, BufRead};

/// Request line plus headers, in bytes.
pub const MAX_HEAD_BYTES: usize = 16 * 1024;
pub const MAX_HEADERS: usize = 64;
/// Body limit. A `Write` hook carries the whole new file content; it is parsed as it streams in
/// and never kept, so the limit only bounds the time spent reading. Larger bodies are refused,
/// which Claude Code treats as a failed hook and lets the edit through (the step A warning then
/// still applies).
pub const MAX_BODY_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    MethodNotAllowed,
    LengthRequired,
    PayloadTooLarge,
    HeadersTooLarge,
    NotImplemented,
    Unavailable,
}

impl Status {
    pub fn code(self) -> u16 {
        match self {
            Status::Ok => 200,
            Status::BadRequest => 400,
            Status::Unauthorized => 401,
            Status::Forbidden => 403,
            Status::NotFound => 404,
            Status::MethodNotAllowed => 405,
            Status::LengthRequired => 411,
            Status::PayloadTooLarge => 413,
            Status::HeadersTooLarge => 431,
            Status::NotImplemented => 501,
            Status::Unavailable => 503,
        }
    }

    fn reason(self) -> &'static str {
        match self {
            Status::Ok => "OK",
            Status::BadRequest => "Bad Request",
            Status::Unauthorized => "Unauthorized",
            Status::Forbidden => "Forbidden",
            Status::NotFound => "Not Found",
            Status::MethodNotAllowed => "Method Not Allowed",
            Status::LengthRequired => "Length Required",
            Status::PayloadTooLarge => "Payload Too Large",
            Status::HeadersTooLarge => "Request Header Fields Too Large",
            Status::NotImplemented => "Not Implemented",
            Status::Unavailable => "Service Unavailable",
        }
    }
}

/// What went wrong while reading a request; [`HeadError::status`] is the answer.
#[derive(Debug, thiserror::Error)]
pub enum HeadError {
    #[error("Malformed request")]
    Malformed,
    #[error("Request head too large")]
    TooLarge,
    #[error("Only POST is accepted")]
    Method,
    #[error("Content-Length missing")]
    NoLength,
    #[error("Body too large")]
    BodyTooLarge,
    #[error("Transfer encodings are not supported")]
    Encoding,
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl HeadError {
    /// `None`: the connection is gone or timed out, nothing to answer.
    pub fn status(&self) -> Option<Status> {
        match self {
            HeadError::Malformed => Some(Status::BadRequest),
            HeadError::TooLarge => Some(Status::HeadersTooLarge),
            HeadError::Method => Some(Status::MethodNotAllowed),
            HeadError::NoLength => Some(Status::LengthRequired),
            HeadError::BodyTooLarge => Some(Status::PayloadTooLarge),
            HeadError::Encoding => Some(Status::NotImplemented),
            HeadError::Io(_) => None,
        }
    }
}

/// The parts of a request head the endpoint uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    /// The path, without a query.
    pub path: String,
    /// The bearer token of `Authorization`, if any.
    pub token: Option<String>,
    pub content_length: u64,
    /// `Host` as sent; the endpoint wants exactly its own `127.0.0.1:<port>`.
    pub host: Option<String>,
    /// `Origin` or a `Sec-Fetch-*` header: a browser sent it (a page trying the local port), not
    /// Claude Code.
    pub from_browser: bool,
}

/// Reads one line ending in LF (CR before it is dropped) into `line`, counting against `budget`.
fn read_line(
    reader: &mut impl BufRead,
    line: &mut Vec<u8>,
    budget: &mut usize,
) -> Result<(), HeadError> {
    line.clear();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Err(HeadError::Io(io::ErrorKind::UnexpectedEof.into()));
        }
        let (chunk, done) = match available.iter().position(|b| *b == b'\n') {
            Some(at) => (&available[..=at], true),
            None => (available, false),
        };
        if chunk.len() > *budget {
            return Err(HeadError::TooLarge);
        }
        *budget -= chunk.len();
        line.extend_from_slice(chunk);
        let used = chunk.len();
        reader.consume(used);
        if done {
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(());
        }
    }
}

fn visible_ascii(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .all(|b| (0x20..0x7f).contains(b) || *b == b'\t')
}

/// Reads and checks a request head. The body is left in `reader`.
pub fn read_head(reader: &mut impl BufRead) -> Result<Head, HeadError> {
    let mut budget = MAX_HEAD_BYTES;
    let mut line = Vec::new();
    read_line(reader, &mut line, &mut budget)?;
    if !visible_ascii(&line) {
        return Err(HeadError::Malformed);
    }
    let text = String::from_utf8_lossy(&line).into_owned();
    let mut parts = text.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(HeadError::Malformed);
    };
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err(HeadError::Malformed);
    }
    if !target.starts_with('/') {
        return Err(HeadError::Malformed);
    }
    let method_ok = method == "POST";

    let mut token = None;
    let mut host: Option<String> = None;
    let mut from_browser = false;
    let mut length: Option<u64> = None;
    let mut headers = 0;
    loop {
        read_line(reader, &mut line, &mut budget)?;
        if line.is_empty() {
            break;
        }
        headers += 1;
        if headers > MAX_HEADERS {
            return Err(HeadError::TooLarge);
        }
        // Folded continuation lines are obsolete (RFC 9112) and refused.
        if line[0] == b' ' || line[0] == b'\t' || !visible_ascii(&line) {
            return Err(HeadError::Malformed);
        }
        let header = String::from_utf8_lossy(&line);
        let Some((name, value)) = header.split_once(':') else {
            return Err(HeadError::Malformed);
        };
        if name.is_empty() || name.ends_with([' ', '\t']) {
            return Err(HeadError::Malformed);
        }
        let value = value.trim_matches([' ', '\t']);
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() || value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(HeadError::Malformed);
            }
            let parsed: u64 = value.parse().map_err(|_| HeadError::BodyTooLarge)?;
            length = Some(parsed);
        } else if name.eq_ignore_ascii_case("host") {
            if host.is_some() {
                return Err(HeadError::Malformed);
            }
            host = Some(value.to_owned());
        } else if name.eq_ignore_ascii_case("origin")
            || name
                .get(..10)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("sec-fetch-"))
        {
            from_browser = true;
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(HeadError::Encoding);
        } else if name.eq_ignore_ascii_case("authorization") {
            if token.is_some() {
                return Err(HeadError::Malformed);
            }
            let bearer = value
                .split_once(' ')
                .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
                .map(|(_, rest)| rest.trim().to_owned());
            token = Some(bearer.unwrap_or_default());
        }
    }
    if !method_ok {
        return Err(HeadError::Method);
    }
    let content_length = length.ok_or(HeadError::NoLength)?;
    if content_length > MAX_BODY_BYTES {
        return Err(HeadError::BodyTooLarge);
    }
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    Ok(Head {
        path: path.to_owned(),
        token: token.filter(|t| !t.is_empty()),
        content_length,
        host,
        from_browser,
    })
}

/// A complete response with a JSON body (or none) that closes the connection.
pub fn response(status: Status, body: &str) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        status.code(),
        status.reason(),
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(raw: &[u8]) -> Result<Head, HeadError> {
        read_head(&mut io::BufReader::new(raw))
    }

    #[test]
    fn a_claude_code_request_head() {
        let raw =
            b"POST /v1/pre-tool-use HTTP/1.1\r\nAccept: application/json, text/plain, */*\r\n\
            Content-Type: application/json\r\nUser-Agent: axios/1.15.2\r\nContent-Length: 687\r\n\
            Accept-Encoding: gzip, compress, deflate, br\r\nHost: 127.0.0.1:47811\r\n\
            Connection: keep-alive\r\nAuthorization: Bearer abc123\r\n\r\n{\"x\":1}";
        let mut reader = io::BufReader::new(&raw[..]);
        assert_eq!(
            read_head(&mut reader).unwrap(),
            Head {
                path: "/v1/pre-tool-use".into(),
                token: Some("abc123".into()),
                content_length: 687,
                host: Some("127.0.0.1:47811".into()),
                from_browser: false,
            }
        );
        // The body stays for the caller.
        let mut rest = String::new();
        io::Read::read_to_string(&mut reader, &mut rest).unwrap();
        assert_eq!(rest, "{\"x\":1}");
    }

    #[test]
    fn bare_lf_lines_query_and_missing_token() {
        let parsed = head(b"POST /a?b=c HTTP/1.0\nContent-Length: 0\n\n").unwrap();
        assert_eq!(parsed.path, "/a");
        assert_eq!(parsed.token, None);
        assert_eq!(parsed.content_length, 0);
        let basic =
            head(b"POST / HTTP/1.1\r\nAuthorization: Basic eDp5\r\nContent-Length: 0\r\n\r\n");
        assert_eq!(basic.unwrap().token, None);
        let empty = head(b"POST / HTTP/1.1\r\nAuthorization: Bearer \r\nContent-Length: 0\r\n\r\n");
        assert_eq!(empty.unwrap().token, None);
    }

    #[test]
    fn host_and_browser_headers_are_reported() {
        let plain = head(b"POST / HTTP/1.1\r\nContent-Length: 0\r\n\r\n").unwrap();
        assert_eq!((plain.host, plain.from_browser), (None, false));
        for header in [
            "Origin: http://example.invalid",
            "origin: null",
            "Sec-Fetch-Site: cross-site",
            "sec-fetch-mode: no-cors",
        ] {
            let raw = format!("POST / HTTP/1.1\r\n{header}\r\nContent-Length: 0\r\n\r\n");
            assert!(head(raw.as_bytes()).unwrap().from_browser, "{header}");
        }
        let host = head(b"POST / HTTP/1.1\r\nHost:  127.0.0.1:4711 \r\nContent-Length: 0\r\n\r\n");
        assert_eq!(host.unwrap().host.as_deref(), Some("127.0.0.1:4711"));
        let twice = head(b"POST / HTTP/1.1\r\nHost: a\r\nHost: b\r\nContent-Length: 0\r\n\r\n");
        assert!(matches!(twice, Err(HeadError::Malformed)));
    }

    #[test]
    fn hostile_heads_are_refused_with_the_right_status() {
        let status = |raw: &[u8]| head(raw).err().and_then(|e| e.status());
        let cases: &[(&[u8], Option<Status>)] = &[
            (
                b"GET / HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
                Some(Status::MethodNotAllowed),
            ),
            (b"POST / HTTP/1.1\r\n\r\n", Some(Status::LengthRequired)),
            (
                b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\n",
                Some(Status::BadRequest),
            ),
            (
                b"POST / HTTP/1.1\r\nContent-Length: -1\r\n\r\n",
                Some(Status::BadRequest),
            ),
            (
                b"POST / HTTP/1.1\r\nContent-Length: 1 2\r\n\r\n",
                Some(Status::BadRequest),
            ),
            (
                b"POST / HTTP/1.1\r\nContent-Length: 99999999999999999999999\r\n\r\n",
                Some(Status::PayloadTooLarge),
            ),
            (
                b"POST / HTTP/1.1\r\nContent-Length: 33554433\r\n\r\n",
                Some(Status::PayloadTooLarge),
            ),
            (
                b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
                Some(Status::NotImplemented),
            ),
            (
                b"POST / HTTP/1.1\r\n Folded: x\r\nContent-Length: 0\r\n\r\n",
                Some(Status::BadRequest),
            ),
            (
                b"POST / HTTP/1.1\r\nNoColon\r\n\r\n",
                Some(Status::BadRequest),
            ),
            (
                b"POST / HTTP/1.1\r\nName : x\r\n\r\n",
                Some(Status::BadRequest),
            ),
            (
                b"POST / HTTP/1.1\r\nX: a\x00b\r\n\r\n",
                Some(Status::BadRequest),
            ),
            (
                b"POST / HTTP/1.1\r\nX: \xc3\xa4\r\n\r\n",
                Some(Status::BadRequest),
            ),
            (b"POST /\x1b HTTP/1.1\r\n\r\n", Some(Status::BadRequest)),
            (b"POST  / HTTP/1.1\r\n\r\n", Some(Status::BadRequest)),
            (b"POST x HTTP/1.1\r\n\r\n", Some(Status::BadRequest)),
            (b"POST / HTTP/2\r\n\r\n", Some(Status::BadRequest)),
            (
                b"POST / HTTP/1.1\r\nAuthorization: Bearer a\r\nAuthorization: Bearer b\r\n\r\n",
                Some(Status::BadRequest),
            ),
            // Cut off: nothing to answer.
            (b"POST / HTTP/1.1\r\nContent-Length: 0\r\n", None),
            (b"", None),
        ];
        for (raw, expected) in cases {
            assert_eq!(status(raw), *expected, "{}", String::from_utf8_lossy(raw));
        }
    }

    #[test]
    fn head_size_and_header_count_are_limited() {
        let long = format!(
            "POST / HTTP/1.1\r\nX: {}\r\nContent-Length: 0\r\n\r\n",
            "a".repeat(MAX_HEAD_BYTES)
        );
        assert!(matches!(head(long.as_bytes()), Err(HeadError::TooLarge)));
        let no_newline = "P".repeat(MAX_HEAD_BYTES + 10);
        assert!(matches!(
            head(no_newline.as_bytes()),
            Err(HeadError::TooLarge)
        ));
        let many: String = (0..=MAX_HEADERS).map(|i| format!("X{i}: y\r\n")).collect();
        let many = format!("POST / HTTP/1.1\r\n{many}Content-Length: 0\r\n\r\n");
        assert!(matches!(head(many.as_bytes()), Err(HeadError::TooLarge)));
        let fits: String = (0..MAX_HEADERS - 1)
            .map(|i| format!("X{i}: y\r\n"))
            .collect();
        let fits = format!("POST / HTTP/1.1\r\n{fits}Content-Length: 0\r\n\r\n");
        assert!(head(fits.as_bytes()).is_ok());
    }

    #[test]
    fn responses_close_the_connection() {
        let text = String::from_utf8(response(Status::Ok, "{\"a\":1}")).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
        assert!(text.contains("Content-Length: 7\r\n"));
        assert!(text.contains("Connection: close\r\n"));
        assert!(text.ends_with("\r\n\r\n{\"a\":1}"));
        let empty = String::from_utf8(response(Status::Unauthorized, "")).unwrap();
        assert!(empty.starts_with("HTTP/1.1 401 Unauthorized\r\n"));
        assert!(empty.ends_with(
            "Content-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
        ));
    }
}

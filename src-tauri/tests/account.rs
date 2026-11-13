//! The account service against a small mock of the Hangar account API on 127.0.0.1 (plain
//! HTTP/1.1, one thread per connection, `Connection: close`). Nothing here talks to a real
//! server, opens a browser or touches the OS keychain: the opener records URLs and the keychain
//! is an in-memory store.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hangar_lib::account::{
    load_config, AccountError, AccountPage, AccountState, AccountView, BaseUrl, FailKind,
    IdleNotice, KEYCHAIN_SERVICE,
};
use hangar_lib::account_api::{AccountApi, ApiError};
use hangar_lib::account_service::{AccountService, Options};
use hangar_lib::secrets::{SecretError, SecretStore};
use zeroize::Zeroizing;

/// One second of the device flow lasts this long in these tests.
const UNIT: Duration = Duration::from_millis(10);

fn guard<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ------------------------------------------------------------------------------------------
// Mock server
// ------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Poll {
    Pending,
    SlowDown,
    Denied,
    Tokens,
}

#[derive(Default)]
struct MockState {
    interval: u64,
    expires_in: u64,
    /// Answers to device code polls, in order; afterwards `authorization_pending`.
    script: VecDeque<Poll>,
    poll_times: Vec<Instant>,
    device_requests: u32,
    /// Origin the verification pages name; the server's own unless set.
    page_origin: Option<String>,
    refreshes: u32,
    refresh_delay: Duration,
    /// The one refresh token the server accepts right now.
    refresh_valid: Option<String>,
    refresh_used: HashSet<String>,
    /// A reused refresh token revoked the device.
    revoked: bool,
    access_valid: HashSet<String>,
    issued: u32,
    projects_failure: Option<(u16, String)>,
    /// `/projects` refuses every access token, even a fresh one.
    projects_always_unauthorized: bool,
    /// The last rotation, for the server's 30 s grace.
    rotation: Option<Rotation>,
    /// A just-rotated refresh token was presented again and got a fresh pair (grace).
    grace_used: u32,
    /// The next refreshes rotate on the server but their answer never arrives.
    lose_refresh_answers: u32,
    /// Bodies of `POST /oauth/revoke`.
    revocations: Vec<serde_json::Value>,
    /// The OAuth error `/oauth/revoke` answers with instead of 200.
    revoke_failure: Option<&'static str>,
    /// `GET /me` requests, and how many of the next ones answer 503.
    me_requests: u32,
    me_failures: u32,
    /// The profile picture `/me` names: content type, bytes and etag.
    avatar: Option<(&'static str, Vec<u8>, String)>,
    /// Origin of `avatar_url`; the server's own unless set.
    avatar_origin: Option<String>,
    /// `If-None-Match` of every picture request.
    avatar_requests: Vec<Option<String>>,
    /// Path of `avatar_url`; `/api/v1/me/avatar` unless set.
    avatar_path: Option<String>,
    /// How the picture endpoint answers.
    avatar_mode: AvatarMode,
    /// Requests that reached an address the client must never call (a redirect target, a path
    /// outside the API).
    forbidden_hits: u32,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum AvatarMode {
    #[default]
    Normal,
    /// 401 even for a valid access token.
    Unauthorized,
    /// 302 to another address on the same server.
    Redirect,
    /// A chunked body without `Content-Length`, larger than the limit.
    Endless,
}

struct Rotation {
    from: String,
    at: Instant,
    successor: String,
    grace_spent: bool,
}

/// Grace of the official service: a just-rotated refresh token presented again within this time,
/// while its successor is unused, gets a fresh pair once.
const GRACE: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct Mock {
    origin: String,
    state: Arc<Mutex<MockState>>,
}

impl Mock {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(MockState {
            interval: 5,
            expires_in: 600,
            ..MockState::default()
        }));
        let shared = Arc::clone(&state);
        let own = origin.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let state = Arc::clone(&shared);
                let origin = own.clone();
                std::thread::spawn(move || serve(stream, &state, &origin));
            }
        });
        Self { origin, state }
    }

    fn base(&self) -> BaseUrl {
        BaseUrl::parse(&format!("{}/api/v1", self.origin)).unwrap()
    }

    fn state(&self) -> MutexGuard<'_, MockState> {
        guard(&self.state)
    }
}

struct Request {
    method: String,
    path: String,
    bearer: Option<String>,
    if_none_match: Option<String>,
    body: serde_json::Value,
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut data = Vec::new();
    let mut buffer = [0u8; 4096];
    let head_end = loop {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        data.extend_from_slice(&buffer[..read]);
        if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
    };
    let head = String::from_utf8_lossy(&data[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let path = first.next()?.to_owned();
    let mut headers = HashMap::new();
    for line in lines.filter(|l| !l.is_empty()) {
        let (name, value) = line.split_once(':')?;
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
    }
    let length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    while data.len() < head_end + length {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        data.extend_from_slice(&buffer[..read]);
    }
    let raw = &data[head_end..head_end + length];
    let form = headers
        .get("content-type")
        .is_some_and(|v| v.starts_with("application/x-www-form-urlencoded"));
    let body = if form {
        form_body(raw)
    } else {
        serde_json::from_slice(raw).unwrap_or(serde_json::Value::Null)
    };
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned);
    let if_none_match = headers.get("if-none-match").cloned();
    Some(Request {
        method,
        path,
        bearer,
        if_none_match,
        body,
    })
}

/// `a=b&c=d` (percent-encoded, `+` for space) as a JSON object of strings.
fn form_body(raw: &[u8]) -> serde_json::Value {
    let decode = |text: &str| {
        let bytes = text.replace('+', " ").into_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(out).unwrap()
    };
    let text = String::from_utf8_lossy(raw);
    let object = text
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(key), serde_json::Value::String(decode(value)))
        })
        .collect();
    serde_json::Value::Object(object)
}

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Error",
    };
    let text = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(text.as_bytes());
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Write);
}

fn json(stream: &mut TcpStream, status: u16, value: &serde_json::Value) {
    respond(stream, status, "application/json", &value.to_string());
}

fn problem(stream: &mut TcpStream, status: u16, code: &str, detail: &str) {
    let value = serde_json::json!({
        "type": format!("urn:hangar:api:problem:{code}"),
        "title": "Problem",
        "status": status,
        "detail": detail,
        "code": code,
    });
    respond(
        stream,
        status,
        "application/problem+json",
        &value.to_string(),
    );
}

fn oauth_error(stream: &mut TcpStream, error: &str) {
    let value = serde_json::json!({
        "type": format!("urn:hangar:api:problem:{error}"),
        "title": "OAuth error",
        "status": 400,
        "code": error,
        "error": error,
        "error_description": format!("mock {error}"),
    });
    respond(stream, 400, "application/problem+json", &value.to_string());
}

/// A new token pair; the old refresh token is spent.
fn issue(state: &mut MockState) -> serde_json::Value {
    state.issued += 1;
    let access = format!("hgat_mock_access_{}", state.issued);
    let refresh = format!("hgrt_mock_refresh_{}", state.issued);
    state.access_valid.insert(access.clone());
    state.refresh_valid = Some(refresh.clone());
    serde_json::json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": 900,
        "refresh_token": refresh,
        "scope": "team",
    })
}

/// The refresh grant: rotation, the 30 s grace for a just-rotated token whose successor is unused,
/// and revocation of the whole device on any other reuse. `None` answers `invalid_grant`.
fn redeem(state: &mut MockState, token: String) -> Option<serde_json::Value> {
    if state.revoked {
        return None;
    }
    if state.refresh_valid.as_deref() == Some(token.as_str()) {
        state.refresh_used.insert(token.clone());
        let value = issue(state);
        state.rotation = Some(Rotation {
            from: token,
            at: Instant::now(),
            successor: value["refresh_token"].as_str().unwrap().to_owned(),
            grace_spent: false,
        });
        return Some(value);
    }
    let graced = state.rotation.as_ref().is_some_and(|rotation| {
        rotation.from == token
            && rotation.at.elapsed() < GRACE
            && !rotation.grace_spent
            && state.refresh_valid.as_deref() == Some(rotation.successor.as_str())
    });
    if graced {
        state.grace_used += 1;
        let value = issue(state);
        if let Some(rotation) = state.rotation.as_mut() {
            rotation.successor = value["refresh_token"].as_str().unwrap().to_owned();
            rotation.grace_spent = true;
        }
        return Some(value);
    }
    if state.refresh_used.contains(&token) {
        // Reuse: the whole device is revoked.
        state.revoked = true;
        state.refresh_valid = None;
        state.access_valid.clear();
    }
    None
}

fn authorized(state: &MockState, request: &Request) -> bool {
    request
        .bearer
        .as_ref()
        .is_some_and(|token| state.access_valid.contains(token))
}

fn serve(mut stream: TcpStream, shared: &Mutex<MockState>, origin: &str) {
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let route = (request.method.as_str(), request.path.as_str());
    match route {
        ("POST", "/api/v1/oauth/device") => {
            let mut state = guard(shared);
            state.device_requests += 1;
            assert_eq!(request.body["client_id"], "hangar-desktop");
            assert_eq!(request.body["scope"], "team");
            let pages = state
                .page_origin
                .clone()
                .unwrap_or_else(|| origin.to_owned());
            let value = serde_json::json!({
                "device_code": "mock-device-code-secret",
                "user_code": "BCDF-GHJK",
                "verification_uri": format!("{pages}/device"),
                "verification_uri_complete": format!("{pages}/device?user_code=BCDF-GHJK"),
                "expires_in": state.expires_in,
                "interval": state.interval,
            });
            drop(state);
            json(&mut stream, 200, &value);
        }
        ("POST", "/api/v1/oauth/token") => {
            let grant = request.body["grant_type"].as_str().unwrap_or("").to_owned();
            if grant == "urn:ietf:params:oauth:grant-type:device_code" {
                let mut state = guard(shared);
                state.poll_times.push(Instant::now());
                assert_eq!(request.body["device_code"], "mock-device-code-secret");
                let next = state.script.pop_front().unwrap_or(Poll::Pending);
                match next {
                    Poll::Pending => {
                        drop(state);
                        oauth_error(&mut stream, "authorization_pending");
                    }
                    Poll::SlowDown => {
                        drop(state);
                        oauth_error(&mut stream, "slow_down");
                    }
                    Poll::Denied => {
                        drop(state);
                        oauth_error(&mut stream, "access_denied");
                    }
                    Poll::Tokens => {
                        let value = issue(&mut state);
                        drop(state);
                        json(&mut stream, 200, &value);
                    }
                }
            } else if grant == "refresh_token" {
                let token = request.body["refresh_token"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned();
                let delay = {
                    let mut state = guard(shared);
                    state.refreshes += 1;
                    state.refresh_delay
                };
                std::thread::sleep(delay);
                let mut state = guard(shared);
                match redeem(&mut state, token) {
                    Some(value) => {
                        let lose = state.lose_refresh_answers > 0;
                        state.lose_refresh_answers = state.lose_refresh_answers.saturating_sub(1);
                        drop(state);
                        if lose {
                            // Rotated on the server, but the answer is lost on the way.
                            let _ = stream.shutdown(std::net::Shutdown::Both);
                        } else {
                            json(&mut stream, 200, &value);
                        }
                    }
                    None => {
                        drop(state);
                        oauth_error(&mut stream, "invalid_grant");
                    }
                }
            } else {
                oauth_error(&mut stream, "unsupported_grant_type");
            }
        }
        ("POST", "/api/v1/oauth/revoke") => {
            let mut state = guard(shared);
            state.revocations.push(request.body.clone());
            let hint = request.body["token_type_hint"].as_str();
            let known_hint = matches!(hint, None | Some("refresh_token" | "access_token"));
            if let Some(code) = state.revoke_failure {
                drop(state);
                oauth_error(&mut stream, code);
            } else if request.body["client_id"] != "hangar-desktop" {
                drop(state);
                oauth_error(&mut stream, "invalid_client");
            } else if !known_hint || request.body["token"].as_str().is_none() {
                // An unknown hint counts as invalid_request, like a missing token.
                drop(state);
                oauth_error(&mut stream, "invalid_request");
            } else {
                let token = request.body["token"].as_str().unwrap_or("");
                let known = state.refresh_valid.as_deref() == Some(token)
                    || state.refresh_used.contains(token);
                if known {
                    // Revoking a refresh token removes the whole device grant.
                    state.refresh_valid = None;
                    state.access_valid.clear();
                    state.rotation = None;
                }
                drop(state);
                respond(&mut stream, 200, "application/json", "");
            }
        }
        ("GET", "/api/v1/me") => {
            let mut state = guard(shared);
            state.me_requests += 1;
            if state.me_failures > 0 && authorized(&state, &request) {
                state.me_failures -= 1;
                drop(state);
                problem(&mut stream, 503, "busy", "Try again");
            } else if authorized(&state, &request) {
                drop(state);
                // `seats_account_used` counts members only, the owner included.
                let mut value = serde_json::json!({
                    "id": "7", "email": "ada@example.test", "name": "Ada", "locale": "de",
                    "seats_account_total": 3, "seats_account_used": 2,
                    "avatar_url": null, "avatar_etag": null,
                });
                let state = guard(shared);
                if let Some((_, _, etag)) = &state.avatar {
                    let pages = state.avatar_origin.as_deref().unwrap_or(origin);
                    let path = state.avatar_path.as_deref().unwrap_or("/api/v1/me/avatar");
                    value["avatar_url"] = format!("{pages}{path}?v={}", &etag[..16]).into();
                    value["avatar_etag"] = etag.clone().into();
                }
                drop(state);
                json(&mut stream, 200, &value);
            } else {
                drop(state);
                problem(&mut stream, 401, "unauthorized", "Sign in again");
            }
        }
        ("GET", "/api/v1/projects") => {
            let state = guard(shared);
            if !authorized(&state, &request) || state.projects_always_unauthorized {
                drop(state);
                problem(&mut stream, 401, "unauthorized", "Sign in again");
            } else if let Some((status, detail)) = state.projects_failure.clone() {
                drop(state);
                problem(&mut stream, status, "busy", &detail);
            } else {
                drop(state);
                let value = serde_json::json!({"projects": [
                    {"id": "317", "name": "Core", "role": "owner", "owner_id": "7",
                     "plan": "free", "seats_account_total": 3, "seats_account_used": 2,
                     "pending_invitations": 1,
                     "invite_url": format!("{origin}/projects/317#invite")},
                    {"id": "318", "name": "Docs", "role": "member", "owner_id": "9",
                     "plan": "team", "seats_account_total": 10, "seats_account_used": 4,
                     "invite_url": format!("{origin}/projects/318#invite")},
                ]});
                json(&mut stream, 200, &value);
            }
        }
        ("POST", "/api/v1/relay-ticket") => {
            let state = guard(shared);
            if !authorized(&state, &request) {
                drop(state);
                problem(&mut stream, 401, "unauthorized", "Sign in again");
            } else if request.body["project_id"] == "317" {
                drop(state);
                let value = serde_json::json!({
                    "ticket": "eyJ2IjoxfQ.c2ln", "expires_at": 1_794_225_660u64,
                    "relay_url": "wss://relay.example.test",
                });
                json(&mut stream, 200, &value);
            } else {
                drop(state);
                problem(&mut stream, 404, "not_found", "No such project");
            }
        }
        ("GET", path) if path.starts_with("/api/v1/me/avatar") => {
            let mut state = guard(shared);
            state.avatar_requests.push(request.if_none_match.clone());
            let mode = state.avatar_mode;
            if !authorized(&state, &request) || mode == AvatarMode::Unauthorized {
                drop(state);
                problem(&mut stream, 401, "unauthorized", "Sign in again");
                return;
            }
            if mode == AvatarMode::Redirect {
                drop(state);
                let text = format!(
                    "HTTP/1.1 302 Found\r\nLocation: {origin}/elsewhere/avatar\r\n\
                     Content-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(text.as_bytes());
                return;
            }
            if mode == AvatarMode::Endless {
                drop(state);
                let head = "HTTP/1.1 200 OK\r\nContent-Type: image/webp\r\n\
                            Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
                if stream.write_all(head.as_bytes()).is_err() {
                    return;
                }
                let mut chunk = WEBP.to_vec();
                chunk.resize(64 * 1024, 0);
                // 4 MiB at most; the client stops reading after 1 MiB.
                for _ in 0..64 {
                    let frame = format!("{:x}\r\n", chunk.len());
                    let sent = stream
                        .write_all(frame.as_bytes())
                        .and_then(|()| stream.write_all(&chunk))
                        .and_then(|()| stream.write_all(b"\r\n"));
                    if sent.is_err() {
                        return;
                    }
                }
                let _ = stream.write_all(b"0\r\n\r\n");
                return;
            }
            let Some((content_type, bytes, etag)) = state.avatar.clone() else {
                drop(state);
                problem(&mut stream, 404, "not_found", "No picture");
                return;
            };
            drop(state);
            let quoted = format!("\"{etag}\"");
            if request.if_none_match.as_deref() == Some(quoted.as_str()) {
                let text = format!(
                    "HTTP/1.1 304 Not Modified\r\nETag: {quoted}\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(text.as_bytes());
                return;
            }
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
                 ETag: {quoted}\r\nCache-Control: private, max-age=86400\r\n\
                 Connection: close\r\n\r\n",
                bytes.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&bytes);
            let _ = stream.flush();
            let _ = stream.shutdown(std::net::Shutdown::Write);
        }
        ("GET", path) if path.starts_with("/elsewhere") || path.starts_with("/other-app") => {
            guard(shared).forbidden_hits += 1;
            respond(&mut stream, 200, "image/webp", "RIFF");
        }
        ("GET" | "POST", "/api/v1/devices") => {
            problem(&mut stream, 501, "not_implemented", "Later")
        }
        _ => problem(&mut stream, 404, "not_found", "Unknown route"),
    }
}

// ------------------------------------------------------------------------------------------
// Service under test
// ------------------------------------------------------------------------------------------

#[derive(Default)]
struct MemoryStore {
    values: Mutex<HashMap<String, Zeroizing<String>>>,
    /// When set, every write checks that a Hangar process holds this lock file.
    lock_file: Option<std::path::PathBuf>,
    /// Writes (`set`, `delete`) made while nobody held [`Self::lock_file`].
    unlocked_writes: Mutex<Vec<&'static str>>,
}

impl MemoryStore {
    /// A store that records every write made without the account lock file in `dir`.
    fn watching(dir: &std::path::Path) -> Self {
        Self {
            lock_file: Some(dir.join(hangar_lib::account::LOCK_FILE)),
            ..Self::default()
        }
    }

    fn value(&self, account: &str) -> Option<String> {
        guard(&self.values).get(account).map(|v| v.to_string())
    }

    fn unlocked_writes(&self) -> Vec<&'static str> {
        guard(&self.unlocked_writes).clone()
    }

    /// A handle of its own stands for another process: it gets the lock only when nobody holds it.
    fn check_locked(&self, write: &'static str) {
        let Some(path) = &self.lock_file else { return };
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .unwrap();
        if file.try_lock().is_ok() {
            guard(&self.unlocked_writes).push(write);
            file.unlock().unwrap();
        }
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, SecretError> {
        Ok(guard(&self.values).get(account).cloned())
    }

    fn exists(&self, account: &str) -> Result<bool, SecretError> {
        Ok(guard(&self.values).contains_key(account))
    }

    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        self.check_locked("set");
        guard(&self.values).insert(account.to_owned(), Zeroizing::new(value.to_owned()));
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.check_locked("delete");
        guard(&self.values).remove(account);
        Ok(())
    }
}

struct Harness {
    mock: Mock,
    store: Arc<MemoryStore>,
    opened: Arc<Mutex<Vec<String>>>,
    events: Arc<Mutex<Vec<String>>>,
    service: AccountService,
}

impl Harness {
    fn new() -> Self {
        Self::with(Mock::start(), Arc::new(MemoryStore::default()))
    }

    fn with(mock: Mock, store: Arc<MemoryStore>) -> Self {
        Self::configured(mock, store, None)
    }

    fn configured(
        mock: Mock,
        store: Arc<MemoryStore>,
        config_dir: Option<std::path::PathBuf>,
    ) -> Self {
        Self::configured_as(mock, store, config_dir, KEYCHAIN_SERVICE)
    }

    /// A service whose store stands for the keychain service `keychain_service`.
    fn configured_as(
        mock: Mock,
        store: Arc<MemoryStore>,
        config_dir: Option<std::path::PathBuf>,
        keychain_service: &str,
    ) -> Self {
        Self::built(mock, store, config_dir, keychain_service, None)
    }

    fn built(
        mock: Mock,
        store: Arc<MemoryStore>,
        config_dir: Option<std::path::PathBuf>,
        keychain_service: &str,
        avatar_dir: Option<std::path::PathBuf>,
    ) -> Self {
        let opened = Arc::new(Mutex::new(Vec::new()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&opened);
        let log = Arc::clone(&events);
        let service = AccountService::new(
            mock.base(),
            Arc::clone(&store) as Arc<dyn SecretStore>,
            Arc::new(move |url: &reqwest::Url| {
                guard(&record).push(url.to_string());
                Ok(())
            }),
            Arc::new(move |view: &AccountView| {
                guard(&log).push(serde_json::to_string(view).unwrap());
            }),
            Options {
                unit: UNIT,
                config_dir,
                keychain_service: keychain_service.to_owned(),
                device_name: "Test machine".to_owned(),
                platform: "test".to_owned(),
                avatar_dir,
            },
        );
        Self {
            mock,
            store,
            opened,
            events,
            service,
        }
    }

    fn account(&self) -> String {
        self.mock.base().as_str().to_owned()
    }

    /// A sign-in as Hangar finds it at start: only a refresh token in the keychain.
    fn stored_sign_in(&self) {
        let token = "hgrt_mock_stored";
        // Put there before Hangar runs, so not a write of the service under test.
        guard(&self.store.values).insert(self.account(), Zeroizing::new(token.to_owned()));
        self.mock.state().refresh_valid = Some(token.to_owned());
    }

    async fn until(&self, what: &str, done: impl Fn(&AccountState) -> bool) -> AccountState {
        let start = Instant::now();
        loop {
            let state = self.service.view().state;
            if done(&state) {
                return state;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "{what}: still {state:?}"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// No token, device code or refresh token ever reached the window.
    fn assert_events_clean(&self) {
        for event in guard(&self.events).iter() {
            for secret in ["hgat_", "hgrt_", "mock-device-code"] {
                assert!(!event.contains(secret), "{event}");
            }
        }
    }
}

fn run<F: std::future::Future>(future: F) -> F::Output {
    tauri::async_runtime::block_on(future)
}

fn signed_in_with_user(state: &AccountState) -> bool {
    matches!(state, AccountState::SignedIn { user: Some(_) })
}

// ------------------------------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------------------------------

#[test]
fn device_flow_signs_in_and_honors_interval_and_slow_down() {
    let harness = Harness::new();
    harness.mock.state().script =
        VecDeque::from([Poll::Pending, Poll::SlowDown, Poll::Pending, Poll::Tokens]);
    run(async {
        assert!(matches!(
            harness.service.status().await.state,
            AccountState::Idle { notice: None }
        ));
        let view = harness.service.sign_in().await.unwrap();
        let AccountState::WaitingForUser {
            user_code,
            verification_uri_complete,
            interval,
            ..
        } = view.state
        else {
            panic!("{:?}", view.state);
        };
        assert_eq!(user_code, "BCDF-GHJK");
        assert_eq!(interval, 5);
        assert_eq!(*guard(&harness.opened), [verification_uri_complete]);
        // "Open browser again" opens the same page.
        harness.service.open(AccountPage::Verification).unwrap();
        assert_eq!(guard(&harness.opened).len(), 2);

        let state = harness.until("signed in", signed_in_with_user).await;
        let AccountState::SignedIn { user: Some(me) } = state else {
            unreachable!()
        };
        assert_eq!(me.email, "ada@example.test");
        assert_eq!(me.seats_account_total, Some(3));
    });
    let state = harness.mock.state();
    assert_eq!(state.device_requests, 1);
    assert_eq!(state.poll_times.len(), 4);
    let gaps: Vec<Duration> = state
        .poll_times
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .collect();
    // 5 s before the second poll; after slow_down 10 s for every further one.
    assert!(gaps[0] >= UNIT * 5, "{gaps:?}");
    assert!(gaps[1] >= UNIT * 10, "{gaps:?}");
    assert!(gaps[2] >= UNIT * 10, "{gaps:?}");
    drop(state);
    assert_eq!(
        harness.store.value(&harness.account()).as_deref(),
        Some("hgrt_mock_refresh_1")
    );
    assert!(matches!(
        harness.service.open(AccountPage::Verification),
        Err(AccountError::NothingToOpen)
    ));
    harness.assert_events_clean();
}

#[test]
fn declined_sign_in_fails_with_a_reason() {
    let harness = Harness::new();
    harness.mock.state().script = VecDeque::from([Poll::Denied]);
    run(async {
        harness.service.sign_in().await.unwrap();
        let state = harness
            .until("failed", |s| matches!(s, AccountState::Failed { .. }))
            .await;
        let AccountState::Failed { reason } = state else {
            unreachable!()
        };
        assert_eq!(reason.kind, FailKind::Denied);
        // Try again from the failed state, then give up.
        assert!(matches!(
            harness.service.cancel_sign_in().state,
            AccountState::Idle { notice: None }
        ));
    });
    assert!(harness.store.value(&harness.account()).is_none());
}

#[test]
fn polling_stops_when_the_code_expires() {
    let harness = Harness::new();
    // Not on a poll boundary: each poll's own request shifts the next one by its duration, and
    // with 20 a few milliseconds of drift on a busy machine dropped the poll at 15. With 19 the
    // third poll stays due below 19 - 5 = 14 units of elapsed time (40 ms of drift allowed), and
    // a fourth can never fit (15 + 5 >= 19).
    harness.mock.state().expires_in = 19;
    run(async {
        harness.service.sign_in().await.unwrap();
        let state = harness
            .until("expired", |s| matches!(s, AccountState::Failed { .. }))
            .await;
        assert!(matches!(
            state,
            AccountState::Failed { reason } if reason.kind == FailKind::Expired
        ));
    });
    // Polls at 5, 10 and 15; none at or after 19.
    assert_eq!(harness.mock.state().poll_times.len(), 3);
}

#[test]
fn cancel_stops_polling() {
    let harness = Harness::new();
    harness.mock.state().interval = 3;
    run(async {
        harness.service.sign_in().await.unwrap();
        tokio::time::sleep(UNIT * 8).await;
        let view = harness.service.cancel_sign_in();
        assert!(matches!(view.state, AccountState::Idle { notice: None }));
        let polls = harness.mock.state().poll_times.len();
        tokio::time::sleep(UNIT * 20).await;
        // At most the one request that was already on its way.
        assert!(harness.mock.state().poll_times.len() <= polls + 1);
        assert!(matches!(
            harness.service.view().state,
            AccountState::Idle { notice: None }
        ));
    });
}

#[test]
fn a_foreign_confirmation_page_is_refused() {
    let harness = Harness::new();
    harness.mock.state().page_origin = Some("https://elsewhere.example.test".to_owned());
    run(async {
        let view = harness.service.sign_in().await.unwrap();
        assert!(matches!(
            view.state,
            AccountState::Failed { reason } if reason.kind == FailKind::ForeignPage
        ));
    });
    assert!(guard(&harness.opened).is_empty());
    assert!(harness.mock.state().poll_times.is_empty());
}

#[test]
fn concurrent_callers_share_one_refresh() {
    let harness = Harness::new();
    harness.stored_sign_in();
    harness.mock.state().refresh_delay = Duration::from_millis(150);
    let first = harness.service.clone();
    let second = harness.service.clone();
    let (a, b) = run(async move {
        let a = tauri::async_runtime::spawn(async move { first.projects().await });
        let b = tauri::async_runtime::spawn(async move { second.projects().await });
        (a.await.unwrap(), b.await.unwrap())
    });
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a, b);
    assert_eq!(a.len(), 2);
    let state = harness.mock.state();
    assert_eq!(state.refreshes, 1, "exactly one refresh request");
    assert!(!state.revoked);
    drop(state);
    // Rotation: the keychain holds the new token, the stored one is spent.
    assert_eq!(
        harness.store.value(&harness.account()).as_deref(),
        Some("hgrt_mock_refresh_1")
    );
    assert!(harness
        .mock
        .state()
        .refresh_used
        .contains("hgrt_mock_stored"));
}

#[test]
fn a_refused_access_token_is_refreshed_with_the_rotated_token() {
    let harness = Harness::new();
    harness.stored_sign_in();
    run(async {
        harness.service.projects().await.unwrap();
        // The server forgets the access token (it expired early): 401, one refresh, retry.
        harness.mock.state().access_valid.clear();
        let projects = harness.service.projects().await.unwrap();
        assert_eq!(projects[0].name, "Core");
    });
    let state = harness.mock.state();
    assert_eq!(state.refreshes, 2);
    assert!(!state.revoked, "no refresh token was used twice");
    drop(state);
    assert_eq!(
        harness.store.value(&harness.account()).as_deref(),
        Some("hgrt_mock_refresh_2")
    );
}

#[test]
fn invalid_grant_signs_out_with_a_reason() {
    let harness = Harness::new();
    harness
        .store
        .set(&harness.account(), "hgrt_unknown_to_the_server")
        .unwrap();
    run(async {
        assert!(matches!(
            harness.service.status().await.state,
            AccountState::SignedIn { .. }
        ));
        let error = harness.service.projects().await.unwrap_err();
        assert_eq!(error, AccountError::SessionEnded);
        let state = harness
            .until("signed out", |s| matches!(s, AccountState::Idle { .. }))
            .await;
        assert_eq!(
            state,
            AccountState::Idle {
                notice: Some(IdleNotice::SessionEnded)
            }
        );
    });
    assert!(harness.store.value(&harness.account()).is_none());
    harness.assert_events_clean();
}

#[test]
fn problems_reach_the_window_as_server_errors() {
    let harness = Harness::new();
    harness.stored_sign_in();
    harness.mock.state().projects_failure = Some((503, "Maintenance until noon".to_owned()));
    run(async {
        let error = harness.service.projects().await.unwrap_err();
        assert_eq!(
            error,
            AccountError::Server(Some("Maintenance until noon".to_owned()))
        );
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["kind"], "server");
    });
}

#[test]
fn invite_pages_open_only_for_owners_and_admins() {
    let harness = Harness::new();
    harness.stored_sign_in();
    run(async {
        let projects = harness.service.projects().await.unwrap();
        assert!(projects[0].can_invite);
        assert!(!projects[1].can_invite);
    });
    harness
        .service
        .open(AccountPage::Invite {
            project_id: "317".into(),
        })
        .unwrap();
    assert_eq!(
        *guard(&harness.opened),
        [format!("{}/projects/317#invite", harness.mock.origin)]
    );
    assert_eq!(
        harness.service.open(AccountPage::Invite {
            project_id: "318".into()
        }),
        Err(AccountError::ForeignPage)
    );
    assert_eq!(
        harness.service.open(AccountPage::Invite {
            project_id: "999".into()
        }),
        Err(AccountError::UnknownProject)
    );
    harness.service.open(AccountPage::Devices).unwrap();
    assert_eq!(
        guard(&harness.opened).last().map(String::as_str),
        Some(format!("{}/account/devices", harness.mock.origin).as_str())
    );
}

#[test]
fn sign_out_drops_the_tokens() {
    let harness = Harness::new();
    harness.stored_sign_in();
    run(async {
        harness.service.projects().await.unwrap();
        let view = harness.service.sign_out().await.unwrap();
        assert_eq!(
            view.state,
            AccountState::Idle {
                notice: Some(IdleNotice::SignedOut)
            }
        );
        assert_eq!(
            harness.service.projects().await.unwrap_err(),
            AccountError::NotSignedIn
        );
    });
    assert!(harness.store.value(&harness.account()).is_none());
}

#[test]
fn switching_servers_keeps_each_sign_in() {
    let harness = Harness::new();
    harness.stored_sign_in();
    let own = harness.account();
    run(async {
        assert!(matches!(
            harness.service.status().await.state,
            AccountState::SignedIn { .. }
        ));
        let refused = harness
            .service
            .set_base_url(Some("http://example.test/api/v1".into()))
            .await;
        assert!(matches!(refused, Err(AccountError::BaseUrl(_))));
        let other = harness
            .service
            .set_base_url(Some("https://team.example.test/api/v1".into()))
            .await
            .unwrap();
        assert_eq!(other.base_url, "https://team.example.test/api/v1");
        assert!(matches!(other.state, AccountState::Idle { notice: None }));
        let back = harness
            .service
            .set_base_url(Some(own.clone()))
            .await
            .unwrap();
        assert!(matches!(back.state, AccountState::SignedIn { .. }));
        let official = harness.service.set_base_url(None).await.unwrap();
        assert!(official.official);
    });
}

#[test]
fn a_server_change_that_cannot_be_saved_ends_the_waiting_flow() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/.tmp")
        .join(format!("account-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // A file where the config folder should be: saving `account.json` fails.
    let blocked = dir.join("not-a-folder");
    std::fs::write(&blocked, "x").unwrap();
    let mock = Mock::start();
    let service = AccountService::new(
        mock.base(),
        Arc::new(MemoryStore::default()),
        Arc::new(|_: &reqwest::Url| Ok(())),
        Arc::new(|_: &AccountView| {}),
        Options {
            unit: UNIT,
            config_dir: Some(blocked),
            keychain_service: KEYCHAIN_SERVICE.to_owned(),
            device_name: "Test machine".to_owned(),
            platform: "test".to_owned(),
            avatar_dir: None,
        },
    );
    run(async {
        let view = service.sign_in().await.unwrap();
        assert!(matches!(view.state, AccountState::WaitingForUser { .. }));
        let error = service
            .set_base_url(Some("https://team.example.test/api/v1".into()))
            .await
            .unwrap_err();
        assert!(matches!(error, AccountError::Config(_)));
        let view = service.view();
        assert!(matches!(view.state, AccountState::Idle { notice: None }));
        assert_eq!(view.base_url, mock.base().as_str());
    });
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn relay_tickets_and_reserved_endpoints() {
    let mock = Mock::start();
    let api = AccountApi::new(mock.base()).unwrap();
    run(async {
        let tokens = {
            let mut state = mock.state();
            state.issued = 40;
            let value = issue(&mut state);
            value["access_token"].as_str().unwrap().to_owned()
        };
        let ticket = api.relay_ticket(&tokens, "317").await.unwrap();
        assert_eq!(ticket.relay_url.as_str(), "wss://relay.example.test/");
        assert_eq!(ticket.expires_at, 1_794_225_660);
        let missing = api.relay_ticket(&tokens, "999").await.unwrap_err();
        assert!(matches!(
            missing,
            hangar_lib::account_api::ApiError::Status { status: 404, .. }
        ));
        let unauthorized = api.relay_ticket("hgat_wrong", "317").await.unwrap_err();
        assert_eq!(
            unauthorized,
            hangar_lib::account_api::ApiError::Unauthorized
        );
    });
}

#[test]
fn plain_http_is_refused_for_other_hosts() {
    assert!(BaseUrl::parse("http://192.0.2.1/api/v1").is_err());
    // The client for an https base never talks plain http.
    let base = BaseUrl::parse("https://127.0.0.1:9/api/v1").unwrap();
    assert!(base.is_https());
}

/// A fresh folder under `tests/.tmp` for one test's `account.json` and lock file.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/.tmp")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn two_hangar_processes_share_one_sign_in_without_reuse() {
    // Two Hangar processes (say, the installed app and a dev build) on one keychain entry and one
    // config folder: each has its own service, refresh gate and memory.
    let dir = temp_dir("account-two-processes");
    let mock = Mock::start();
    let store = Arc::new(MemoryStore::default());
    let first = Harness::configured(mock.clone(), Arc::clone(&store), Some(dir.clone()));
    let second = Harness::configured(mock.clone(), Arc::clone(&store), Some(dir.clone()));
    first.stored_sign_in();
    mock.state().refresh_delay = Duration::from_millis(150);
    run(async {
        for harness in [&first, &second] {
            assert!(matches!(
                harness.service.status().await.state,
                AccountState::SignedIn { .. }
            ));
        }
        // Both find no access token and refresh at the same time.
        let a = first.service.clone();
        let b = second.service.clone();
        let a = tauri::async_runtime::spawn(async move { a.projects().await });
        let b = tauri::async_runtime::spawn(async move { b.projects().await });
        assert_eq!(a.await.unwrap().unwrap().len(), 2);
        assert_eq!(b.await.unwrap().unwrap().len(), 2);
        // Later the first one refreshes again, holding a token the second already spent.
        mock.state().access_valid.clear();
        mock.state().refresh_delay = Duration::ZERO;
        assert_eq!(first.service.projects().await.unwrap().len(), 2);
    });
    let state = mock.state();
    assert!(!state.revoked, "no refresh token was presented twice");
    assert_eq!(
        state.grace_used, 0,
        "no refresh relied on the server's grace"
    );
    let latest = state.refresh_valid.clone();
    drop(state);
    assert_eq!(store.value(&first.account()), latest);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_lost_refresh_answer_is_retried_within_the_grace() {
    let harness = Harness::new();
    harness.stored_sign_in();
    // The server rotates, the answer is lost (timeout, dropped connection).
    harness.mock.state().lose_refresh_answers = 1;
    run(async {
        let projects = harness.service.projects().await.unwrap();
        assert_eq!(projects.len(), 2);
    });
    let state = harness.mock.state();
    assert_eq!(state.refreshes, 2, "one retry with the same token");
    assert_eq!(state.grace_used, 1);
    assert!(!state.revoked);
    let latest = state.refresh_valid.clone();
    drop(state);
    // The pair of the grace is the one stored.
    assert_eq!(harness.store.value(&harness.account()), latest);
}

#[test]
fn sign_out_revokes_the_device_on_the_server() {
    let harness = Harness::new();
    harness.stored_sign_in();
    run(async {
        harness.service.projects().await.unwrap();
        let view = harness.service.sign_out().await.unwrap();
        assert_eq!(
            view.state,
            AccountState::Idle {
                notice: Some(IdleNotice::SignedOut)
            }
        );
    });
    let state = harness.mock.state();
    assert_eq!(state.revocations.len(), 1);
    let body = &state.revocations[0];
    assert_eq!(body["token"], "hgrt_mock_refresh_1");
    assert_eq!(body["token_type_hint"], "refresh_token");
    assert_eq!(body["client_id"], "hangar-desktop");
    assert!(state.refresh_valid.is_none(), "the device grant is gone");
    drop(state);
    assert!(harness.store.value(&harness.account()).is_none());
    harness.assert_events_clean();
}

#[test]
fn sign_out_signs_out_locally_when_the_server_refuses_the_revoke() {
    let harness = Harness::new();
    harness.stored_sign_in();
    harness.mock.state().revoke_failure = Some("invalid_client");
    run(async {
        let view = harness.service.sign_out().await.unwrap();
        assert_eq!(
            view.state,
            AccountState::Idle {
                notice: Some(IdleNotice::SignedOutLocally)
            }
        );
    });
    assert_eq!(harness.mock.state().revocations.len(), 1);
    assert!(harness.store.value(&harness.account()).is_none());
}

#[test]
fn sign_out_names_the_sign_ins_kept_for_other_servers() {
    let harness = Harness::new();
    harness.stored_sign_in();
    let own = harness.account();
    run(async {
        harness.service.status().await;
        let other = harness
            .service
            .set_base_url(Some("https://team.example.test/api/v1".into()))
            .await
            .unwrap();
        assert_eq!(other.other_sign_ins, std::slice::from_ref(&own));
        let back = harness
            .service
            .set_base_url(Some(own.clone()))
            .await
            .unwrap();
        assert!(back.other_sign_ins.is_empty());
        let out = harness.service.sign_out().await.unwrap();
        assert!(out.other_sign_ins.is_empty());
        let official = harness.service.set_base_url(None).await.unwrap();
        assert!(official.other_sign_ins.is_empty(), "signed out everywhere");
    });
}

#[test]
fn the_window_gets_the_checked_verification_page() {
    let harness = Harness::new();
    // Same origin, but not in normal form.
    harness.mock.state().page_origin = Some(format!("{}/x/..", harness.mock.origin));
    run(async {
        let view = harness.service.sign_in().await.unwrap();
        let AccountState::WaitingForUser {
            verification_uri,
            verification_uri_complete,
            ..
        } = view.state
        else {
            panic!("{:?}", view.state);
        };
        assert_eq!(verification_uri, format!("{}/device", harness.mock.origin));
        assert_eq!(
            verification_uri_complete,
            format!("{}/device?user_code=BCDF-GHJK", harness.mock.origin)
        );
        harness.service.cancel_sign_in();
    });
}

#[test]
fn debug_and_release_builds_keep_their_own_sign_in_lists() {
    // A release and a debug build share the config folder but use separate keychain services.
    let dir = temp_dir("account-two-services");
    let (release_mock, dev_mock) = (Mock::start(), Mock::start());
    let release = Harness::configured_as(
        release_mock.clone(),
        Arc::new(MemoryStore::default()),
        Some(dir.clone()),
        "hangar-account",
    );
    let dev = Harness::configured_as(
        dev_mock.clone(),
        Arc::new(MemoryStore::default()),
        Some(dir.clone()),
        "hangar-account-dev",
    );
    release.stored_sign_in();
    dev.stored_sign_in();
    run(async {
        release.service.status().await;
        dev.service.status().await;
    });
    let config = load_config(&dir);
    assert_eq!(
        config.signed_in_for("hangar-account"),
        [release_mock.base()]
    );
    assert_eq!(
        config.signed_in_for("hangar-account-dev"),
        [dev_mock.base()]
    );
    run(async {
        // Each build names only the sign-ins in its own keychain service.
        let view = release
            .service
            .set_base_url(Some(dev.account()))
            .await
            .unwrap();
        assert!(matches!(view.state, AccountState::Idle { notice: None }));
        assert_eq!(view.other_sign_ins, [release.account()]);
        let view = dev
            .service
            .set_base_url(Some(release.account()))
            .await
            .unwrap();
        assert_eq!(view.other_sign_ins, [dev.account()]);
    });
    let config = load_config(&dir);
    assert_eq!(config.base, release_mock.base(), "the last chosen server");
    assert_eq!(
        config.signed_in_for("hangar-account"),
        [release_mock.base()]
    );
    assert_eq!(
        config.signed_in_for("hangar-account-dev"),
        [dev_mock.base()]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sign_in_list_written_by_another_process_is_kept() {
    // Two processes of one build, started before either wrote the list.
    let dir = temp_dir("account-list-reload");
    let store = Arc::new(MemoryStore::default());
    let (first_mock, second_mock) = (Mock::start(), Mock::start());
    let first = Harness::configured(first_mock.clone(), Arc::clone(&store), Some(dir.clone()));
    let second = Harness::configured(second_mock.clone(), Arc::clone(&store), Some(dir.clone()));
    first.stored_sign_in();
    second.stored_sign_in();
    run(async {
        first.service.status().await;
        second.service.status().await;
    });
    assert_eq!(
        load_config(&dir).signed_in_for(KEYCHAIN_SERVICE),
        [first_mock.base(), second_mock.base()]
    );
    // The second process learned the first one's entry when it wrote.
    assert_eq!(second.service.view().other_sign_ins, [first.account()]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sign_in_is_stored_under_the_process_lock() {
    let dir = temp_dir("account-sign-in-lock");
    let store = Arc::new(MemoryStore::watching(&dir));
    let harness = Harness::configured(Mock::start(), Arc::clone(&store), Some(dir.clone()));
    harness.mock.state().script = VecDeque::from([Poll::Tokens]);
    run(async {
        harness.service.sign_in().await.unwrap();
        harness.until("signed in", signed_in_with_user).await;
    });
    assert!(harness.store.value(&harness.account()).is_some());
    assert_eq!(store.unlocked_writes(), Vec::<&str>::new());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_second_refusal_ends_the_session_under_the_process_lock() {
    let dir = temp_dir("account-end-session-lock");
    let store = Arc::new(MemoryStore::watching(&dir));
    let harness = Harness::configured(Mock::start(), Arc::clone(&store), Some(dir.clone()));
    harness.stored_sign_in();
    harness.mock.state().projects_always_unauthorized = true;
    run(async {
        // 401, one refresh, 401 again: the sign-in ends.
        assert_eq!(
            harness.service.projects().await.unwrap_err(),
            AccountError::SessionEnded
        );
    });
    // One refresh for the first access token, one after the first 401.
    assert_eq!(harness.mock.state().refreshes, 2);
    assert!(harness.store.value(&harness.account()).is_none());
    assert_eq!(store.unlocked_writes(), Vec::<&str>::new());
    assert!(load_config(&dir).signed_in_for(KEYCHAIN_SERVICE).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_me_is_retried_until_the_user_arrives() {
    let harness = Harness::new();
    harness.stored_sign_in();
    harness.mock.state().me_failures = 2;
    run(async {
        assert!(matches!(
            harness.service.status().await.state,
            AccountState::SignedIn { user: None }
        ));
        harness.until("user loaded", signed_in_with_user).await;
    });
    assert_eq!(harness.mock.state().me_requests, 3);
    // The window got the person, not only the backend.
    let last = guard(&harness.events).last().cloned().unwrap();
    assert!(last.contains("ada@example.test"), "{last}");
}

#[test]
fn status_loads_the_user_when_another_caller_read_the_sign_in() {
    // A team connection asks for a relay ticket before the window's first status: the sign-in is
    // read from the keychain on that path, and the status must still learn who is signed in.
    let harness = Harness::new();
    harness.stored_sign_in();
    run(async {
        harness.service.relay_ticket("317").await.unwrap();
        assert!(matches!(
            harness.service.status().await.state,
            AccountState::SignedIn { .. }
        ));
        harness.until("user loaded", signed_in_with_user).await;
    });
}

const AVATAR_ETAG: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
const WEBP: &[u8] = b"RIFF\x10\0\0\0WEBPVP8 picture";

impl Harness {
    /// Waits until the picture the backend holds satisfies `done`.
    async fn until_avatar(
        &self,
        what: &str,
        done: impl Fn(Option<&str>) -> bool,
    ) -> Option<String> {
        let start = Instant::now();
        loop {
            let avatar = self.service.avatar();
            if done(avatar.as_deref()) {
                return avatar;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "{what}");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Waits until the mock saw `count` picture requests, then a little longer.
    async fn until_avatar_requests(&self, count: usize) {
        let start = Instant::now();
        while self.mock.state().avatar_requests.len() < count {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "picture requests"
            );
            tokio::time::sleep(UNIT).await;
        }
        tokio::time::sleep(UNIT * 5).await;
    }
}

fn avatar_marker(state: &AccountState) -> Option<u64> {
    match state {
        AccountState::SignedIn { user: Some(me) } => me.avatar_version,
        _ => None,
    }
}

/// A started service with a picture cache in `dir`, signed in from the keychain.
fn with_avatar(mock: Mock, dir: &std::path::Path) -> Harness {
    let store = Arc::new(MemoryStore::default());
    let harness = Harness::built(
        mock,
        store,
        None,
        KEYCHAIN_SERVICE,
        Some(dir.join("account-avatar")),
    );
    harness.stored_sign_in();
    harness
}

#[test]
fn the_profile_picture_is_downloaded_cached_and_reused() {
    let dir = temp_dir("account-avatar");
    let mock = Mock::start();
    mock.state().avatar = Some(("image/webp", WEBP.to_vec(), AVATAR_ETAG.to_owned()));
    let first = with_avatar(mock.clone(), &dir);
    let data = run(async {
        first.service.status().await;
        first.until_avatar("picture", |a| a.is_some()).await
    })
    .unwrap();
    assert!(data.starts_with("data:image/webp;base64,"), "{data}");
    assert_eq!(mock.state().avatar_requests, [None]);
    // The view carries only a marker; neither the picture, the remote address nor a token
    // reaches the window's events.
    run(first.until("marker", |s| avatar_marker(s).is_some()));
    for event in guard(&first.events).iter() {
        assert!(
            !event.contains("/me/avatar") && !event.contains("base64"),
            "{event}"
        );
    }
    first.assert_events_clean();

    // The next start: the etag is unchanged, the cached picture is shown without a request.
    let second = with_avatar(mock.clone(), &dir);
    run(async {
        second.service.status().await;
        second.until("user", signed_in_with_user).await;
    });
    assert_eq!(second.service.avatar(), Some(data.clone()));
    assert!(avatar_marker(&second.service.view().state).is_some());
    assert_eq!(mock.state().avatar_requests.len(), 1);

    // A new picture: downloaded with the cached etag as If-None-Match, then shown.
    let changed = "a".repeat(64);
    mock.state().avatar = Some(("image/png", b"\x89PNG\r\n\x1a\nnew".to_vec(), changed));
    let third = with_avatar(mock.clone(), &dir);
    run(async {
        third.service.status().await;
        third
            .until_avatar("new picture", |a| {
                a.is_some_and(|d| d.starts_with("data:image/png;"))
            })
            .await;
    });
    assert_eq!(
        mock.state().avatar_requests.last().cloned().flatten(),
        Some(format!("\"{AVATAR_ETAG}\""))
    );

    // Sign-out removes the cached copy.
    run(async {
        third.service.sign_out().await.unwrap();
    });
    assert!(!dir.join("account-avatar").join("avatar.img").exists());
    assert_eq!(third.service.avatar(), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_profile_picture_elsewhere_or_of_the_wrong_kind_is_refused() {
    let dir = temp_dir("account-avatar-refused");
    let mock = Mock::start();
    mock.state().avatar = Some(("image/webp", WEBP.to_vec(), AVATAR_ETAG.to_owned()));
    mock.state().avatar_origin = Some("https://elsewhere.example.test".to_owned());
    let foreign = with_avatar(mock.clone(), &dir);
    run(async {
        foreign.service.status().await;
        foreign.until("user", signed_in_with_user).await;
        tokio::time::sleep(UNIT * 5).await;
    });
    assert_eq!(foreign.service.avatar(), None);
    assert!(mock.state().avatar_requests.is_empty());

    // On the server's own origin, but outside its API path: the token stays home.
    mock.state().avatar_origin = None;
    mock.state().avatar_path = Some("/other-app/avatar".to_owned());
    let outside = with_avatar(mock.clone(), &dir);
    run(async {
        outside.service.status().await;
        outside.until("user", signed_in_with_user).await;
        tokio::time::sleep(UNIT * 5).await;
    });
    assert_eq!(outside.service.avatar(), None);
    assert_eq!(mock.state().forbidden_hits, 0);
    mock.state().avatar_path = None;

    // On the server's own address, but an SVG and a picture over 1 MiB are refused.
    mock.state().avatar_origin = None;
    for (content_type, bytes) in [
        ("image/svg+xml", b"<svg/>".to_vec()),
        ("image/webp", {
            let mut large = WEBP.to_vec();
            large.resize(1024 * 1024 + 1, 0);
            large
        }),
    ] {
        mock.state().avatar = Some((content_type, bytes, AVATAR_ETAG.to_owned()));
        let before = mock.state().avatar_requests.len();
        let harness = with_avatar(mock.clone(), &dir);
        run(async {
            harness.service.status().await;
            harness.until("user", signed_in_with_user).await;
            let start = Instant::now();
            while mock.state().avatar_requests.len() == before {
                assert!(start.elapsed() < Duration::from_secs(10));
                tokio::time::sleep(UNIT).await;
            }
            tokio::time::sleep(UNIT * 5).await;
        });
        assert_eq!(harness.service.avatar(), None, "{content_type}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn published_views_carry_a_rising_revision() {
    let harness = Harness::new();
    harness.stored_sign_in();
    run(async {
        harness.service.status().await;
        harness.until("user", signed_in_with_user).await;
    });
    let revisions: Vec<u64> = guard(&harness.events)
        .iter()
        .map(|event| {
            serde_json::from_str::<serde_json::Value>(event).unwrap()["revision"]
                .as_u64()
                .unwrap()
        })
        .collect();
    assert!(revisions.len() >= 2, "{revisions:?}");
    assert!(
        revisions.windows(2).all(|pair| pair[0] < pair[1]),
        "{revisions:?}"
    );
    // A command answer carries the revision of the newest published view.
    assert_eq!(harness.service.view().revision, *revisions.last().unwrap());
}

#[test]
fn avatar_401_twice_keeps_the_sign_in() {
    let dir = temp_dir("account-avatar-refused-token");
    let mock = Mock::start();
    mock.state().avatar = Some(("image/webp", WEBP.to_vec(), AVATAR_ETAG.to_owned()));
    mock.state().avatar_mode = AvatarMode::Unauthorized;
    let harness = with_avatar(mock.clone(), &dir);
    run(async {
        harness.service.status().await;
        harness.until("user", signed_in_with_user).await;
        // One try, one ordinary refresh, one more try; then only the picture is dropped.
        harness.until_avatar_requests(2).await;
        assert_eq!(harness.service.avatar(), None);
        assert!(signed_in_with_user(&harness.service.view().state));
        harness.service.projects().await.unwrap();
    });
    let state = mock.state();
    assert_eq!(state.avatar_requests.len(), 2);
    assert_eq!(
        state.refreshes, 2,
        "the start's refresh and one after the first 401"
    );
    assert!(!state.revoked);
    drop(state);
    assert!(
        harness.store.value(&harness.account()).is_some(),
        "still signed in"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_picture_client_follows_no_redirect_and_stops_at_the_limit() {
    let mock = Mock::start();
    let api = AccountApi::new(mock.base()).unwrap();
    let token = {
        let mut state = mock.state();
        state.issued = 60;
        state.avatar = Some(("image/webp", WEBP.to_vec(), AVATAR_ETAG.to_owned()));
        let value = issue(&mut state);
        value["access_token"].as_str().unwrap().to_owned()
    };
    let url = format!("{}/me/avatar?v=0123456789abcdef", mock.base().as_str());
    run(async {
        mock.state().avatar_mode = AvatarMode::Redirect;
        let redirected = api.avatar(&url, &token, None).await.unwrap_err();
        assert!(
            matches!(redirected, ApiError::Status { status: 302, .. }),
            "{redirected:?}"
        );
        assert_eq!(
            mock.state().forbidden_hits,
            0,
            "the redirect was not followed"
        );

        // No Content-Length, chunks beyond 1 MiB: reading stops at the limit.
        mock.state().avatar_mode = AvatarMode::Endless;
        let endless = api.avatar(&url, &token, None).await.unwrap_err();
        assert_eq!(endless, ApiError::TooLarge);

        // A 401 is a plain status error here, never the sign-in's `Unauthorized`.
        mock.state().avatar_mode = AvatarMode::Unauthorized;
        let refused = api.avatar(&url, &token, None).await.unwrap_err();
        assert!(
            matches!(refused, ApiError::Status { status: 401, .. }),
            "{refused:?}"
        );
        assert_eq!(refused.summary(), "HTTP 401 (unauthorized)");

        // Outside the API path the token is never sent.
        let before = mock.state().avatar_requests.len();
        let outside = format!("{}/other-app/avatar", mock.origin);
        assert_eq!(
            api.avatar(&outside, &token, None).await.unwrap_err(),
            ApiError::Malformed
        );
        assert_eq!(mock.state().avatar_requests.len(), before);
        assert_eq!(mock.state().forbidden_hits, 0);
    });
}

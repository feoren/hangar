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
    AccountError, AccountPage, AccountState, AccountView, BaseUrl, FailKind, IdleNotice,
};
use hangar_lib::account_api::AccountApi;
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
}

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
    let body = serde_json::from_slice(&data[head_end..head_end + length])
        .unwrap_or(serde_json::Value::Null);
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned);
    Some(Request {
        method,
        path,
        bearer,
        body,
    })
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
                if state.refresh_valid.as_deref() == Some(token.as_str()) && !state.revoked {
                    state.refresh_used.insert(token);
                    let value = issue(&mut state);
                    drop(state);
                    json(&mut stream, 200, &value);
                } else {
                    if state.refresh_used.contains(&token) {
                        // Reuse: the whole device is revoked.
                        state.revoked = true;
                        state.refresh_valid = None;
                        state.access_valid.clear();
                    }
                    drop(state);
                    oauth_error(&mut stream, "invalid_grant");
                }
            } else {
                oauth_error(&mut stream, "unsupported_grant_type");
            }
        }
        ("GET", "/api/v1/me") => {
            let state = guard(shared);
            if authorized(&state, &request) {
                drop(state);
                let value = serde_json::json!({
                    "id": "7", "email": "ada@example.test", "name": "Ada", "locale": "de",
                    "seats_account_total": 3, "seats_account_used": 2,
                });
                json(&mut stream, 200, &value);
            } else {
                drop(state);
                problem(&mut stream, 401, "unauthorized", "Sign in again");
            }
        }
        ("GET", "/api/v1/projects") => {
            let state = guard(shared);
            if !authorized(&state, &request) {
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
}

impl MemoryStore {
    fn value(&self, account: &str) -> Option<String> {
        guard(&self.values).get(account).map(|v| v.to_string())
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
        guard(&self.values).insert(account.to_owned(), Zeroizing::new(value.to_owned()));
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), SecretError> {
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
                config_dir: None,
                device_name: "Test machine".to_owned(),
                platform: "test".to_owned(),
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
        self.store.set(&self.account(), token).unwrap();
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
    harness.mock.state().expires_in = 20;
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
    // Polls at 5, 10 and 15; none at or after 20.
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
            device_name: "Test machine".to_owned(),
            platform: "test".to_owned(),
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
        assert_eq!(ticket.relay_url, "wss://relay.example.test");
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

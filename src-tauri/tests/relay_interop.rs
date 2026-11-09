//! Interop with a real Hangar team relay in self-host mode (`IDENTITY=local`). Ignored by default;
//! it needs a relay started by hand on the loopback and two one-time sign-in links:
//!
//! ```text
//! HANGAR_INTEROP_BASE=http://127.0.0.1:8080          the relay's PUBLIC_URL
//! HANGAR_INTEROP_LINK_A=<login link for the owner>   `hangar-team-server login-link a@example.org`
//! HANGAR_INTEROP_LINK_B=<login link for the member>  `hangar-team-server login-link b@example.org`
//! cargo test --test relay_interop -- --ignored --nocapture
//! ```
//!
//! Without `HANGAR_INTEROP_BASE` the test returns at once. Nothing here knows a server secret.
//!
//! Part 1 points the app's own account client (`AccountApi`) straight at the relay's self-host API
//! under three plausible bases and records every endpoint that does not answer the way the app
//! expects. Part 2 bridges that gap with an in-test adapter on 127.0.0.1 that speaks the website
//! API the app knows (`/api/v1/oauth/device`, `/oauth/token`, `/me`, `/projects`,
//! `/relay-ticket`) and translates each call onto the relay's self-host API. Through it the real
//! `AccountService` signs in two accounts with the device flow, and the real `TeamService` runs
//! both against the real relay: team creation, a new member confirmed into the membership log,
//! presence, a claim overlap and ticket renewals. The test fails at the end while part 1 found a
//! mismatch, so it turns green only once the relay serves the app's API itself.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hangar_lib::account::{AccountState, AccountView, BaseUrl};
use hangar_lib::account_api::AccountApi;
use hangar_lib::account_service::{AccountService, Options as AccountOptions};
use hangar_lib::claims::{ChangeKind, Claim, LineRange, OverlapLevel};
use hangar_lib::secrets::{SecretError, SecretStore};
use hangar_lib::team::{TeamSnapshot, TeamStatus, Timing};
use hangar_lib::team_service::{Options, TeamService};
use hangar_lib::team_share::{LocalSession, SessionState};
use zeroize::Zeroizing;

const FOLDER: &str = "c:/src/shop/.git";
/// Real seconds: the relay counts its device-flow interval in seconds.
const WAIT: Duration = Duration::from_secs(90);

fn guard<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

// ------------------------------------------------------------------------------------------
// In-memory keychain and test folders
// ------------------------------------------------------------------------------------------

#[derive(Default)]
struct MemoryStore(Mutex<HashMap<String, Zeroizing<String>>>);

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, SecretError> {
        Ok(guard(&self.0).get(account).cloned())
    }
    fn exists(&self, account: &str) -> Result<bool, SecretError> {
        Ok(guard(&self.0).contains_key(account))
    }
    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        guard(&self.0).insert(account.to_owned(), Zeroizing::new(value.to_owned()));
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), SecretError> {
        guard(&self.0).remove(account);
        Ok(())
    }
}

/// A folder under `<repo>/tests/.tmp/`, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let dir = manifest
            .parent()
            .unwrap()
            .join("tests")
            .join(".tmp")
            .join(format!("interop-{name}-{}", std::process::id()));
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

// ------------------------------------------------------------------------------------------
// Plain HTTP helpers (reqwest without its json/form features, as the app builds it)
// ------------------------------------------------------------------------------------------

fn client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap()
}

fn form(pairs: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (key, value) in pairs {
        if !out.is_empty() {
            out.push('&');
        }
        out.push_str(key);
        out.push('=');
        for byte in value.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                out.push(char::from(byte));
            } else {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

struct Reply {
    status: u16,
    body: Vec<u8>,
    cookies: Vec<String>,
}

impl Reply {
    fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

enum Body<'a> {
    None,
    Form(String),
    Json(&'a serde_json::Value),
}

async fn call(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    headers: &[(&str, &str)],
    body: Body<'_>,
) -> Reply {
    let mut request = client.request(method, url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request = match body {
        Body::None => request,
        Body::Form(text) => request
            .header("content-type", "application/x-www-form-urlencoded")
            .body(text),
        Body::Json(value) => request
            .header("content-type", "application/json")
            .body(value.to_string()),
    };
    let response = request.send().await.expect("relay reachable");
    let status = response.status().as_u16();
    let cookies = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_owned))
        .collect();
    let body = response
        .bytes()
        .await
        .map(|b| b.to_vec())
        .unwrap_or_default();
    Reply {
        status,
        body,
        cookies,
    }
}

// ------------------------------------------------------------------------------------------
// The relay's own sign-in: magic link, device page, device flow
// ------------------------------------------------------------------------------------------

/// A browser signed in at the relay through a login link (session cookie `hangar_team`).
struct Browser {
    origin: String,
    cookie: String,
}

impl Browser {
    async fn sign_in(client: &reqwest::Client, origin: &str, link: &str) -> Self {
        assert!(link.starts_with(origin), "the link belongs to {origin}");
        let reply = call(
            client,
            reqwest::Method::POST,
            link,
            &[("origin", origin)],
            Body::None,
        )
        .await;
        assert_eq!(reply.status, 303, "login link: {}", reply.text());
        let cookie = reply
            .cookies
            .iter()
            .filter_map(|c| c.split(';').next())
            .find(|c| c.starts_with("hangar_team=") && c.len() > "hangar_team=".len())
            .expect("session cookie")
            .to_owned();
        Self {
            origin: origin.to_owned(),
            cookie,
        }
    }

    /// Approves `user_code` on the relay's `/device` page, as the person would in the browser.
    async fn approve(&self, client: &reqwest::Client, user_code: &str) {
        let page = call(
            client,
            reqwest::Method::GET,
            &format!("{}/device?code={user_code}", self.origin),
            &[("cookie", &self.cookie)],
            Body::None,
        )
        .await;
        assert_eq!(page.status, 200, "device page: {}", page.text());
        let html = page.text();
        let marker = r#"name="csrf" value=""#;
        let start = html.find(marker).expect("csrf field") + marker.len();
        let csrf = &html[start..start + html[start..].find('"').unwrap()];
        let decided = call(
            client,
            reqwest::Method::POST,
            &format!("{}/device", self.origin),
            &[("cookie", &self.cookie), ("origin", &self.origin)],
            Body::Form(form(&[
                ("csrf", csrf),
                ("user_code", user_code),
                ("decision", "approve"),
            ])),
        )
        .await;
        assert_eq!(decided.status, 200, "approve: {}", decided.text());
    }

    /// An access token straight from the relay's device flow (for the team and invitation calls
    /// the app does not make, and for part 1).
    async fn access_token(&self, client: &reqwest::Client) -> String {
        let started = call(
            client,
            reqwest::Method::POST,
            &format!("{}/oauth/device_authorization", self.origin),
            &[],
            Body::Form(form(&[("client_id", "hangar"), ("device_name", "interop")])),
        )
        .await;
        assert_eq!(started.status, 200, "device flow: {}", started.text());
        let started = started.json();
        let device_code = started["device_code"].as_str().unwrap().to_owned();
        self.approve(client, started["user_code"].as_str().unwrap())
            .await;
        let interval = started["interval"].as_u64().unwrap_or(5);
        let deadline = Instant::now() + WAIT;
        loop {
            tokio::time::sleep(Duration::from_secs(interval + 1)).await;
            let reply = call(
                client,
                reqwest::Method::POST,
                &format!("{}/oauth/token", self.origin),
                &[],
                Body::Form(form(&[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("device_code", &device_code),
                    ("client_id", "hangar"),
                ])),
            )
            .await;
            if reply.status == 200 {
                return reply.json()["access_token"].as_str().unwrap().to_owned();
            }
            assert!(Instant::now() < deadline, "token: {}", reply.text());
        }
    }
}

async fn relay_api(
    client: &reqwest::Client,
    origin: &str,
    method: reqwest::Method,
    path: &str,
    access: &str,
    body: Option<&serde_json::Value>,
) -> Reply {
    let bearer = format!("Bearer {access}");
    call(
        client,
        method,
        &format!("{origin}{path}"),
        &[("authorization", &bearer)],
        body.map_or(Body::None, Body::Json),
    )
    .await
}

// ------------------------------------------------------------------------------------------
// Part 1: the app's account client straight against the relay
// ------------------------------------------------------------------------------------------

/// Calls every endpoint the app uses under `base` and returns one line per endpoint that does not
/// answer as the app expects (empty when all do).
async fn probe(base: &str, access: &str, team: &str) -> Vec<String> {
    let api = AccountApi::new(BaseUrl::parse(base).unwrap()).unwrap();
    let mut bad = Vec::new();
    match api.device_authorization("interop", "windows").await {
        Ok(_) => {}
        Err(error) => bad.push(format!("POST {base}/oauth/device -> {error:?}")),
    }
    // An unknown device code: the app expects the OAuth error `invalid_grant`.
    match api.poll_token("unknown-device-code").await {
        hangar_lib::account::PollResponse::OAuth(code, _) => {
            let code = format!("{code:?}");
            if code != "InvalidGrant" {
                bad.push(format!("POST {base}/oauth/token -> OAuth {code}"));
            }
        }
        hangar_lib::account::PollResponse::Tokens(_) => {}
        hangar_lib::account::PollResponse::Transient => {
            bad.push(format!("POST {base}/oauth/token -> transient error"))
        }
        hangar_lib::account::PollResponse::Malformed => bad.push(format!(
            "POST {base}/oauth/token -> malformed/unexpected answer"
        )),
    }
    match api.me(access).await {
        Ok(_) => {}
        Err(error) => bad.push(format!("GET {base}/me -> {error:?}")),
    }
    match api.projects(access).await {
        Ok(projects) if projects.iter().any(|p| p.id == team) => {}
        Ok(projects) => bad.push(format!("GET {base}/projects -> team missing: {projects:?}")),
        Err(error) => bad.push(format!("GET {base}/projects -> {error:?}")),
    }
    match api.relay_ticket(access, team).await {
        Ok(_) => {}
        Err(error) => bad.push(format!("POST {base}/relay-ticket -> {error:?}")),
    }
    bad
}

// ------------------------------------------------------------------------------------------
// Part 2: adapter from the app's website API to the relay's self-host API
// ------------------------------------------------------------------------------------------

struct Adapter {
    origin: String,
}

impl Adapter {
    fn start(relay: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let relay = relay.to_owned();
        let own = origin.clone();
        std::thread::spawn(move || {
            let client = client();
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let (client, relay, own) = (client.clone(), relay.clone(), own.clone());
                std::thread::spawn(move || translate(stream, &client, &relay, &own));
            }
        });
        Self { origin }
    }

    fn base(&self) -> BaseUrl {
        BaseUrl::parse(&format!("{}/api/v1", self.origin)).unwrap()
    }
}

struct Request {
    method: String,
    path: String,
    bearer: Option<String>,
    body: Vec<u8>,
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
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned);
    Some(Request {
        method,
        path,
        bearer,
        body: data[head_end..head_end + length].to_vec(),
    })
}

fn respond(stream: &mut TcpStream, status: u16, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Write);
}

/// One call of the app, translated: what the relay's self-host API needs differently is written
/// out here, and each difference is one mismatch of the report.
fn translate(mut stream: TcpStream, client: &reqwest::Client, relay: &str, own: &str) {
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let app: serde_json::Value =
        serde_json::from_slice(&request.body).unwrap_or(serde_json::Value::Null);
    let text = |key: &str| app[key].as_str().unwrap_or_default().to_owned();
    let access = request.bearer.clone().unwrap_or_default();
    let (status, body) = tauri::async_runtime::block_on(async {
        match (request.method.as_str(), request.path.as_str()) {
            // App: JSON, client_id `hangar-desktop`, scope, platform. Relay: form at
            // `/oauth/device_authorization`, client_id `hangar`.
            ("POST", "/api/v1/oauth/device") => {
                let reply = call(
                    client,
                    reqwest::Method::POST,
                    &format!("{relay}/oauth/device_authorization"),
                    &[],
                    Body::Form(form(&[
                        ("client_id", "hangar"),
                        ("device_name", &text("device_name")),
                    ])),
                )
                .await;
                // The app opens the page only on its server's origin.
                let body = reply.text().replace(relay, own).into_bytes();
                (reply.status, body)
            }
            // App: JSON with client_id `hangar-desktop`. Relay: form with client_id `hangar`.
            ("POST", "/api/v1/oauth/token") => {
                let grant = text("grant_type");
                let mut pairs = vec![
                    ("grant_type", grant.clone()),
                    ("client_id", "hangar".into()),
                ];
                if grant == "refresh_token" {
                    pairs.push(("refresh_token", text("refresh_token")));
                } else {
                    pairs.push(("device_code", text("device_code")));
                }
                let pairs: Vec<(&str, &str)> =
                    pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
                let reply = call(
                    client,
                    reqwest::Method::POST,
                    &format!("{relay}/oauth/token"),
                    &[],
                    Body::Form(form(&pairs)),
                )
                .await;
                (reply.status, reply.body)
            }
            // App: `/me`. Relay: `/v1/me`, same `id` and `email`.
            ("GET", "/api/v1/me") => {
                let reply =
                    relay_api(client, relay, reqwest::Method::GET, "/v1/me", &access, None).await;
                (reply.status, reply.body)
            }
            // App: `/projects` -> `{"projects":[{id,name,role}]}`. Relay: the `teams` of `/v1/me`.
            ("GET", "/api/v1/projects") => {
                let reply =
                    relay_api(client, relay, reqwest::Method::GET, "/v1/me", &access, None).await;
                if reply.status != 200 {
                    return (reply.status, reply.body);
                }
                let projects = reply.json()["teams"].clone();
                (
                    200,
                    serde_json::json!({ "projects": projects })
                        .to_string()
                        .into_bytes(),
                )
            }
            // App: `/relay-ticket` `{project_id}` -> `{ticket, expires_at, relay_url}`. Relay:
            // `/v1/tickets` `{team}` -> `{ticket, expiresIn, live}`.
            ("POST", "/api/v1/relay-ticket") => {
                let body = serde_json::json!({ "team": text("project_id") });
                let reply = relay_api(
                    client,
                    relay,
                    reqwest::Method::POST,
                    "/v1/tickets",
                    &access,
                    Some(&body),
                )
                .await;
                if reply.status != 200 {
                    return (reply.status, reply.body);
                }
                let issued = reply.json();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let ticket = serde_json::json!({
                    "ticket": issued["ticket"],
                    "expires_at": now + issued["expiresIn"].as_u64().unwrap_or(60),
                    "relay_url": issued["live"],
                });
                (200, ticket.to_string().into_bytes())
            }
            _ => (404, br#"{"error":"not_found"}"#.to_vec()),
        }
    });
    respond(&mut stream, status, &body);
}

// ------------------------------------------------------------------------------------------
// One Hangar on the adapter
// ------------------------------------------------------------------------------------------

struct App {
    account: AccountService,
    team: TeamService,
    _dir: TempDir,
}

fn timing() -> Timing {
    Timing {
        // A few real `ticket.renew` during the run (the relay allows 600 tickets an hour).
        renew_every: Duration::from_secs(3),
        max_age: Duration::from_secs(540),
        ping_every: Duration::from_secs(5),
        silence: Duration::from_secs(30),
        devices_every: Duration::from_millis(500),
        connect_timeout: Duration::from_secs(5),
        backoff_unit: Duration::from_millis(200),
        stable_after: Duration::from_secs(30),
        lock_retry: Duration::from_millis(50),
    }
}

impl App {
    fn new(adapter: &Adapter, name: &str) -> Self {
        let dir = TempDir::new(name);
        let account = AccountService::new(
            adapter.base(),
            Arc::new(MemoryStore::default()),
            Arc::new(|_: &reqwest::Url| Ok(())),
            Arc::new(|_: &AccountView| {}),
            AccountOptions {
                device_name: format!("interop {name}"),
                ..AccountOptions::default()
            },
        );
        let team = TeamService::new(
            account.clone(),
            Arc::new(MemoryStore::default()),
            Arc::new(|_: &TeamSnapshot| {}),
            Options {
                config_dir: Some(dir.0.join("config")),
                state_dir: Some(dir.0.join("local")),
                timing: timing(),
                ..Options::default()
            },
        );
        Self {
            account,
            team,
            _dir: dir,
        }
    }

    /// The app's own device flow; the person approves on the relay's page.
    async fn sign_in(&self, client: &reqwest::Client, browser: &Browser) -> String {
        let view = self.account.sign_in().await.unwrap();
        let AccountState::WaitingForUser { user_code, .. } = view.state else {
            panic!("device flow did not start: {:?}", view.state);
        };
        browser.approve(client, &user_code).await;
        let deadline = Instant::now() + WAIT;
        loop {
            if let AccountState::SignedIn { user: Some(me) } = self.account.view().state {
                return me.id;
            }
            assert!(
                Instant::now() < deadline,
                "not signed in: {:?}",
                self.account.view().state
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    async fn until(&self, project: &str, what: &str, test: impl Fn(&TeamStatus) -> bool) {
        let deadline = Instant::now() + WAIT;
        loop {
            let status = self.team.status(project);
            if test(&status) {
                eprintln!("interop: {what}: {status:?}");
                return;
            }
            assert!(Instant::now() < deadline, "{what}: still {status:?}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn connected(&self, project: &str, epoch: u32, members: usize, devices: usize) {
        let want = TeamStatus::Connected {
            epoch,
            members,
            devices,
        };
        self.until(project, &format!("{want:?}"), |s| *s == want)
            .await;
    }
}

async fn until(what: &str, test: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !test() {
        assert!(Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    eprintln!("interop: {what}");
}

fn local(id: u64, agent: &str, root: &str, files: &[(&str, u32, u32)]) -> LocalSession {
    LocalSession {
        id,
        agent: agent.to_owned(),
        title: format!("{agent} at work"),
        state: SessionState::Working,
        repo: root.to_owned(),
        folder: FOLDER.to_owned(),
        repo_name: "shop".to_owned(),
        base: Some("b".repeat(40)),
        claims: files
            .iter()
            .map(|&(path, start, end)| Claim {
                path: path.to_owned(),
                kind: ChangeKind::Modified,
                whole_file: false,
                ranges: vec![LineRange { start, end }],
                area: path.split_once('/').map(|(area, _)| area.to_owned()),
            })
            .collect(),
        truncated: false,
    }
}

// ------------------------------------------------------------------------------------------
// The test
// ------------------------------------------------------------------------------------------

#[test]
#[ignore = "needs a running self-hosted relay, see the module documentation"]
fn hangar_against_a_self_hosted_relay() {
    let Some(origin) = env("HANGAR_INTEROP_BASE") else {
        eprintln!("interop: HANGAR_INTEROP_BASE not set, skipped");
        return;
    };
    let origin = origin.trim_end_matches('/').to_owned();
    let link_a = env("HANGAR_INTEROP_LINK_A").expect("HANGAR_INTEROP_LINK_A");
    let link_b = env("HANGAR_INTEROP_LINK_B").expect("HANGAR_INTEROP_LINK_B");
    tauri::async_runtime::block_on(async move {
        let client = client();
        let alice = Browser::sign_in(&client, &origin, &link_a).await;
        let bob = Browser::sign_in(&client, &origin, &link_b).await;
        eprintln!("interop: both login links redeemed");
        let alice_token = alice.access_token(&client).await;
        let bob_token = bob.access_token(&client).await;
        eprintln!("interop: relay device flow gave both accounts a token");

        // The team, the invitation and its acceptance: the app has no calls for these yet.
        let created = relay_api(
            &client,
            &origin,
            reqwest::Method::POST,
            "/v1/teams",
            &alice_token,
            Some(&serde_json::json!({ "name": "Interop" })),
        )
        .await;
        assert_eq!(created.status, 201, "team: {}", created.text());
        let team = created.json()["id"].as_str().unwrap().to_owned();
        let bob_me = relay_api(
            &client,
            &origin,
            reqwest::Method::GET,
            "/v1/me",
            &bob_token,
            None,
        )
        .await
        .json();
        let invited = relay_api(
            &client,
            &origin,
            reqwest::Method::POST,
            &format!("/v1/teams/{team}/invitations"),
            &alice_token,
            Some(&serde_json::json!({ "emails": [bob_me["email"]], "role": "member" })),
        )
        .await;
        assert_eq!(invited.status, 200, "invite: {}", invited.text());
        let link = invited.json()["invitations"][0]["link"]
            .as_str()
            .expect("the invitation link comes back without mail")
            .to_owned();
        let token = link.rsplit('/').next().unwrap().to_owned();
        let accepted = relay_api(
            &client,
            &origin,
            reqwest::Method::POST,
            "/v1/invitations/accept",
            &bob_token,
            Some(&serde_json::json!({ "token": token })),
        )
        .await;
        assert_eq!(accepted.status, 200, "accept: {}", accepted.text());
        eprintln!("interop: team created, member invited and joined");

        // Part 1: the app's account client as it is, against the relay.
        let mut mismatches = Vec::new();
        for base in [
            origin.clone(),
            format!("{origin}/v1"),
            format!("{origin}/api/v1"),
        ] {
            let bad = probe(&base, &alice_token, &team).await;
            if bad.is_empty() {
                mismatches.clear();
                break;
            }
            mismatches.extend(bad);
        }
        for line in &mismatches {
            eprintln!("interop: account API mismatch: {line}");
        }

        // Part 2: the real account and team services through the adapter.
        let adapter = Adapter::start(&origin);
        let a1 = App::new(&adapter, "a1");
        let b1 = App::new(&adapter, "b1");
        let alice_id = a1.sign_in(&client, &alice).await;
        let bob_id = b1.sign_in(&client, &bob).await;
        eprintln!("interop: app device flow signed in {alice_id} and {bob_id}");

        a1.team.enable(&team).await.unwrap();
        a1.connected(&team, 1, 1, 1).await;
        b1.team.enable(&team).await.unwrap();
        b1.until(&team, "member waits for confirmation", |s| {
            matches!(s, TeamStatus::WaitingForConfirmation { .. })
        })
        .await;
        let deadline = Instant::now() + WAIT;
        let pending = loop {
            let pending = a1.team.pending();
            if !pending.is_empty() {
                break pending;
            }
            assert!(Instant::now() < deadline, "no pending confirmation");
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        assert_eq!(pending[0].member, bob_id);
        a1.team.confirm(&pending[0].id).await.unwrap();
        b1.connected(&team, 1, 2, 2).await;
        a1.connected(&team, 1, 2, 2).await;

        // Presence and a claim overlap through the relay's ClaimBoard.
        let root = "a".repeat(40);
        a1.team.set_local(vec![local(
            1,
            "Claude Code",
            &root,
            &[("src/auth.rs", 10, 12)],
        )]);
        b1.team
            .set_local(vec![local(5, "Codex", &root, &[("src/Auth.rs", 14, 15)])]);
        a1.team.link_repo(&team, &root, FOLDER, true).unwrap();
        b1.team.link_repo(&team, &root, FOLDER, true).unwrap();
        until("bob sees alice's session", || {
            b1.team.snapshot().teams.first().is_some_and(|t| {
                t.activity.peers.iter().any(|p| {
                    p.sessions
                        .iter()
                        .any(|s| s.agent == "Claude Code" && s.files == ["src/auth.rs"])
                })
            })
        })
        .await;
        until("alice sees the overlapping lines", || {
            a1.team.snapshot().teams.first().is_some_and(|t| {
                t.activity.overlaps.iter().any(|o| {
                    o.level == OverlapLevel::OverlappingLines
                        && o.lines == [LineRange { start: 10, end: 15 }]
                        && o.agent == "Codex"
                })
            })
        })
        .await;

        // Several ticket renewals later both are still on the same connection state.
        tokio::time::sleep(Duration::from_secs(10)).await;
        a1.connected(&team, 1, 2, 2).await;
        b1.connected(&team, 1, 2, 2).await;

        // The owner removes the member on the server: the relay closes his connection (4403), his
        // app sees the project gone from its list and stops; the owner's app takes him out of
        // the membership log and rotates to epoch 2.
        let removed = relay_api(
            &client,
            &origin,
            reqwest::Method::DELETE,
            &format!("/v1/teams/{team}/members/{bob_id}"),
            &alice_token,
            None,
        )
        .await;
        assert_eq!(removed.status, 204, "remove member: {}", removed.text());
        b1.until(&team, "member removed", |s| *s == TeamStatus::Removed)
            .await;
        a1.team.remove_member(&team, &bob_id).await.unwrap();
        a1.connected(&team, 2, 1, 1).await;
        until("bob gone from alice's view", || {
            a1.team
                .snapshot()
                .teams
                .first()
                .is_some_and(|t| t.activity.peers.is_empty())
        })
        .await;

        // The owner deletes the team: 4404, confirmed by the (adapted) project list.
        let deleted = relay_api(
            &client,
            &origin,
            reqwest::Method::DELETE,
            &format!("/v1/teams/{team}"),
            &alice_token,
            None,
        )
        .await;
        assert_eq!(deleted.status, 204, "delete team: {}", deleted.text());
        a1.until(&team, "project deleted", |s| {
            *s == TeamStatus::ProjectDeleted
        })
        .await;
        a1.team.shutdown();
        b1.team.shutdown();

        assert!(
            mismatches.is_empty(),
            "the app's account client does not work against the relay's self-host API:\n{}",
            mismatches.join("\n")
        );
    });
}

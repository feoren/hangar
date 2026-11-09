//! Interop with a real Hangar team relay in self-host mode (`IDENTITY=local`). Ignored by default;
//! it needs a relay started by hand on the loopback and two one-time sign-in links:
//!
//! ```text
//! HANGAR_INTEROP_BASE=http://127.0.0.1:8080          the relay's PUBLIC_URL (origin)
//! HANGAR_INTEROP_LINK_A=<login link for the owner>   `hangar-team-server login-link a@example.org`
//! HANGAR_INTEROP_LINK_B=<login link for the member>  `hangar-team-server login-link b@example.org`
//! HANGAR_INTEROP_SHORT_TIMERS=1                      optional: the relay runs with
//!     RESERVATION_INACTIVITY_SECS=120 RESERVATION_WARN_SECS=60 CONNECTION_MAX_AGE_SECS=60,
//!     which adds the hold expiry and the connection age close (4401) to the run
//! cargo test --test relay_interop -- --ignored --nocapture
//! ```
//!
//! Without `HANGAR_INTEROP_BASE` the test returns at once. Nothing here knows a server secret.
//!
//! The app uses the account API under `<origin>/api/v1`, exactly as a person would enter it in
//! Settings: the real `AccountService` signs both accounts in with the device flow (the person's
//! approval goes through the relay's `/device` page), and the real `TeamService` runs both
//! against the real relay. Only what the app has no call for (creating the team, inviting,
//! accepting, removing a member, deleting the team) goes through the relay's own `/v1` routes.
//! Steps the protocol may legitimately take differently are checked softly: each miss is
//! collected and the test fails at the end with the full list.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hangar_lib::account::{AccountState, AccountView, BaseUrl, IdleNotice, PollResponse, Role};
use hangar_lib::account_api::{AccountApi, ApiError};
use hangar_lib::account_service::{AccountService, Options as AccountOptions};
use hangar_lib::claims::{ChangeKind, Claim, LineRange, OverlapLevel};
use hangar_lib::secrets::{SecretError, SecretStore};
use hangar_lib::team::{self, TeamSnapshot, TeamStatus, Timing};
use hangar_lib::team_service::{Options, Task, TeamService};
use hangar_lib::team_share::{LocalSession, SessionState, TeamActivity};
use hangar_lib::team_tasks::{AttemptState, NoticeKind};
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

/// Soft checks: every miss is kept, the test fails at the end with all of them.
#[derive(Default)]
struct Findings(Vec<String>);

impl Findings {
    fn check(&mut self, ok: bool, what: impl Into<String>) -> bool {
        let what = what.into();
        if ok {
            eprintln!("interop: ok: {what}");
        } else {
            eprintln!("interop: MISMATCH: {what}");
            self.0.push(what);
        }
        ok
    }
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

/// The relay's own management routes (`/v1/...`), which the app has no calls for.
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
// The person in the browser: login link, device page
// ------------------------------------------------------------------------------------------

/// A browser signed in at the relay through a login link (session cookie `hangar_team`).
struct Browser {
    origin: String,
    cookie: String,
}

/// Tokens of a sign-in made by the test itself, for the management routes.
struct Tokens {
    access: String,
    refresh: String,
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

    /// A device flow of the test itself through the app's account client (`AccountApi`), so the
    /// test holds tokens for the management routes.
    async fn tokens(&self, client: &reqwest::Client, api: &AccountApi) -> Tokens {
        let started = api
            .device_authorization("interop test", "windows")
            .await
            .expect("device flow starts");
        self.approve(client, &started.user_code).await;
        let interval = started.interval.unwrap_or(5);
        let deadline = Instant::now() + WAIT;
        loop {
            tokio::time::sleep(Duration::from_secs(interval + 1)).await;
            if let PollResponse::Tokens(tokens) = api.poll_token(&started.device_code).await {
                return Tokens {
                    access: tokens.access.to_string(),
                    refresh: tokens.refresh.expect("rotating refresh token").to_string(),
                };
            }
            assert!(Instant::now() < deadline, "no token");
        }
    }
}

// ------------------------------------------------------------------------------------------
// One Hangar on the relay's account API
// ------------------------------------------------------------------------------------------

struct App {
    account: AccountService,
    team: TeamService,
    /// Every status the team service published, in order.
    statuses: Arc<Mutex<Vec<TeamStatus>>>,
    /// What survives a restart of the app: the account's and the team's keychain entries and
    /// the config and state folders.
    accounts: Arc<MemoryStore>,
    keychain: Arc<MemoryStore>,
    dir: Arc<TempDir>,
    name: String,
    renew_every: Duration,
}

fn timing(renew_every: Duration) -> Timing {
    Timing {
        renew_every,
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
    fn new(base: &BaseUrl, name: &str, renew_every: Duration) -> Self {
        Self::with(
            base,
            name,
            renew_every,
            Arc::default(),
            Arc::default(),
            Arc::new(TempDir::new(name)),
        )
    }

    /// The same Hangar started again: new services on the stored sign-in, keys and state.
    fn restart(&self, base: &BaseUrl) -> Self {
        Self::with(
            base,
            &self.name,
            self.renew_every,
            Arc::clone(&self.accounts),
            Arc::clone(&self.keychain),
            Arc::clone(&self.dir),
        )
    }

    fn with(
        base: &BaseUrl,
        name: &str,
        renew_every: Duration,
        accounts: Arc<MemoryStore>,
        keychain: Arc<MemoryStore>,
        dir: Arc<TempDir>,
    ) -> Self {
        let account = AccountService::new(
            base.clone(),
            Arc::clone(&accounts) as Arc<dyn SecretStore>,
            Arc::new(|_: &reqwest::Url| Ok(())),
            Arc::new(|_: &AccountView| {}),
            AccountOptions {
                device_name: format!("interop {name}"),
                ..AccountOptions::default()
            },
        );
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&statuses);
        let team = TeamService::new(
            account.clone(),
            Arc::clone(&keychain) as Arc<dyn SecretStore>,
            Arc::new(move |snapshot: &TeamSnapshot| {
                if let Some(entry) = snapshot.teams.first() {
                    let mut seen = guard(&seen);
                    if seen.last() != Some(&entry.status) {
                        seen.push(entry.status.clone());
                    }
                }
            }),
            Options {
                config_dir: Some(dir.0.join("config")),
                state_dir: Some(dir.0.join("local")),
                timing: timing(renew_every),
                ..Options::default()
            },
        );
        Self {
            account,
            team,
            statuses,
            accounts,
            keychain,
            dir,
            name: name.to_owned(),
            renew_every,
        }
    }

    /// Whether this device's identity for `project` is in the keychain.
    fn has_identity(&self, base: &BaseUrl, project: &str) -> bool {
        let account = team::device_account(&team::team_key(base.as_str(), project));
        guard(&self.keychain.0).contains_key(&account)
    }

    /// The app's own device flow; the person approves on the relay's page.
    async fn sign_in(&self, client: &reqwest::Client, browser: &Browser) -> String {
        let view = self.account.sign_in().await.unwrap();
        let AccountState::WaitingForUser {
            user_code,
            verification_uri_complete,
            ..
        } = view.state
        else {
            panic!("device flow did not start: {:?}", view.state);
        };
        eprintln!("interop: app device flow shows {user_code} at {verification_uri_complete}");
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

    fn activity(&self) -> Option<TeamActivity> {
        self.team
            .snapshot()
            .teams
            .first()
            .map(|t| t.activity.clone())
    }

    /// Waits for `test`; `false` after `limit`.
    async fn wait(
        &self,
        project: &str,
        what: &str,
        limit: Duration,
        test: impl Fn(&TeamStatus) -> bool,
    ) -> bool {
        let deadline = Instant::now() + limit;
        loop {
            let status = self.team.status(project);
            if test(&status) {
                eprintln!("interop: {what}: {status:?}");
                return true;
            }
            if Instant::now() >= deadline {
                eprintln!("interop: {what}: still {status:?}");
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn until(&self, project: &str, what: &str, test: impl Fn(&TeamStatus) -> bool) {
        assert!(self.wait(project, what, WAIT, test).await, "{what}");
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

/// Waits for `test`; `false` after `limit`.
async fn wait(what: &str, limit: Duration, test: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + limit;
    while !test() {
        if Instant::now() >= deadline {
            eprintln!("interop: {what}: timed out");
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    eprintln!("interop: {what}");
    true
}

async fn until(what: &str, test: impl Fn() -> bool) {
    assert!(wait(what, WAIT, test).await, "{what}");
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

/// After the owner removed `removed`'s device while it held `title`: the device verified the
/// removing entry, stopped as removed and forgot its identity, the task is free for the others,
/// and the owner and the member's other device stay connected.
#[allow(clippy::too_many_arguments)]
async fn removed_device(
    owner: &App,
    other: &App,
    removed: &App,
    base: &BaseUrl,
    project: &str,
    title: &str,
    findings: &mut Findings,
    what: &str,
) {
    let stopped = removed
        .wait(
            project,
            &format!("{what}: removed device stops"),
            WAIT,
            |s| *s == TeamStatus::Removed,
        )
        .await;
    findings.check(
        stopped,
        format!(
            "{what}: the removed device stops as removed: {:?}",
            removed.team.status(project)
        ),
    );
    findings.check(
        wait(&format!("{what}: identity wiped"), WAIT, || {
            !removed.has_identity(base, project)
        })
        .await,
        format!("{what}: the removed device forgot its identity"),
    );
    findings.check(
        wait(&format!("{what}: task released"), WAIT, || {
            [owner, other].iter().all(|app| {
                app.activity()
                    .is_some_and(|a| a.tasks.iter().all(|t| t.title != title))
            })
        })
        .await,
        format!("{what}: the removed device's task is free for the others"),
    );
    for (app, who) in [(owner, "owner"), (other, "member's other device")] {
        findings.check(
            app.wait(
                project,
                &format!("{what}: {who} after the removal"),
                WAIT,
                |s| matches!(s, TeamStatus::Connected { devices: 2, .. }),
            )
            .await,
            format!("{what}: the {who} stays connected without the removed device"),
        );
    }
}

/// A device that forgot its identity turns sharing on again: it waits as a new device until
/// `confirmer`, another device of the same person, endorses it.
async fn rejoin(device: &App, confirmer: &App, project: &str, findings: &mut Findings, what: &str) {
    device.team.enable(project).await.unwrap();
    let waiting = device
        .wait(
            project,
            &format!("{what}: waits as a new device"),
            WAIT,
            |s| matches!(s, TeamStatus::WaitingForConfirmation { .. }),
        )
        .await;
    let asked = waiting
        && wait(&format!("{what}: confirmation asked"), WAIT, || {
            !confirmer.team.pending().is_empty()
        })
        .await;
    if !findings.check(
        asked,
        format!(
            "{what}: the device comes back as a new one waiting for confirmation: {:?}",
            device.team.status(project)
        ),
    ) {
        return;
    }
    let pending = confirmer.team.pending();
    confirmer.team.confirm(&pending[0].id).await.unwrap();
    findings.check(
        device
            .wait(project, &format!("{what}: connected again"), WAIT, |s| {
                matches!(s, TeamStatus::Connected { devices: 3, .. })
            })
            .await,
        format!("{what}: the new device joins"),
    );
}

fn task(title: &str, session: Option<u64>, area: Option<&str>) -> Task {
    Task {
        title: title.to_owned(),
        session,
        area: area.map(str::to_owned),
    }
}

fn has_notice(activity: Option<TeamActivity>, kind: NoticeKind) -> bool {
    activity.is_some_and(|a| a.notices.iter().any(|n| n.kind == kind))
}

async fn session_count(client: &reqwest::Client, origin: &str, access: &str) -> usize {
    let reply = relay_api(
        client,
        origin,
        reqwest::Method::GET,
        "/v1/sessions",
        access,
        None,
    )
    .await;
    reply.json()["sessions"].as_array().map_or(0, Vec::len)
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
    let short_timers = env("HANGAR_INTEROP_SHORT_TIMERS").is_some();
    tauri::async_runtime::block_on(async move {
        let mut findings = Findings::default();
        let client = client();
        let base = BaseUrl::parse(&format!("{origin}/api/v1")).unwrap();
        let api = AccountApi::new(base.clone()).unwrap();
        let alice = Browser::sign_in(&client, &origin, &link_a).await;
        let bob = Browser::sign_in(&client, &origin, &link_b).await;
        eprintln!("interop: both login links redeemed");

        // ---- Part 1: the account API as the app's client speaks it ----
        let alice_tokens = alice.tokens(&client, &api).await;
        let bob_tokens = bob.tokens(&client, &api).await;
        eprintln!("interop: AccountApi device flow gave both accounts tokens");
        let poll = api.poll_token("unknown-device-code").await;
        findings.check(
            matches!(
                poll,
                PollResponse::OAuth(hangar_lib::account::OAuthCode::InvalidGrant, _)
            ),
            "POST /oauth/token with an unknown device code -> invalid_grant",
        );
        let refreshed = api.refresh("hgrt_unknown").await;
        findings.check(
            matches!(
                refreshed,
                Err(ApiError::OAuth(
                    hangar_lib::account::OAuthCode::InvalidGrant,
                    _
                ))
            ),
            format!("POST /oauth/token with an unknown refresh token -> {refreshed:?}"),
        );
        let me = api.me(&alice_tokens.access).await;
        findings.check(
            me.as_ref().is_ok_and(|me| me.email == "a@example.org"),
            format!("GET /me -> {me:?}"),
        );
        let unauthorized = api.me("hgat_unknown").await;
        findings.check(
            matches!(unauthorized, Err(ApiError::Unauthorized)),
            format!("GET /me with an unknown token -> {unauthorized:?}"),
        );

        // The team, the invitation and its acceptance: the app has no calls for these yet.
        let created = relay_api(
            &client,
            &origin,
            reqwest::Method::POST,
            "/v1/teams",
            &alice_tokens.access,
            Some(&serde_json::json!({ "name": "Interop" })),
        )
        .await;
        assert_eq!(created.status, 201, "team: {}", created.text());
        let team = created.json()["id"].as_str().unwrap().to_owned();
        let projects = api.projects(&alice_tokens.access).await;
        findings.check(
            projects
                .as_ref()
                .is_ok_and(|p| p.iter().any(|p| p.id == team && p.role == Role::Owner)),
            format!("GET /projects lists the team as owner -> {projects:?}"),
        );
        let ticket = api.relay_ticket(&alice_tokens.access, &team).await;
        findings.check(
            ticket.as_ref().is_ok_and(|t| {
                t.relay_url.as_str() == format!("{}/v1/live", origin.replacen("http", "ws", 1))
            }),
            format!("POST /relay-ticket -> {ticket:?}"),
        );
        let stranger = api.relay_ticket(&bob_tokens.access, &team).await;
        findings.check(
            matches!(stranger, Err(ApiError::Status { status: 404, .. })),
            format!("POST /relay-ticket for a non-member -> {stranger:?}"),
        );
        let bob_me = api.me(&bob_tokens.access).await.expect("bob's /me");
        let invited = relay_api(
            &client,
            &origin,
            reqwest::Method::POST,
            &format!("/v1/teams/{team}/invitations"),
            &alice_tokens.access,
            Some(&serde_json::json!({ "emails": [bob_me.email], "role": "member" })),
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
            &bob_tokens.access,
            Some(&serde_json::json!({ "token": token })),
        )
        .await;
        assert_eq!(accepted.status, 200, "accept: {}", accepted.text());
        eprintln!("interop: team created, member invited and joined");

        // ---- Part 2: the real account and team services ----
        // Alice's device renews no ticket when the relay's connection age is short, so the relay
        // ends her connection with 4401 and she reconnects; Bob's renews every 3 s.
        let alice_renew = if short_timers {
            Duration::from_secs(3600)
        } else {
            Duration::from_secs(3)
        };
        let a1 = App::new(&base, "a1", alice_renew);
        let b1 = App::new(&base, "b1", Duration::from_secs(3));
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
        until("alice is asked to confirm bob", || {
            !a1.team.pending().is_empty()
        })
        .await;
        let pending = a1.team.pending();
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
            b1.activity().is_some_and(|a| {
                a.peers.iter().any(|p| {
                    p.sessions
                        .iter()
                        .any(|s| s.agent == "Claude Code" && s.files == ["src/auth.rs"])
                })
            })
        })
        .await;
        until("alice sees the overlapping lines", || {
            a1.activity().is_some_and(|a| {
                a.overlaps.iter().any(|o| {
                    o.level == OverlapLevel::OverlappingLines
                        && o.lines == [LineRange { start: 10, end: 15 }]
                        && o.agent == "Codex"
                })
            })
        })
        .await;

        // ---- Task reservations ----
        // T1: Alice reserves, Bob is second and waits on her, asks, Alice hands it over.
        a1.team
            .reserve_task(&team, task("Fix checkout", Some(1), Some("src/pay")))
            .await
            .unwrap();
        until("bob sees alice's reservation", || {
            b1.activity().is_some_and(|a| {
                a.tasks
                    .iter()
                    .any(|t| t.title == "Fix checkout" && t.member == alice_id)
            })
        })
        .await;
        b1.team
            .reserve_task(&team, task("fix checkout", None, None))
            .await
            .unwrap();
        until("bob's attempt waits on alice by name", || {
            b1.activity().is_some_and(|a| {
                a.attempts.len() == 1
                    && a.attempts[0].state == AttemptState::Blocked
                    && a.attempts[0].member.as_deref() == Some(alice_id.as_str())
            })
        })
        .await;
        let t1 = b1.activity().unwrap().attempts[0].task.clone();
        b1.team.ask_release(&team, &t1).await.unwrap();
        until("alice is asked to hand over", || {
            a1.activity().is_some_and(|a| {
                a.tasks.iter().any(|t| {
                    t.task == t1
                        && t.mine
                        && t.request
                            .as_ref()
                            .is_some_and(|r| r.member == bob_id && r.confirmed)
                })
            })
        })
        .await;
        a1.team
            .answer_release(&team, &t1, &bob_id, true)
            .await
            .unwrap();
        until("bob holds the task", || {
            b1.activity()
                .is_some_and(|a| a.tasks.iter().any(|t| t.task == t1 && t.mine))
        })
        .await;
        findings.check(
            has_notice(b1.activity(), NoticeKind::ReleaseAccepted),
            "bob is told the handover was accepted",
        );
        until("alice reads bob's note", || {
            a1.activity().is_some_and(|a| {
                a.tasks
                    .iter()
                    .any(|t| t.task == t1 && t.member == bob_id && t.title == "fix checkout")
            })
        })
        .await;

        // T2: Alice declines.
        a1.team
            .reserve_task(&team, task("Update docs", None, None))
            .await
            .unwrap();
        until("bob sees the second reservation", || {
            b1.activity()
                .is_some_and(|a| a.tasks.iter().any(|t| t.title == "Update docs"))
        })
        .await;
        b1.team
            .reserve_task(&team, task("Update docs", None, None))
            .await
            .unwrap();
        until("bob's second attempt is blocked", || {
            b1.activity().is_some_and(|a| {
                a.attempts
                    .iter()
                    .any(|t| t.title == "Update docs" && t.state == AttemptState::Blocked)
            })
        })
        .await;
        let t2 = b1
            .activity()
            .unwrap()
            .attempts
            .iter()
            .find(|t| t.title == "Update docs")
            .unwrap()
            .task
            .clone();
        b1.team.ask_release(&team, &t2).await.unwrap();
        until("alice is asked for the second task", || {
            a1.activity()
                .is_some_and(|a| a.tasks.iter().any(|t| t.task == t2 && t.request.is_some()))
        })
        .await;
        a1.team
            .answer_release(&team, &t2, &bob_id, false)
            .await
            .unwrap();
        findings.check(
            wait("bob is told the request was declined", WAIT, || {
                has_notice(b1.activity(), NoticeKind::ReleaseDeclined)
            })
            .await,
            "bob is told the request was declined",
        );
        findings.check(
            a1.activity().is_some_and(|a| {
                a.tasks
                    .iter()
                    .any(|t| t.task == t2 && t.mine && t.request.is_none())
            }),
            "alice keeps the declined task, the request is gone",
        );
        // Bob asks again at once: the relay's cooldown refuses it.
        b1.team.ask_release(&team, &t2).await.unwrap();
        findings.check(
            wait(
                "a second request within the cooldown is refused",
                WAIT,
                || has_notice(b1.activity(), NoticeKind::AskRefused),
            )
            .await,
            "a second request within the cooldown is refused",
        );
        b1.team.release_task(&team, &t2).await.unwrap();
        a1.team.release_task(&team, &t2).await.unwrap();
        findings.check(
            wait("the second task is gone everywhere", WAIT, || {
                a1.activity()
                    .is_some_and(|a| a.tasks.iter().all(|t| t.task != t2))
                    && b1.activity().is_some_and(|a| {
                        a.tasks.iter().all(|t| t.task != t2)
                            && a.attempts.iter().all(|t| t.task != t2)
                    })
            })
            .await,
            "the second task is gone everywhere after release and cancel",
        );

        // ---- Expiry of an offline holder's task and the connection age (4401) ----
        if short_timers {
            let before = guard(&a1.statuses).len();
            b1.team.disable(&team).await.unwrap();
            findings.check(
                wait("alice sees bob's hold run on a deadline", WAIT, || {
                    a1.activity().is_some_and(|a| {
                        a.tasks
                            .iter()
                            .any(|t| t.task == t1 && t.expires_at.is_some())
                    })
                })
                .await,
                "an offline holder's task gets a deadline",
            );
            findings.check(
                wait(
                    "alice sees the expiry warning",
                    Duration::from_secs(150),
                    || {
                        a1.activity()
                            .is_some_and(|a| a.tasks.iter().any(|t| t.task == t1 && t.expiring))
                    },
                )
                .await,
                "the expiry warning reaches the team",
            );
            findings.check(
                wait("bob's task expired", Duration::from_secs(150), || {
                    a1.activity()
                        .is_some_and(|a| a.tasks.iter().all(|t| t.task != t1))
                })
                .await,
                "the offline holder's task expires",
            );
            let seen: Vec<TeamStatus> = guard(&a1.statuses)[before..].to_vec();
            findings.check(
                seen.iter()
                    .any(|s| matches!(s, TeamStatus::Connecting { .. })),
                format!("alice's connection was replaced after the relay's age limit: {seen:?}"),
            );
            a1.connected(&team, 1, 2, 2).await;
            b1.team.enable(&team).await.unwrap();
            b1.connected(&team, 1, 2, 2).await;
            let notices: Vec<NoticeKind> = b1
                .activity()
                .map(|a| a.notices.iter().map(|n| n.kind).collect())
                .unwrap_or_default();
            eprintln!("interop: bob's notices after coming back: {notices:?}");
            findings.check(
                wait(
                    "bob learns that his task ran out",
                    Duration::from_secs(20),
                    || {
                        has_notice(b1.activity(), NoticeKind::Expired)
                            || has_notice(b1.activity(), NoticeKind::Expiring)
                    },
                )
                .await,
                "bob learns about the expiry when he comes back",
            );
        } else {
            eprintln!("interop: short timers off: hold expiry and 4401 not exercised");
            tokio::time::sleep(Duration::from_secs(10)).await;
            a1.connected(&team, 1, 2, 2).await;
            b1.connected(&team, 1, 2, 2).await;
        }

        // ---- A second device of Bob, endorsed by his first ----
        let bob_first = a1
            .activity()
            .and_then(|a| a.peers.into_iter().find(|p| p.member == bob_id))
            .map(|p| p.device)
            .expect("bob's device in alice's view");
        let b2 = App::new(&base, "b2", Duration::from_secs(3));
        assert_eq!(b2.sign_in(&client, &bob).await, bob_id);
        b2.team.enable(&team).await.unwrap();
        b2.until(&team, "bob's second device waits", |s| {
            matches!(s, TeamStatus::WaitingForConfirmation { .. })
        })
        .await;
        until("bob's first device is asked", || {
            !b1.team.pending().is_empty()
        })
        .await;
        let pending = b1.team.pending();
        findings.check(
            pending[0].kind == hangar_lib::team::PendingKind::OwnDevice,
            format!(
                "the second device is the member's own: {:?}",
                pending[0].kind
            ),
        );
        b1.team.confirm(&pending[0].id).await.unwrap();
        b2.connected(&team, 1, 2, 3).await;
        a1.connected(&team, 1, 2, 3).await;

        // ---- device.remove (a): Bob's second device is online and holds a task ----
        let bob_second = a1
            .activity()
            .and_then(|a| {
                a.peers
                    .into_iter()
                    .find(|p| p.member == bob_id && p.device != bob_first)
            })
            .map(|p| p.device)
            .expect("bob's second device in alice's view");
        b2.team
            .reserve_task(&team, task("Refactor cart", None, None))
            .await
            .unwrap();
        until("alice sees the task of bob's second device", || {
            a1.activity().is_some_and(|a| {
                a.tasks
                    .iter()
                    .any(|t| t.title == "Refactor cart" && t.device == bob_second)
            })
        })
        .await;
        assert!(b2.has_identity(&base, &team));
        a1.team
            .remove_device(&team, &bob_id, &bob_second)
            .await
            .unwrap();
        removed_device(
            &a1,
            &b1,
            &b2,
            &base,
            &team,
            "Refactor cart",
            &mut findings,
            "live",
        )
        .await;
        rejoin(&b2, &b1, &team, &mut findings, "live").await;
        // The new device shows in Alice's view once its presence arrives.
        let new_device = || {
            a1.activity()
                .and_then(|a| {
                    a.peers.into_iter().find(|p| {
                        p.member == bob_id && p.device != bob_first && p.device != bob_second
                    })
                })
                .map(|p| p.device)
        };
        wait("alice sees bob's new device", WAIT, || {
            new_device().is_some()
        })
        .await;
        let bob_third = new_device().unwrap_or_default();
        findings.check(
            !bob_third.is_empty() && bob_third != bob_second,
            format!("live: the device came back under a new id ({bob_second} -> {bob_third})"),
        );

        // ---- device.remove (b): removed while the app is closed, seen at the next start ----
        b2.team
            .reserve_task(&team, task("Tidy logs", None, None))
            .await
            .unwrap();
        until("alice sees the second task of bob's device", || {
            a1.activity().is_some_and(|a| {
                a.tasks
                    .iter()
                    .any(|t| t.title == "Tidy logs" && t.device == bob_third)
            })
        })
        .await;
        b2.team.shutdown();
        until("bob's device is offline for alice", || {
            a1.activity()
                .is_some_and(|a| a.peers.iter().all(|p| p.device != bob_third))
        })
        .await;
        a1.team
            .remove_device(&team, &bob_id, &bob_third)
            .await
            .unwrap();
        findings.check(
            b2.has_identity(&base, &team),
            "restart: the closed app still holds its identity before it starts",
        );
        let b2 = b2.restart(&base);
        b2.account.status().await;
        b2.team.start_enabled();
        removed_device(
            &a1,
            &b1,
            &b2,
            &base,
            &team,
            "Tidy logs",
            &mut findings,
            "restart",
        )
        .await;
        rejoin(&b2, &b1, &team, &mut findings, "restart").await;
        b2.team.shutdown();

        // ---- Member removal (4403) and team deletion (4404) ----
        let removed = relay_api(
            &client,
            &origin,
            reqwest::Method::DELETE,
            &format!("/v1/teams/{team}/members/{bob_id}"),
            &alice_tokens.access,
            None,
        )
        .await;
        assert_eq!(removed.status, 204, "remove member: {}", removed.text());
        findings.check(
            b1.wait(&team, "member removed", WAIT, |s| *s == TeamStatus::Removed)
                .await,
            "a member removed on the server stops as removed",
        );
        a1.team.remove_member(&team, &bob_id).await.unwrap();
        a1.until(&team, "alice alone", |s| {
            matches!(
                s,
                TeamStatus::Connected {
                    members: 1,
                    devices: 1,
                    ..
                }
            )
        })
        .await;
        let deleted = relay_api(
            &client,
            &origin,
            reqwest::Method::DELETE,
            &format!("/v1/teams/{team}"),
            &alice_tokens.access,
            None,
        )
        .await;
        assert_eq!(deleted.status, 204, "delete team: {}", deleted.text());
        findings.check(
            a1.wait(&team, "project deleted", WAIT, |s| {
                *s == TeamStatus::ProjectDeleted
            })
            .await,
            "a deleted team stops as deleted",
        );
        a1.team.shutdown();
        b1.team.shutdown();

        // ---- Sign-out revokes the device grant on the server ----
        let sessions_before = session_count(&client, &origin, &alice_tokens.access).await;
        let view = a1.account.sign_out().await.unwrap();
        let sessions_after = session_count(&client, &origin, &alice_tokens.access).await;
        findings.check(
            matches!(
                view.state,
                AccountState::Idle {
                    notice: Some(IdleNotice::SignedOut)
                }
            ) && sessions_after + 1 == sessions_before,
            format!(
                "sign-out revokes: {:?}, sessions {sessions_before} -> {sessions_after}",
                view.state
            ),
        );
        let view = b1.account.sign_out().await.unwrap();
        findings.check(
            matches!(
                view.state,
                AccountState::Idle {
                    notice: Some(IdleNotice::SignedOut)
                }
            ),
            format!("bob's sign-out revokes: {:?}", view.state),
        );
        // The test's own grant: revoke, after which the refresh token is spent.
        let revoked = api.revoke(&alice_tokens.refresh).await;
        let reused = api.refresh(&alice_tokens.refresh).await;
        findings.check(
            revoked.is_ok()
                && matches!(
                    reused,
                    Err(ApiError::OAuth(
                        hangar_lib::account::OAuthCode::InvalidGrant,
                        _
                    ))
                ),
            format!("revoke then refresh -> {revoked:?}, {reused:?}"),
        );
        let _ = api.revoke(&bob_tokens.refresh).await;

        assert!(
            findings.0.is_empty(),
            "mismatches between the app and the relay:\n{}",
            findings.0.join("\n")
        );
    });
}

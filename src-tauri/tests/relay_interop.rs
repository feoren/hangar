//! Interop with a real Hangar team relay in self-host mode (`IDENTITY=local`). Ignored by default;
//! it needs a relay started by hand on the loopback and two one-time sign-in links:
//!
//! ```text
//! HANGAR_INTEROP_BASE=http://127.0.0.1:8080          the relay's PUBLIC_URL (origin)
//! HANGAR_INTEROP_LINK_A=<login link for the owner>   `hangar-server login-link a@example.org`
//! HANGAR_INTEROP_LINK_B=<login link for the member>  `hangar-server login-link b@example.org`
//! HANGAR_INTEROP_LINK_C=<login link, second member>   only `team_notes_against_a_self_hosted_relay`
//! HANGAR_INTEROP_OLD_BASE / HANGAR_INTEROP_OLD_LINK  optional: a relay of protocol version 1
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
use hangar_lib::notes::NoteFolder;
use hangar_lib::project_notes::{LockReason, ProjectNotes, SharedStatus};
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
        website_every: Duration::from_millis(200),
        members_every: Duration::from_secs(600),
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
        let AccountState::WaitingForUser { user_code, .. } = view.state else {
            panic!("device flow did not start");
        };
        // The code and the link stay out of the output; they sign the device in.
        eprintln!("interop: app device flow started");
        browser.approve(client, &user_code).await;
        let deadline = Instant::now() + WAIT;
        loop {
            if let AccountState::SignedIn { user: Some(me) } = self.account.view().state {
                return me.id;
            }
            assert!(Instant::now() < deadline, "not signed in in time");
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
            // The person compares the admitting device's safety numbers and confirms.
            if let TeamStatus::ConfirmAdmission {
                log_index,
                safety_number,
                ..
            } = &status
            {
                eprintln!("interop: {what}: confirming the admission");
                let _ = self
                    .team
                    .confirm_admission(project, *log_index, safety_number.clone())
                    .await;
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
        assert_ne!(alice_id, bob_id);
        eprintln!("interop: app device flow signed in both accounts");

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
        // Bob removes his own second device from his first (the app removes own devices only).
        b1.team.remove_device(&team, &bob_second).await.unwrap();
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
        b1.team.remove_device(&team, &bob_third).await.unwrap();
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
        // The owner's device removes Bob from the log on its own, from the website's member
        // list (needs `GET /api/v1/projects/{id}/members` on the server).
        findings.check(
            a1.wait(&team, "alice alone", WAIT, |s| {
                matches!(
                    s,
                    TeamStatus::Connected {
                        members: 1,
                        devices: 1,
                        ..
                    }
                )
            })
            .await,
            "a member removed on the website leaves the log through the owner's device",
        );
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

// ------------------------------------------------------------------------------------------
// Live agents: a real Claude Code and a real Codex in one team project
// ------------------------------------------------------------------------------------------

/// The real Hangar executable, which Codex's hook command starts as `hangar gate-hook`.
const HELPER: &str = env!("CARGO_BIN_EXE_hangar");

/// One device's agent gate, wired to its team service as `lib.rs` wires the app's.
fn live_gate(
    app: &App,
    settings: &std::path::Path,
    work: &std::path::Path,
    session: u64,
    root: &str,
    claimed: &str,
) -> hangar_lib::gate_service::GateService {
    let worktrees = Arc::new(vec![hangar_lib::claims::WorktreeClaims {
        repo: FOLDER.to_owned(),
        root_key: hangar_lib::git::path_key(work),
        name: "shop".to_owned(),
        root_commit: Some(root.to_owned()),
        base: Some("b".repeat(40)),
        sessions: vec![session],
        claims: vec![Claim {
            path: claimed.to_owned(),
            kind: ChangeKind::Modified,
            whole_file: false,
            ranges: vec![LineRange { start: 1, end: 2 }],
            area: claimed.split_once('/').map(|(area, _)| area.to_owned()),
        }],
        truncated: false,
    }]);
    let service = hangar_lib::gate_service::GateService::new(
        settings.to_path_buf(),
        move || Arc::clone(&worktrees),
        move || [(session, "live".to_owned())].into_iter().collect(),
        |_| {},
    );
    service.configure(true);
    service.set_helper(PathBuf::from(HELPER));
    let team = app.team.clone();
    service.set_teammates(move |repo, folder| team.teammate_files(repo, folder));
    let team = app.team.clone();
    service.set_team_tasks(move |repo, folder| team.team_tasks(repo, folder));
    service
}

fn live_agent(id: &str) -> hangar_lib::agent_catalog::Agent {
    let entry = hangar_lib::agent_catalog::builtin()
        .unwrap()
        .into_iter()
        .find(|a| a.id == id)
        .unwrap();
    hangar_lib::agent_catalog::Agent::from_catalog(entry)
}

/// `PATH` without the build folders `cargo test` puts in front of it: with them, cmd.exe in
/// npm's `codex.cmd` does not find `node` (seen with cargo on Windows 11; the app's own
/// environment has no such entries).
fn agent_path() -> std::ffi::OsString {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    std::env::join_paths(std::env::split_paths(&path).filter(|p| !p.starts_with(&target)))
        .unwrap_or(path)
}

/// `<program> --version`, first word that starts with a digit.
fn live_version(program: &str) -> Option<String> {
    let output = std::process::Command::new(program)
        .env("PATH", agent_path())
        .arg("--version")
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .find(|word| word.starts_with(|c: char| c.is_ascii_digit()))
        .map(str::to_owned)
}

/// What an agent run gave: the edits the gate held (each answered with a deny), exit code,
/// stdout and stderr.
struct LiveRun {
    held: Vec<hangar_lib::gate::PendingDecision>,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Starts the agent as a team session would start it (the gate's arguments and environment),
/// denies every edit the gate holds, and waits at most 5 minutes.
async fn live_run(
    gate: &hangar_lib::gate_service::GateService,
    program: &str,
    launch: &hangar_lib::manager::GateLaunch,
    args: &[&str],
    work: &std::path::Path,
) -> LiveRun {
    let mut command = std::process::Command::new(program);
    command
        .args(&launch.prepend)
        .args(&launch.args)
        .args(args)
        .env("PATH", agent_path())
        .current_dir(work)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (name, value) in &launch.env {
        command.env(name, value.as_str());
    }
    let mut child = command.spawn().expect("agent starts");
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let stdout = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut out, &mut text);
        text
    });
    let stderr = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut err, &mut text);
        text
    });
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut held = Vec::new();
    let status = loop {
        for pending in gate.snapshot().pending {
            eprintln!(
                "live: held {} {} (teammates {:?}, reserved {:?}); denied",
                pending.tool, pending.path, pending.teammates, pending.reserved
            );
            gate.answer(pending.id, hangar_lib::gate::Answer::Deny);
            held.push(pending);
        }
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            #[cfg(windows)]
            let _ = std::process::Command::new("taskkill")
                .args(["/T", "/F", "/PID", &child.id().to_string()])
                .status();
            let _ = child.kill();
            break child.wait().unwrap();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    LiveRun {
        held,
        code: status.code(),
        stdout: stdout.join().unwrap(),
        stderr: stderr.join().unwrap(),
    }
}

/// A real Claude Code (`claude -p`, small model) on Alice's device and a real Codex
/// (`codex exec`, small model) on Bob's, both started with their team session's gate, against
/// a real self-hosted relay. Needs, besides the variables of the module documentation:
///
/// ```text
/// HANGAR_LIVE_AGENTS=1
/// HANGAR_LIVE_CLAUDE=<path of claude.exe>
/// HANGAR_LIVE_CODEX=<path of codex.cmd or codex>
/// HANGAR_LIVE_CODEX_MODEL=<a small model, optional>
/// cargo test --test relay_interop live_agents -- --ignored --nocapture
/// ```
///
/// Each agent is asked to change the file the other person's session changes; the gate holds
/// the edit and the test denies it. Claude Code is asked to name the teammates' files and
/// reservations from its context. Costs a few model calls.
#[test]
#[ignore = "needs a running self-hosted relay and signed-in agent CLIs, see the documentation"]
fn live_agents_claude_and_codex_hold_each_others_files() {
    let (Some(origin), Some(_)) = (env("HANGAR_INTEROP_BASE"), env("HANGAR_LIVE_AGENTS")) else {
        eprintln!("live: HANGAR_INTEROP_BASE or HANGAR_LIVE_AGENTS not set, skipped");
        return;
    };
    let origin = origin.trim_end_matches('/').to_owned();
    let link_a = env("HANGAR_INTEROP_LINK_A").expect("HANGAR_INTEROP_LINK_A");
    let link_b = env("HANGAR_INTEROP_LINK_B").expect("HANGAR_INTEROP_LINK_B");
    let claude_program = env("HANGAR_LIVE_CLAUDE").expect("HANGAR_LIVE_CLAUDE");
    let codex_program = env("HANGAR_LIVE_CODEX").expect("HANGAR_LIVE_CODEX");
    let codex_model = env("HANGAR_LIVE_CODEX_MODEL");
    tauri::async_runtime::block_on(async move {
        let mut findings = Findings::default();
        let client = client();
        let base = BaseUrl::parse(&format!("{origin}/api/v1")).unwrap();
        let api = AccountApi::new(base.clone()).unwrap();
        let alice = Browser::sign_in(&client, &origin, &link_a).await;
        let bob = Browser::sign_in(&client, &origin, &link_b).await;
        let alice_tokens = alice.tokens(&client, &api).await;
        let bob_tokens = bob.tokens(&client, &api).await;
        let created = relay_api(
            &client,
            &origin,
            reqwest::Method::POST,
            "/v1/teams",
            &alice_tokens.access,
            Some(&serde_json::json!({ "name": "Live agents" })),
        )
        .await;
        assert_eq!(created.status, 201, "team: {}", created.text());
        let team = created.json()["id"].as_str().unwrap().to_owned();
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
            .unwrap()
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

        let a1 = App::new(&base, "live-a1", Duration::from_secs(3));
        let b1 = App::new(&base, "live-b1", Duration::from_secs(3));
        let alice_id = a1.sign_in(&client, &alice).await;
        let bob_id = b1.sign_in(&client, &bob).await;
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
        a1.team.confirm(&pending[0].id).await.unwrap();
        b1.connected(&team, 1, 2, 2).await;

        // Two clones of one repository: Alice's Claude Code changes src/auth.rs, Bob's Codex
        // docs/notes.md; Bob holds a reservation for docs/guide.
        let root = "a".repeat(40);
        let work_a = TempDir::new("live-work-a");
        let work_b = TempDir::new("live-work-b");
        for work in [&work_a, &work_b] {
            // A repository of its own: the agents take the git top folder as their project,
            // which would otherwise be this repository's.
            let init = std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&work.0)
                .status()
                .unwrap();
            assert!(init.success());
            std::fs::create_dir_all(work.0.join("src")).unwrap();
            std::fs::create_dir_all(work.0.join("docs")).unwrap();
            std::fs::write(work.0.join("src").join("auth.rs"), "fn auth() {}\n").unwrap();
            std::fs::write(work.0.join("docs").join("notes.md"), "# Notes\n").unwrap();
        }
        a1.team.set_local(vec![local(
            1,
            "Claude Code",
            &root,
            &[("src/auth.rs", 1, 1)],
        )]);
        b1.team
            .set_local(vec![local(5, "Codex", &root, &[("docs/notes.md", 1, 1)])]);
        a1.team.link_repo(&team, &root, FOLDER, true).unwrap();
        b1.team.link_repo(&team, &root, FOLDER, true).unwrap();
        b1.team
            .reserve_task(&team, task("Write the guide", Some(5), Some("docs/guide")))
            .await
            .unwrap();
        until(
            "both devices know the other's file and bob's reservation",
            || {
                b1.team
                    .teammate_files(&root, FOLDER)
                    .iter()
                    .any(|f| f.key == "src/auth.rs")
                    && a1
                        .team
                        .teammate_files(&root, FOLDER)
                        .iter()
                        .any(|f| f.key == "docs/notes.md")
                    && a1.team.team_tasks(&root, FOLDER).len() == 1
            },
        )
        .await;
        let settings_a = TempDir::new("live-gate-a");
        let settings_b = TempDir::new("live-gate-b");
        let gate_a = live_gate(&a1, &settings_a.0, &work_a.0, 1, &root, "src/auth.rs");
        let gate_b = live_gate(&b1, &settings_b.0, &work_b.0, 5, &root, "docs/notes.md");

        // Codex on Bob's device tries Alice's file.
        use hangar_lib::manager::LaunchGate as _;
        let codex_version = live_version(&codex_program);
        eprintln!("live: codex {codex_version:?}");
        let hangar_lib::manager::GatePlan::Launch(codex_launch) = gate_b.prepare(
            5,
            &live_agent("codex"),
            std::path::Path::new(&codex_program),
            &[],
            codex_version.as_deref(),
        ) else {
            panic!("codex gets no gate");
        };
        let work_b_text = work_b.0.to_string_lossy().into_owned();
        let mut codex_args = vec![
            "exec",
            "--sandbox",
            "workspace-write",
            "-C",
            &work_b_text,
            "--skip-git-repo-check",
            "-c",
            "model_reasoning_effort=low",
        ];
        if let Some(model) = &codex_model {
            codex_args.extend(["-m", model.as_str()]);
        }
        codex_args.push(
            "Append the line `// live` to the end of src/auth.rs with apply_patch. If the patch is \
             blocked, do not try again or any other way; reply with the reason you were given.",
        );
        let codex = live_run(
            &gate_b,
            &codex_program,
            &codex_launch,
            &codex_args,
            &work_b.0,
        )
        .await;
        eprintln!(
            "live: codex exit {:?}\n--- stdout\n{}\n--- stderr (tail)\n{}",
            codex.code,
            codex.stdout,
            codex
                .stderr
                .lines()
                .rev()
                .take(25)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
        );
        // Teammates are named by their short member id when the relay sends no display name.
        let alice_tag = hangar_lib::team_share::person_tag("", &alice_id);
        findings.check(
            codex.held.iter().any(|h| {
                h.path == "src/auth.rs" && h.teammates.iter().any(|t| t.contains(&alice_tag))
            }),
            "codex's apply_patch on alice's file was held by bob's gate",
        );
        findings.check(
            std::fs::read_to_string(work_b.0.join("src").join("auth.rs")).unwrap()
                == "fn auth() {}\n",
            "src/auth.rs unchanged after the deny",
        );

        // Claude Code on Alice's device: names its context, then tries Bob's file.
        let claude_version = live_version(&claude_program);
        eprintln!("live: claude {claude_version:?}");
        let hangar_lib::manager::GatePlan::Launch(claude_launch) = gate_a.prepare(
            1,
            &live_agent("claude"),
            std::path::Path::new(&claude_program),
            &[],
            claude_version.as_deref(),
        ) else {
            panic!("claude gets no gate");
        };
        let claude = live_run(
            &gate_a,
            &claude_program,
            &claude_launch,
            &[
                "-p",
                "--model",
                "haiku",
                "--permission-mode",
                "acceptEdits",
                "--max-turns",
                "6",
                "Answer in English. Step 1: copy verbatim every line of the additional context \
                 Hangar gave you about files other sessions are changing and about reserved \
                 tasks. Step 2: read docs/notes.md and append the line `live` with the Edit \
                 tool. If that edit is refused, do not try again or any other way; quote the \
                 reason you were given.",
            ],
            &work_a.0,
        )
        .await;
        eprintln!(
            "live: claude exit {:?}\n--- stdout\n{}\n--- stderr\n{}",
            claude.code, claude.stdout, claude.stderr
        );
        let bob_tag = hangar_lib::team_share::person_tag("", &bob_id);
        findings.check(
            claude.held.iter().any(|h| {
                h.path == "docs/notes.md" && h.teammates.iter().any(|t| t.contains(&bob_tag))
            }),
            "claude's edit of bob's file was held by alice's gate",
        );
        findings.check(
            std::fs::read_to_string(work_a.0.join("docs").join("notes.md")).unwrap() == "# Notes\n",
            "docs/notes.md unchanged after the deny",
        );
        findings.check(
            claude.stdout.contains("docs/notes.md") && claude.stdout.contains("Write the guide"),
            "claude names bob's file and reservation from its context",
        );

        a1.team.shutdown();
        b1.team.shutdown();
        let _ = api.revoke(&alice_tokens.refresh).await;
        let _ = api.revoke(&bob_tokens.refresh).await;
        assert!(
            findings.0.is_empty(),
            "live agents:\n{}",
            findings.0.join("\n")
        );
    });
}

// ------------------------------------------------------------------------------------------
// Shared team notes (ADR 0006, phase 2)
// ------------------------------------------------------------------------------------------

/// Runs `git` in `dir` with a fixed identity; panics on failure.
fn git_in(dir: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Interop",
            "-c",
            "user.email=interop@example.org",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// One Hangar's shared notes on the team service of `app`.
fn notes_of(app: &App) -> ProjectNotes {
    let notes = ProjectNotes::new(&app.dir.0.join("notes"), Arc::new(MemoryStore::default()));
    notes.set_team(Arc::new(app.team.clone()));
    notes
}

fn shared_folder(repo: &std::path::Path) -> NoteFolder {
    NoteFolder {
        path: Some(repo.to_path_buf()),
        shared: true,
    }
}

/// The notes calls block (git, the team's answer), as in the app's background threads.
async fn blocking<T: Send + 'static>(
    notes: &ProjectNotes,
    work: impl FnOnce(&ProjectNotes) -> T + Send + 'static,
) -> T {
    let notes = notes.clone();
    tauri::async_runtime::spawn_blocking(move || work(&notes))
        .await
        .expect("background task")
}

async fn notes_status(notes: &ProjectNotes, repo: &std::path::Path) -> SharedStatus {
    let folder = shared_folder(repo);
    blocking(notes, move |n| n.status(&folder, "other", "Untitled"))
        .await
        .expect("status")
}

async fn notes_until(
    notes: &ProjectNotes,
    repo: &std::path::Path,
    what: &str,
    test: impl Fn(&SharedStatus) -> bool,
) -> bool {
    let deadline = Instant::now() + WAIT;
    loop {
        let status = notes_status(notes, repo).await;
        if test(&status) {
            eprintln!("interop: {what}: {status:?}");
            return true;
        }
        if Instant::now() >= deadline {
            eprintln!("interop: {what}: still {status:?}");
            return false;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn read(notes: &ProjectNotes, repo: &std::path::Path, id: &str) -> Option<String> {
    let folder = shared_folder(repo);
    let id = id.to_owned();
    blocking(notes, move |n| n.read_note(&folder, &id))
        .await
        .ok()
}

async fn write(notes: &ProjectNotes, repo: &std::path::Path, title: &str, text: &str) -> String {
    let folder = shared_folder(repo);
    let (title, text) = (title.to_owned(), text.to_owned());
    blocking(notes, move |n| {
        let id = n.create_note(&folder, &title)?;
        n.write_note(&folder, &id, &text)?;
        Ok::<_, hangar_lib::project_notes::ProjectNotesError>(id)
    })
    .await
    .expect("note written")
}

/// Invites the account behind `browser` into `team` and accepts.
async fn join_team(
    client: &reqwest::Client,
    origin: &str,
    api: &AccountApi,
    owner: &str,
    browser: &Browser,
    team: &str,
) {
    let tokens = browser.tokens(client, api).await;
    let me = api.me(&tokens.access).await.expect("/me");
    let invited = relay_api(
        client,
        origin,
        reqwest::Method::POST,
        &format!("/v1/teams/{team}/invitations"),
        owner,
        Some(&serde_json::json!({ "emails": [me.email], "role": "member" })),
    )
    .await;
    assert_eq!(invited.status, 200, "invite: {}", invited.text());
    let link = invited.json()["invitations"][0]["link"]
        .as_str()
        .expect("invitation link")
        .to_owned();
    let token = link.rsplit('/').next().unwrap().to_owned();
    let accepted = relay_api(
        client,
        origin,
        reqwest::Method::POST,
        "/v1/invitations/accept",
        &tokens.access,
        Some(&serde_json::json!({ "token": token })),
    )
    .await;
    assert_eq!(accepted.status, 200, "accept: {}", accepted.text());
}

/// Turns sharing on in `app` and has `owner` confirm its device.
async fn admit(app: &App, owner: &App, team: &str, member: &str) {
    app.team.enable(team).await.unwrap();
    app.until(team, "waits for confirmation", |s| {
        matches!(s, TeamStatus::WaitingForConfirmation { .. })
    })
    .await;
    until("the owner is asked to confirm", || {
        owner.team.pending().iter().any(|p| p.member == member)
    })
    .await;
    let pending = owner
        .team
        .pending()
        .into_iter()
        .find(|p| p.member == member)
        .unwrap();
    owner.team.confirm(&pending.id).await.unwrap();
}

/// Shared team notes against a self-hosted relay of protocol version 2: turning them on,
/// a member reading, a removal with a new key, and a member admitted afterwards reading every
/// note while the removed one cannot read what followed. Needs a third login link:
///
/// ```text
/// HANGAR_INTEROP_LINK_C=<login link for a second member>   `hangar-server login-link c@example.org`
/// ```
///
/// With `HANGAR_INTEROP_OLD_BASE` and `HANGAR_INTEROP_OLD_LINK` (a relay of version 1 and a
/// login link there) it also checks that this app and that relay settle on version 1 and that
/// shared team notes are offered nowhere.
#[test]
#[ignore = "needs a running self-hosted relay, see the module documentation"]
fn team_notes_against_a_self_hosted_relay() {
    let Some(origin) = env("HANGAR_INTEROP_BASE") else {
        eprintln!("interop: HANGAR_INTEROP_BASE not set, skipped");
        return;
    };
    let origin = origin.trim_end_matches('/').to_owned();
    let link_a = env("HANGAR_INTEROP_LINK_A").expect("HANGAR_INTEROP_LINK_A");
    let link_b = env("HANGAR_INTEROP_LINK_B").expect("HANGAR_INTEROP_LINK_B");
    let link_c = env("HANGAR_INTEROP_LINK_C").expect("HANGAR_INTEROP_LINK_C");
    tauri::async_runtime::block_on(async move {
        let mut findings = Findings::default();
        let client = client();
        let base = BaseUrl::parse(&format!("{origin}/api/v1")).unwrap();
        let api = AccountApi::new(base.clone()).unwrap();
        let alice = Browser::sign_in(&client, &origin, &link_a).await;
        let bob = Browser::sign_in(&client, &origin, &link_b).await;
        let carol = Browser::sign_in(&client, &origin, &link_c).await;
        let alice_tokens = alice.tokens(&client, &api).await;
        let created = relay_api(
            &client,
            &origin,
            reqwest::Method::POST,
            "/v1/teams",
            &alice_tokens.access,
            Some(&serde_json::json!({ "name": "Notes interop" })),
        )
        .await;
        assert_eq!(created.status, 201, "team: {}", created.text());
        let team = created.json()["id"].as_str().unwrap().to_owned();
        join_team(&client, &origin, &api, &alice_tokens.access, &bob, &team).await;

        let a1 = App::new(&base, "notes-a1", Duration::from_secs(3));
        let b1 = App::new(&base, "notes-b1", Duration::from_secs(3));
        let alice_id = a1.sign_in(&client, &alice).await;
        let bob_id = b1.sign_in(&client, &bob).await;
        a1.team.enable(&team).await.unwrap();
        a1.connected(&team, 1, 1, 1).await;
        admit(&b1, &a1, &team, &bob_id).await;
        b1.connected(&team, 1, 2, 2).await;

        // The repository both work in, linked to the team on Alice's side.
        let work = TempDir::new("notes-repo");
        let repo = work.0.join("shop");
        std::fs::create_dir_all(&repo).unwrap();
        git_in(&repo, &["init", "-q"]);
        std::fs::write(repo.join("readme.txt"), "shop\n").unwrap();
        git_in(&repo, &["add", "readme.txt"]);
        git_in(&repo, &["commit", "-qm", "root"]);
        let root = git_in(&repo, &["rev-list", "--max-parents=0", "HEAD"]);
        a1.team
            .link_folder(&team, &root, &repo.to_string_lossy())
            .unwrap();

        // ---- Turning on: the relay stores the ring, then the files are written ----
        let notes_a = notes_of(&a1);
        until("alice's relay speaks version 2", || {
            a1.team
                .notes_keys(&team)
                .is_some_and(|v| v.speaks == Some(true) && v.exists.is_some())
        })
        .await;
        let folder = shared_folder(&repo);
        let enabled = blocking(&notes_a, move |n| {
            n.enable_team(&folder, "other", "Untitled")
        })
        .await;
        findings.check(
            matches!(
                enabled,
                Ok(SharedStatus::Unlocked {
                    renewing: false,
                    ..
                })
            ),
            format!("an owner turns shared team notes on -> {enabled:?}"),
        );
        let before = write(&notes_a, &repo, "Plan", "before the removal").await;

        // ---- A member reads after linking the folder from the notes ----
        let notes_b = notes_of(&b1);
        let locked = notes_status(&notes_b, &repo).await;
        findings.check(
            locked
                == SharedStatus::Locked {
                    reason: LockReason::Link,
                },
            format!("an unlinked member's clone offers the link -> {locked:?}"),
        );
        let folder = shared_folder(&repo);
        let linked = blocking(&notes_b, move |n| n.link_team(&folder, "other", "Untitled")).await;
        findings.check(linked.is_ok(), format!("link from the notes -> {linked:?}"));
        notes_until(&notes_b, &repo, "bob's notes open", |s| {
            matches!(s, SharedStatus::Unlocked { .. })
        })
        .await;
        findings.check(
            read(&notes_b, &repo, &before).await.as_deref() == Some("before the removal"),
            "a member reads the owner's note",
        );

        // ---- Removal: rotation, a new notes key, the removed member is shut out ----
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
        a1.until(&team, "alice alone in epoch 2", |s| {
            matches!(
                s,
                TeamStatus::Connected {
                    epoch: 2,
                    members: 1,
                    ..
                }
            )
        })
        .await;
        findings.check(
            wait("the ring is renewed under epoch 2", WAIT, || {
                a1.team.notes_keys(&team).is_some_and(|v| {
                    !v.renewing
                        && v.fresh.map(|e| e.0) == Some(2)
                        && v.ring.as_ref().is_some_and(|r| r.len() == 2)
                })
            })
            .await,
            "after a removal the owner's device re-seals the ring with a new key",
        );
        notes_until(&notes_a, &repo, "alice may save again", |s| {
            matches!(
                s,
                SharedStatus::Unlocked {
                    renewing: false,
                    ..
                }
            )
        })
        .await;
        let after = write(&notes_a, &repo, "Later", "after the removal").await;
        findings.check(
            read(&notes_b, &repo, &before).await.as_deref() == Some("before the removal"),
            "the removed member keeps what was written before",
        );
        findings.check(
            read(&notes_b, &repo, &after).await.is_none(),
            "the removed member cannot read what was written after the removal",
        );

        // ---- A member admitted later reads every note ----
        join_team(&client, &origin, &api, &alice_tokens.access, &carol, &team).await;
        let c1 = App::new(&base, "notes-c1", Duration::from_secs(3));
        let carol_id = c1.sign_in(&client, &carol).await;
        admit(&c1, &a1, &team, &carol_id).await;
        c1.connected(&team, 2, 2, 2).await;
        let notes_c = notes_of(&c1);
        let folder = shared_folder(&repo);
        let _ = blocking(&notes_c, move |n| n.link_team(&folder, "other", "Untitled")).await;
        notes_until(&notes_c, &repo, "carol's notes open", |s| {
            matches!(s, SharedStatus::Unlocked { .. })
        })
        .await;
        findings.check(
            read(&notes_c, &repo, &before).await.as_deref() == Some("before the removal")
                && read(&notes_c, &repo, &after).await.as_deref() == Some("after the removal"),
            "a member admitted after the removal reads every note",
        );
        let _ = alice_id;
        a1.team.shutdown();
        b1.team.shutdown();
        c1.team.shutdown();

        // ---- This app against a relay of version 1 ----
        if let (Some(old), Some(old_link)) = (
            env("HANGAR_INTEROP_OLD_BASE"),
            env("HANGAR_INTEROP_OLD_LINK"),
        ) {
            let old = old.trim_end_matches('/').to_owned();
            let old_base = BaseUrl::parse(&format!("{old}/api/v1")).unwrap();
            let old_api = AccountApi::new(old_base.clone()).unwrap();
            let owner = Browser::sign_in(&client, &old, &old_link).await;
            let owner_tokens = owner.tokens(&client, &old_api).await;
            let created = relay_api(
                &client,
                &old,
                reqwest::Method::POST,
                "/v1/teams",
                &owner_tokens.access,
                Some(&serde_json::json!({ "name": "Old relay" })),
            )
            .await;
            assert_eq!(created.status, 201, "team: {}", created.text());
            let old_team = created.json()["id"].as_str().unwrap().to_owned();
            let o1 = App::new(&old_base, "notes-old", Duration::from_secs(3));
            o1.sign_in(&client, &owner).await;
            o1.team.enable(&old_team).await.unwrap();
            o1.connected(&old_team, 1, 1, 1).await;
            findings.check(
                o1.team
                    .notes_keys(&old_team)
                    .is_some_and(|v| v.speaks == Some(false) && v.exists.is_none()),
                "a relay of version 1 settles on version 1 and is never asked for notes keys",
            );
            let refused = o1.team.notes_enable(&old_team, None).await;
            findings.check(
                matches!(refused, Err(team::TeamError::NotesUnsupported)),
                format!("turning team notes on against a relay of version 1 -> {refused:?}"),
            );
            o1.team.shutdown();
        } else {
            eprintln!("interop: HANGAR_INTEROP_OLD_BASE not set, the version 1 part is skipped");
        }

        assert!(
            findings.0.is_empty(),
            "interop mismatches:\n{}",
            findings.0.join("\n")
        );
    });
}

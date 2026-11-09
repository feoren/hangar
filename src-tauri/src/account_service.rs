//! The Hangar account at run time: the device flow, the tokens and the project list
//! (contract: docs/wiki/pages/components/hangar-account-api.md).
//!
//! - The refresh token lives only in the OS keychain (service [`account::KEYCHAIN_SERVICE`],
//!   account = the server address) and, while Hangar runs, in memory as [`Zeroizing`]; the access
//!   token only in memory.
//! - Refreshes are single-flight: the server rotates the refresh token and revokes the whole
//!   device when an old one is used again, and two concurrent refreshes count as such a reuse.
//!   Every refresh, sign-in, sign-out and server change holds `refresh_gate`. Across processes
//!   (the installed app and a second instance on the same keychain entry), every write of the
//!   keychain entry or of `account.json` also holds the lock file [`account::LOCK_FILE`], taken
//!   after the gate. A refresh reads the keychain under it first, so it redeems the newest token
//!   another process stored instead of the one it remembers; `account.json` is reread before it
//!   is written, so another process's list of sign-ins is kept.
//! - A refresh whose answer was lost (network error, timeout, 5xx) is repeated once with the same
//!   token: the server's grace gives a fresh pair for a just-rotated token whose successor is
//!   unused.
//! - A refused refresh (`invalid_grant`, or 401 right after a refresh) signs out locally with
//!   [`IdleNotice::SessionEnded`]. Signing out revokes the device on the server first (RFC 7009,
//!   best effort).
//! - Every state change goes to the window through `emit` ([`AccountView`]); no token is part of
//!   it, and no log line names one.

use std::fs::File;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::Url;
use tokio::sync::Notify;
use zeroize::Zeroizing;

use crate::account::{
    self, AccountError, AccountPage, AccountState, AccountView, BaseUrl, DevicePoll, FailKind,
    FailReason, IdleNotice, OAuthCode, PollStep, Project, TokenSet,
};
use crate::account_api::{AccountApi, ApiError};
use crate::lock;
use crate::secrets::{SecretError, SecretStore};

pub type Emit = Arc<dyn Fn(&AccountView) + Send + Sync>;
/// Opens a page in the default browser (`desktop::open_url`); replaced in tests.
pub type Opener = Arc<dyn Fn(&Url) -> std::io::Result<()> + Send + Sync>;

/// Run-time settings of the service.
#[derive(Debug, Clone)]
pub struct Options {
    /// Length of one second of the device flow's timing (interval, expiry); shorter in tests.
    pub unit: Duration,
    /// Where `account.json` (the chosen server) is written; `None` keeps it in memory only.
    pub config_dir: Option<PathBuf>,
    /// The keychain service the store writes to ([`account::KEYCHAIN_SERVICE`]); `account.json`
    /// keeps the list of stored sign-ins per service.
    pub keychain_service: String,
    pub device_name: String,
    pub platform: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            unit: Duration::from_secs(1),
            config_dir: None,
            keychain_service: account::KEYCHAIN_SERVICE.to_owned(),
            device_name: account::device_name(),
            platform: account::platform().to_owned(),
        }
    }
}

#[derive(Clone)]
pub struct AccountService {
    inner: Arc<Inner>,
}

struct Inner {
    store: Arc<dyn SecretStore>,
    emit: Emit,
    open: Opener,
    options: Options,
    core: Mutex<Core>,
    /// Held across every refresh and every change of the stored tokens.
    refresh_gate: tokio::sync::Mutex<()>,
    /// Wakes a waiting poll when its flow was cancelled.
    cancel: Notify,
}

struct Core {
    base: BaseUrl,
    api: Option<Arc<AccountApi>>,
    state: AccountState,
    tokens: Slot,
    /// Id of the current sign-in flow; raising it cancels a running one.
    flow: u64,
    next_generation: u64,
    /// The confirmation page of the waiting flow, for "Open browser again".
    verification: Option<Url>,
    /// The last project list, for opening an invite page by project id.
    projects: Vec<Project>,
    /// Servers whose sign-in is in the keychain (`account.json`), for the window's mention.
    signed_in: Vec<BaseUrl>,
    /// The last rotated token could not be written: the keychain holds a spent one, so a refresh
    /// uses the token in memory instead of the stored one.
    keychain_behind: bool,
}

enum Slot {
    /// The keychain was not read yet for this server.
    Unloaded,
    Empty,
    Present(Tokens),
}

struct Tokens {
    access: Option<Access>,
    refresh: Zeroizing<String>,
    /// Rises with every new token set; a caller that saw a refused access token refreshes only
    /// when no newer set exists.
    generation: u64,
}

const MAX_ACCESS_LIFETIME: u64 = 86_400;

struct Access {
    token: Zeroizing<String>,
    valid_until: Instant,
}

impl Access {
    /// Treated as expired a little early (30 s, or half the lifetime when that is shorter), so a
    /// request does not reach the server with a token that ran out on the way.
    fn new(token: Zeroizing<String>, expires_in: u64) -> Self {
        // At most a day: the server's number must not overflow the clock.
        let lifetime = expires_in.min(MAX_ACCESS_LIFETIME);
        let margin = (lifetime / 2).min(30);
        Self {
            token,
            valid_until: Instant::now() + Duration::from_secs(lifetime - margin),
        }
    }
}

impl Tokens {
    fn valid_access(&self) -> Option<Zeroizing<String>> {
        self.access
            .as_ref()
            .filter(|access| Instant::now() < access.valid_until)
            .map(|access| access.token.clone())
    }
}

impl Core {
    fn api(&mut self) -> Result<Arc<AccountApi>, AccountError> {
        if let Some(api) = &self.api {
            return Ok(Arc::clone(api));
        }
        let api = Arc::new(AccountApi::new(self.base.clone()).map_err(AccountError::from)?);
        self.api = Some(Arc::clone(&api));
        Ok(api)
    }

    fn generation(&mut self) -> u64 {
        self.next_generation += 1;
        self.next_generation
    }

    fn view(&self) -> AccountView {
        AccountView::new(&self.base, self.state.clone(), &self.signed_in)
    }

    /// Why there is no token: a caller that waited while another refresh was refused learns that
    /// the session ended, not merely that nobody is signed in.
    fn signed_out_error(&self) -> AccountError {
        match self.state {
            AccountState::Idle {
                notice: Some(IdleNotice::SessionEnded),
            } => AccountError::SessionEnded,
            _ => AccountError::NotSignedIn,
        }
    }

    fn clear(&mut self, notice: IdleNotice) {
        self.tokens = Slot::Empty;
        self.state = AccountState::Idle {
            notice: Some(notice),
        };
        self.projects.clear();
        self.verification = None;
        self.keychain_behind = false;
    }
}

type Authorized = (Zeroizing<String>, u64, Arc<AccountApi>);

/// How long a refresh waits for another Hangar process to finish its own: longer than two
/// requests plus the keychain.
const PROCESS_LOCK_WAIT: Duration = Duration::from_secs(40);
const PROCESS_LOCK_POLL: Duration = Duration::from_millis(20);

/// The cross-process lock on [`account::LOCK_FILE`]; released when dropped. Empty when there is no
/// config folder (tests) or the file could not be opened (the in-process gate still holds).
struct ProcessLock(Option<File>);

impl Drop for ProcessLock {
    fn drop(&mut self) {
        if let Some(file) = &self.0 {
            // Closing the file releases the lock as well; unlocking first makes it immediate.
            let _ = file.unlock();
        }
    }
}

/// Blocks until this process holds the lock file in `dir`, at most `wait`.
fn acquire_process_lock(dir: &Path, wait: Duration) -> Result<ProcessLock, AccountError> {
    let opened = std::fs::create_dir_all(dir).and_then(|()| {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(account::LOCK_FILE))
    });
    let file = match opened {
        Ok(file) => file,
        Err(error) => {
            eprintln!("hangar: account lock file not opened: {error}");
            return Ok(ProcessLock(None));
        }
    };
    let deadline = Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(ProcessLock(Some(file))),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(PROCESS_LOCK_POLL);
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(AccountError::Network(
                    "another Hangar is refreshing the sign-in".to_owned(),
                ));
            }
            Err(std::fs::TryLockError::Error(error)) => {
                eprintln!("hangar: account lock file not locked: {error}");
                return Ok(ProcessLock(None));
            }
        }
    }
}

impl AccountService {
    pub fn new(
        base: BaseUrl,
        store: Arc<dyn SecretStore>,
        open: Opener,
        emit: Emit,
        options: Options,
    ) -> Self {
        let signed_in = options
            .config_dir
            .as_deref()
            .map(|dir| {
                account::load_config(dir)
                    .signed_in_for(&options.keychain_service)
                    .to_vec()
            })
            .unwrap_or_default();
        Self {
            inner: Arc::new(Inner {
                store,
                emit,
                open,
                options,
                core: Mutex::new(Core {
                    base,
                    api: None,
                    state: AccountState::Idle { notice: None },
                    tokens: Slot::Unloaded,
                    flow: 0,
                    next_generation: 0,
                    verification: None,
                    projects: Vec::new(),
                    signed_in,
                    keychain_behind: false,
                }),
                refresh_gate: tokio::sync::Mutex::new(()),
                cancel: Notify::new(),
            }),
        }
    }

    pub fn view(&self) -> AccountView {
        lock(&self.inner.core).view()
    }

    /// Notes in `account.json` whether `base` has a sign-in in this build's keychain service.
    /// Rereads the file first, so what another Hangar process wrote since is kept; the caller
    /// holds the process lock (`_held`) for that read and write.
    fn remember(&self, base: &BaseUrl, stored: bool, _held: &ProcessLock) {
        let service = &self.inner.options.keychain_service;
        let Some(dir) = &self.inner.options.config_dir else {
            let mut core = lock(&self.inner.core);
            let mut config = account::AccountConfig::default();
            config
                .signed_in
                .insert(service.clone(), core.signed_in.clone());
            config.note_sign_in(service, base, stored);
            core.signed_in = config.signed_in_for(service).to_vec();
            return;
        };
        let mut config = account::load_config(dir);
        if config.note_sign_in(service, base, stored) {
            if let Err(error) = account::save_config(dir, &config) {
                eprintln!("hangar: account settings not saved: {error}");
            }
        }
        lock(&self.inner.core).signed_in = config.signed_in_for(service).to_vec();
    }

    /// The process lock, or none when it cannot be had within [`PROCESS_LOCK_WAIT`]: for steps
    /// that must happen anyway (storing a confirmed sign-in, signing out, ending a refused
    /// session, changing the server); the in-process gate still holds.
    async fn process_lock_or_none(&self, step: &str) -> ProcessLock {
        self.process_lock().await.unwrap_or_else(|error| {
            eprintln!("hangar: account {step} without the lock: {error}");
            ProcessLock(None)
        })
    }

    async fn process_lock(&self) -> Result<ProcessLock, AccountError> {
        let Some(dir) = self.inner.options.config_dir.clone() else {
            return Ok(ProcessLock(None));
        };
        tauri::async_runtime::spawn_blocking(move || acquire_process_lock(&dir, PROCESS_LOCK_WAIT))
            .await
            .map_err(|error| AccountError::Config(error.to_string()))?
    }

    fn publish(&self) {
        let view = self.view();
        (self.inner.emit)(&view);
    }

    /// The current state; reads the keychain once per server first, so a stored sign-in counts.
    pub async fn status(&self) -> AccountView {
        self.ensure_loaded().await;
        self.view()
    }

    /// Reads the keychain if not done yet; a stored sign-in then learns the person's name in
    /// the background.
    async fn ensure_loaded(&self) {
        if self.load_tokens().await {
            self.publish();
            let service = self.clone();
            tauri::async_runtime::spawn(async move { service.fetch_me().await });
        }
    }

    /// `true` when this call found a stored sign-in. Never spawns, so the token path
    /// (`access_token`) does not await a future that spawns itself (no Send cycle).
    async fn load_tokens(&self) -> bool {
        let account = {
            let core = lock(&self.inner.core);
            if !matches!(core.tokens, Slot::Unloaded) {
                return false;
            }
            core.base.as_str().to_owned()
        };
        let loaded = self.keychain_get(account.clone()).await;
        let found = {
            let mut core = lock(&self.inner.core);
            if !matches!(core.tokens, Slot::Unloaded) || core.base.as_str() != account {
                return false;
            }
            match loaded {
                Ok(Some(refresh)) => {
                    let generation = core.generation();
                    core.tokens = Slot::Present(Tokens {
                        access: None,
                        refresh,
                        generation,
                    });
                    if matches!(core.state, AccountState::Idle { .. }) {
                        core.state = AccountState::SignedIn { user: None };
                    }
                    true
                }
                Ok(None) => {
                    core.tokens = Slot::Empty;
                    false
                }
                Err(error) => {
                    eprintln!("hangar: account sign-in not read from the keychain: {error}");
                    core.tokens = Slot::Empty;
                    return false;
                }
            }
        };
        if let Ok(base) = BaseUrl::parse(&account) {
            match self.process_lock().await {
                Ok(held) => self.remember(&base, found, &held),
                Err(error) => eprintln!("hangar: account settings not updated: {error}"),
            }
        }
        found
    }

    /// Starts the device flow: asks the server for a code, shows it, opens the confirmation page
    /// and polls in the background. A failure of the first request ends in `Failed`.
    pub async fn sign_in(&self) -> Result<AccountView, AccountError> {
        self.ensure_loaded().await;
        let (flow, api) = {
            let mut core = lock(&self.inner.core);
            if matches!(core.state, AccountState::SignedIn { .. }) {
                return Ok(core.view());
            }
            core.flow += 1;
            (core.flow, core.api()?)
        };
        // A previous flow stops waiting and sees that it is no longer current.
        self.inner.cancel.notify_waiters();
        let started = Instant::now();
        let options = &self.inner.options;
        let authorization = match api
            .device_authorization(&options.device_name, &options.platform)
            .await
        {
            Ok(authorization) => authorization,
            Err(error) => {
                self.fail_flow(flow, fail_reason(&error));
                return Ok(self.view());
            }
        };
        let base = api.base();
        let complete = authorization
            .verification_uri_complete
            .clone()
            .unwrap_or_else(|| authorization.verification_uri.clone());
        let (Some(page), Some(manual)) = (
            base.same_origin(&complete),
            base.same_origin(&authorization.verification_uri),
        ) else {
            self.fail_flow(
                flow,
                FailReason {
                    kind: FailKind::ForeignPage,
                    detail: None,
                },
            );
            return Ok(self.view());
        };
        let poll = DevicePoll::new(authorization.interval, authorization.expires_in);
        let unit_ms = u64::try_from(options.unit.as_millis()).unwrap_or(u64::MAX);
        let expires_at = crate::updates::now_millis()
            .saturating_add(authorization.expires_in.saturating_mul(unit_ms));
        {
            let mut core = lock(&self.inner.core);
            if core.flow != flow {
                return Ok(core.view());
            }
            core.verification = Some(page.clone());
            // The addresses as checked, not as sent.
            core.state = AccountState::WaitingForUser {
                user_code: authorization.user_code.clone(),
                verification_uri: manual.to_string(),
                verification_uri_complete: page.to_string(),
                expires_at,
                interval: poll.interval(),
            };
        }
        self.publish();
        // The code is on screen and "Open browser again" retries, so a failure only goes to the log.
        if let Err(error) = (self.inner.open)(&page) {
            eprintln!("hangar: sign-in page not opened: {error}");
        }
        let service = self.clone();
        let device_code = authorization.device_code;
        tauri::async_runtime::spawn(async move {
            service.run_flow(flow, device_code, poll, started).await;
        });
        Ok(self.view())
    }

    fn flow_current(&self, flow: u64) -> bool {
        lock(&self.inner.core).flow == flow
    }

    async fn run_flow(
        &self,
        flow: u64,
        device_code: Zeroizing<String>,
        mut poll: DevicePoll,
        started: Instant,
    ) {
        let unit = self.inner.options.unit;
        let unit_ms = unit.as_millis().max(1);
        loop {
            let elapsed =
                u64::try_from(started.elapsed().as_millis() / unit_ms).unwrap_or(u64::MAX);
            let Some(wait) = poll.next_wait(elapsed) else {
                self.fail_flow(
                    flow,
                    FailReason {
                        kind: FailKind::Expired,
                        detail: None,
                    },
                );
                return;
            };
            // Registered before the check, so a cancel in between still wakes it.
            let cancelled = self.inner.cancel.notified();
            if !self.flow_current(flow) {
                return;
            }
            let pause = unit.saturating_mul(u32::try_from(wait).unwrap_or(u32::MAX));
            let _ = tokio::time::timeout(pause, cancelled).await;
            let api = {
                let mut core = lock(&self.inner.core);
                if core.flow != flow {
                    return;
                }
                match core.api() {
                    Ok(api) => api,
                    Err(_) => return,
                }
            };
            let response = api.poll_token(&device_code).await;
            if !self.flow_current(flow) {
                return;
            }
            match poll.on_response(response) {
                PollStep::Wait(_) => {}
                PollStep::SignedIn(tokens) => {
                    self.finish_sign_in(flow, tokens).await;
                    return;
                }
                PollStep::Failed(reason) => {
                    self.fail_flow(flow, reason);
                    return;
                }
            }
        }
    }

    fn fail_flow(&self, flow: u64, reason: FailReason) {
        {
            let mut core = lock(&self.inner.core);
            if core.flow != flow {
                return;
            }
            core.verification = None;
            core.state = AccountState::Failed { reason };
        }
        self.publish();
    }

    async fn finish_sign_in(&self, flow: u64, tokens: TokenSet) {
        let Some(refresh) = tokens.refresh else {
            self.fail_flow(
                flow,
                FailReason {
                    kind: FailKind::BadResponse,
                    detail: None,
                },
            );
            return;
        };
        let gate = self.inner.refresh_gate.lock().await;
        if !self.flow_current(flow) {
            return;
        }
        // The server already issued the pair, so it is stored even without the other processes'
        // lock; with it, no other process reads or rotates the entry halfway.
        let process = self.process_lock_or_none("sign-in").await;
        let (account, base) = {
            let core = lock(&self.inner.core);
            if core.flow != flow {
                return;
            }
            (core.base.as_str().to_owned(), core.base.clone())
        };
        if let Err(error) = self.keychain_set(account.clone(), refresh.clone()).await {
            self.fail_flow(
                flow,
                FailReason {
                    kind: FailKind::Keychain,
                    detail: Some(error.to_string()),
                },
            );
            return;
        }
        let stored = {
            let mut core = lock(&self.inner.core);
            if core.flow == flow {
                let generation = core.generation();
                core.tokens = Slot::Present(Tokens {
                    access: Some(Access::new(tokens.access, tokens.expires_in)),
                    refresh,
                    generation,
                });
                core.state = AccountState::SignedIn { user: None };
                core.verification = None;
                core.keychain_behind = false;
                true
            } else {
                false
            }
        };
        if !stored {
            // Cancelled while the keychain was written: take it back out.
            if let Err(error) = self.keychain_delete(account).await {
                eprintln!("hangar: cancelled account sign-in not removed: {error}");
            }
            return;
        }
        self.remember(&base, true, &process);
        drop(process);
        drop(gate);
        self.publish();
        self.fetch_me().await;
    }

    /// Ends a waiting or failed flow; the state returns to `Idle`.
    pub fn cancel_sign_in(&self) -> AccountView {
        let changed = {
            let mut core = lock(&self.inner.core);
            let ends = matches!(
                core.state,
                AccountState::WaitingForUser { .. } | AccountState::Failed { .. }
            );
            if ends {
                core.flow += 1;
                core.verification = None;
                core.state = AccountState::Idle { notice: None };
            }
            ends
        };
        self.inner.cancel.notify_waiters();
        if changed {
            self.publish();
        }
        self.view()
    }

    /// Revokes the device on the server (RFC 7009, best effort, a few seconds at most) and drops
    /// the tokens here. Should the server not take it, the sign-out still happens on this device
    /// and the notice says the device stays listed on the account page.
    pub async fn sign_out(&self) -> Result<AccountView, AccountError> {
        lock(&self.inner.core).flow += 1;
        self.inner.cancel.notify_waiters();
        let _gate = self.inner.refresh_gate.lock().await;
        // Signing out must always work here, with or without the other processes' lock.
        let process = self.process_lock_or_none("sign-out").await;
        let (account, base, memory, api, behind) = {
            let mut core = lock(&self.inner.core);
            let memory = match &core.tokens {
                Slot::Present(tokens) => Some(tokens.refresh.clone()),
                _ => None,
            };
            let api = core.api().ok();
            (
                core.base.as_str().to_owned(),
                core.base.clone(),
                memory,
                api,
                core.keychain_behind,
            )
        };
        // The newest token: another process may have rotated it, unless the last rotation here
        // could not be stored (then the keychain holds a spent one and memory the current one).
        let stored = self
            .keychain_get(account.clone())
            .await
            .unwrap_or_else(|error| {
                eprintln!("hangar: account sign-in not read for the sign-out: {error}");
                None
            });
        let newest = if behind {
            memory.or(stored)
        } else {
            stored.or(memory)
        };
        let revoked = match (newest, api) {
            (None, _) => true,
            (Some(token), Some(api)) => match api.revoke(&token).await {
                Ok(()) => true,
                Err(error) => {
                    eprintln!("hangar: device not removed from the account: {error}");
                    false
                }
            },
            (Some(_), None) => false,
        };
        let notice = if revoked {
            IdleNotice::SignedOut
        } else {
            IdleNotice::SignedOutLocally
        };
        lock(&self.inner.core).clear(notice);
        let deleted = self.keychain_delete(account).await;
        if deleted.is_ok() {
            self.remember(&base, false, &process);
        }
        drop(process);
        drop(_gate);
        self.publish();
        deleted
            .map(|()| self.view())
            .map_err(|error| AccountError::Keychain(error.to_string()))
    }

    /// Signs out locally because the server no longer accepts the tokens of `generation`.
    /// Called with `refresh_gate` and then the process lock (`held`) held.
    async fn end_session(&self, generation: u64, held: &ProcessLock) {
        let (account, base) = {
            let mut core = lock(&self.inner.core);
            let current = matches!(&core.tokens, Slot::Present(t) if t.generation == generation);
            if !current {
                return;
            }
            core.clear(IdleNotice::SessionEnded);
            (core.base.as_str().to_owned(), core.base.clone())
        };
        match self.keychain_delete(account).await {
            Ok(()) => self.remember(&base, false, held),
            Err(error) => {
                eprintln!("hangar: ended account sign-in not removed from the keychain: {error}");
            }
        }
        self.publish();
    }

    /// A valid access token, refreshed first when needed.
    async fn access_token(&self) -> Result<Authorized, AccountError> {
        self.load_tokens().await;
        let seen = {
            let mut core = lock(&self.inner.core);
            let api = core.api()?;
            match &core.tokens {
                Slot::Present(tokens) => {
                    if let Some(access) = tokens.valid_access() {
                        return Ok((access, tokens.generation, api));
                    }
                    tokens.generation
                }
                _ => return Err(core.signed_out_error()),
            }
        };
        self.refreshed_after(seen).await
    }

    /// Single-flight refresh: waits for any running one, and refreshes only when the tokens are
    /// still those of `seen` (a newer set from another caller is used as it is). Holds the
    /// cross-process lock and redeems the newest token in the keychain.
    async fn refreshed_after(&self, seen: u64) -> Result<Authorized, AccountError> {
        let _gate = self.inner.refresh_gate.lock().await;
        let (memory, generation, api, account) = {
            let mut core = lock(&self.inner.core);
            let api = core.api()?;
            let account = core.base.as_str().to_owned();
            match &core.tokens {
                Slot::Present(tokens) => {
                    if tokens.generation != seen {
                        if let Some(access) = tokens.valid_access() {
                            return Ok((access, tokens.generation, api));
                        }
                    }
                    (tokens.refresh.clone(), tokens.generation, api, account)
                }
                _ => return Err(core.signed_out_error()),
            }
        };
        let process = self.process_lock().await?;
        let refresh = match self.keychain_get(account.clone()).await {
            // Another Hangar may have rotated the token since this one read it.
            Ok(Some(stored)) if !lock(&self.inner.core).keychain_behind => stored,
            Ok(Some(_)) => memory,
            Ok(None) => {
                // Another Hangar signed out, which also revoked the device on the server.
                self.end_session(generation, &process).await;
                return Err(AccountError::SessionEnded);
            }
            Err(error) => {
                eprintln!("hangar: account sign-in not read before the refresh: {error}");
                memory
            }
        };
        let mut answer = api.refresh(&refresh).await;
        if matches!(&answer, Err(error) if error.is_transient()) {
            // The server may have rotated before the answer was lost. Within its grace the same
            // token gets a fresh pair once; any later attempt would count as reuse.
            tokio::time::sleep(self.inner.options.unit).await;
            answer = api.refresh(&refresh).await;
        }
        match answer {
            Ok(set) => {
                let rotated = set.refresh.unwrap_or(refresh);
                // Stored before use: the old token is spent on the server now. Should the keychain
                // fail, this run keeps working from memory; the next start may then present the
                // spent token and be signed out (the server revokes the device).
                let behind = match self.keychain_set(account, rotated.clone()).await {
                    Ok(()) => false,
                    Err(error) => {
                        eprintln!(
                            "hangar: rotated account sign-in not stored in the keychain: {error}"
                        );
                        true
                    }
                };
                let mut core = lock(&self.inner.core);
                core.keychain_behind = behind;
                let current =
                    matches!(&core.tokens, Slot::Present(t) if t.generation == generation);
                if !current {
                    return Err(AccountError::NotSignedIn);
                }
                let generation = core.generation();
                let access = set.access.clone();
                core.tokens = Slot::Present(Tokens {
                    access: Some(Access::new(set.access, set.expires_in)),
                    refresh: rotated,
                    generation,
                });
                Ok((access, generation, api))
            }
            Err(ApiError::OAuth(OAuthCode::InvalidGrant, _) | ApiError::Unauthorized) => {
                self.end_session(generation, &process).await;
                Err(AccountError::SessionEnded)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Runs `call` with an access token; on 401 refreshes once and tries again, and a second 401
    /// ends the sign-in.
    async fn authorized<T, F, Fut>(&self, call: F) -> Result<T, AccountError>
    where
        F: Fn(Arc<AccountApi>, Zeroizing<String>) -> Fut,
        Fut: Future<Output = Result<T, ApiError>>,
    {
        let (token, generation, api) = self.access_token().await?;
        match call(api, token).await {
            Err(ApiError::Unauthorized) => {}
            other => return other.map_err(AccountError::from),
        }
        let (token, generation, api) = self.refreshed_after(generation).await?;
        match call(api, token).await {
            Err(ApiError::Unauthorized) => {
                let _gate = self.inner.refresh_gate.lock().await;
                // The server refused twice: the sign-in ends here even without the lock.
                let process = self.process_lock_or_none("session end").await;
                self.end_session(generation, &process).await;
                Err(AccountError::SessionEnded)
            }
            other => other.map_err(AccountError::from),
        }
    }

    /// Fills in the signed-in person; failures leave `user` empty (the next status tries again).
    async fn fetch_me(&self) {
        let me = self
            .authorized(|api, token| async move { api.me(&token).await })
            .await;
        match me {
            Ok(me) => {
                {
                    let mut core = lock(&self.inner.core);
                    if !matches!(core.state, AccountState::SignedIn { .. }) {
                        return;
                    }
                    core.state = AccountState::SignedIn { user: Some(me) };
                }
                self.publish();
            }
            Err(error) => eprintln!("hangar: account details not loaded: {error}"),
        }
    }

    /// The account's projects (teams), fresh from the server.
    pub async fn projects(&self) -> Result<Vec<Project>, AccountError> {
        let projects = self
            .authorized(|api, token| async move { api.projects(&token).await })
            .await?;
        lock(&self.inner.core).projects.clone_from(&projects);
        // A sign-in loaded at start learns its name here too.
        let needs_user = matches!(
            lock(&self.inner.core).state,
            AccountState::SignedIn { user: None }
        );
        if needs_user {
            self.fetch_me().await;
        }
        Ok(projects)
    }

    /// A short-lived ticket for the relay of `project_id` (step D; not used by the window yet).
    pub async fn relay_ticket(
        &self,
        project_id: &str,
    ) -> Result<account::RelayTicket, AccountError> {
        let project_id = project_id.to_owned();
        self.authorized(move |api, token| {
            let project_id = project_id.clone();
            async move { api.relay_ticket(&token, &project_id).await }
        })
        .await
    }

    /// Switches the server (`None` or empty: the official one). Ends a waiting flow; a sign-in
    /// stored for the new server counts at once, the old one stays in the keychain for a switch
    /// back.
    pub async fn set_base_url(&self, text: Option<String>) -> Result<AccountView, AccountError> {
        let base = match text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            None => BaseUrl::official(),
            Some(text) => BaseUrl::parse(text).map_err(AccountError::BaseUrl)?,
        };
        {
            let mut core = lock(&self.inner.core);
            if core.base == base {
                return Ok(core.view());
            }
            core.flow += 1;
        }
        self.inner.cancel.notify_waiters();
        {
            let _gate = self.inner.refresh_gate.lock().await;
            let process = self.process_lock_or_none("server change").await;
            let mut signed_in = None;
            if let Some(dir) = &self.inner.options.config_dir {
                // Reread under the lock: the lists of other processes and builds stay as they are.
                let mut config = account::load_config(dir);
                config.base = base.clone();
                signed_in = Some(
                    config
                        .signed_in_for(&self.inner.options.keychain_service)
                        .to_vec(),
                );
                if let Err(error) = account::save_config(dir, &config) {
                    // The waiting flow was already stopped above; do not leave its code on screen.
                    let ended = {
                        let mut core = lock(&self.inner.core);
                        let waiting = matches!(core.state, AccountState::WaitingForUser { .. });
                        if waiting {
                            core.verification = None;
                            core.state = AccountState::Idle { notice: None };
                        }
                        waiting
                    };
                    if ended {
                        self.publish();
                    }
                    return Err(AccountError::Config(error.to_string()));
                }
            }
            drop(process);
            let mut core = lock(&self.inner.core);
            if let Some(signed_in) = signed_in {
                core.signed_in = signed_in;
            }
            core.base = base;
            core.api = None;
            core.tokens = Slot::Unloaded;
            core.state = AccountState::Idle { notice: None };
            core.projects.clear();
            core.verification = None;
            core.keychain_behind = false;
        }
        self.publish();
        self.ensure_loaded().await;
        Ok(self.view())
    }

    /// Opens one of the account's pages in the browser; only pages on the server's own origin.
    pub fn open(&self, page: AccountPage) -> Result<(), AccountError> {
        let url = {
            let core = lock(&self.inner.core);
            match page {
                AccountPage::Verification => core
                    .verification
                    .clone()
                    .ok_or(AccountError::NothingToOpen)?,
                AccountPage::Devices => core.base.devices_page(),
                AccountPage::Invite { project_id } => {
                    let project = core
                        .projects
                        .iter()
                        .find(|project| project.id == project_id)
                        .ok_or(AccountError::UnknownProject)?;
                    project
                        .invite_url
                        .as_deref()
                        .filter(|_| project.can_invite)
                        .and_then(|url| core.base.same_origin(url))
                        .ok_or(AccountError::ForeignPage)?
                }
            }
        };
        (self.inner.open)(&url).map_err(|error| AccountError::Open(error.to_string()))
    }

    async fn keychain_get(
        &self,
        account: String,
    ) -> Result<Option<Zeroizing<String>>, SecretError> {
        self.keychain(move |store| store.get(&account)).await
    }

    async fn keychain_set(
        &self,
        account: String,
        value: Zeroizing<String>,
    ) -> Result<(), SecretError> {
        self.keychain(move |store| store.set(&account, &value))
            .await
    }

    async fn keychain_delete(&self, account: String) -> Result<(), SecretError> {
        self.keychain(move |store| store.delete(&account)).await
    }

    /// The keychain may block (D-Bus on Linux, a prompt on macOS), so it runs off the async threads.
    async fn keychain<T: Send + 'static>(
        &self,
        work: impl FnOnce(&dyn SecretStore) -> Result<T, SecretError> + Send + 'static,
    ) -> Result<T, SecretError> {
        let store = Arc::clone(&self.inner.store);
        tauri::async_runtime::spawn_blocking(move || work(store.as_ref()))
            .await
            .map_err(|error| SecretError::Task(error.to_string()))?
    }
}

/// The window's reason for an error of the first device request.
fn fail_reason(error: &ApiError) -> FailReason {
    let (kind, detail) = match error {
        ApiError::Network(_) | ApiError::Timeout => (FailKind::Network, None),
        ApiError::OAuth(_, detail) => (FailKind::Refused, detail.clone()),
        ApiError::Status { problem, .. } => {
            (FailKind::Server, problem.as_ref().and_then(|p| p.message()))
        }
        ApiError::TooLarge | ApiError::Malformed | ApiError::Unauthorized => {
            (FailKind::BadResponse, None)
        }
    };
    FailReason { kind, detail }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_process_lock_waits_for_the_holder_and_gives_up_in_time() {
        let temp = crate::test_support::TempDir::new("account-process-lock");
        let dir = temp.path();
        let held = acquire_process_lock(dir, Duration::ZERO).unwrap();
        assert!(held.0.is_some(), "the lock file was locked");
        // A second handle stands for another process: it waits, then gives up.
        let started = Instant::now();
        let refused = acquire_process_lock(dir, Duration::from_millis(100));
        assert!(matches!(refused, Err(AccountError::Network(_))));
        assert!(started.elapsed() >= Duration::from_millis(100));
        // Released by drop, it is free at once.
        drop(held);
        let again = acquire_process_lock(dir, Duration::ZERO).unwrap();
        assert!(again.0.is_some());
        // Released by the holder going away mid-wait.
        let waiter = std::thread::spawn({
            let dir = dir.to_path_buf();
            move || acquire_process_lock(&dir, Duration::from_secs(5)).map(|lock| lock.0.is_some())
        });
        std::thread::sleep(Duration::from_millis(50));
        drop(again);
        assert!(matches!(waiter.join(), Ok(Ok(true))));
    }
}

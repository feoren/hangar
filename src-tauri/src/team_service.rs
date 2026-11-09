//! Team connections at run time (team service step F part 2a): one worker per project with
//! "Share with team" on, each with one WebSocket to the relay.
//!
//! A worker first takes the team's lock file, so only one Hangar window on this computer runs a
//! team (a second one shows [`AttentionReason::OtherInstance`] and takes over once the first
//! ends). Then it loops: learn the person's role from the project list, load or create this
//! device's identity (keychain), fetch a fresh relay ticket, connect, and run the protocol engine
//! (`team_engine.rs`) on the relay's frames, its own timers (ticket renewal every 5 minutes, a
//! reconnect before the relay's connection age, pings, devices waiting for confirmation) and the
//! window's commands. When the connection ends, the reconnect policy (`team.rs`) decides between
//! reconnecting after a jittered backoff and stopping with a status the window shows.
//!
//! Trust: the relay's events are not signed. Keys are deleted only when the verified membership
//! log removed this device, or when the website (authenticated HTTPS) no longer lists the project
//! after the relay said it was deleted or that this member was removed; every such deletion also
//! turns sharing off, so the next start does not register a new identity unasked.
//!
//! Storage: the device identity (`device/<key>`) and the verified log head with the newest epoch
//! secrets (`keys/<key>`) live in the OS keychain under the build's keychain service
//! ([`team::KEYCHAIN_SERVICE`]); the event log cursor, send counter, replay marks and rejected
//! requests (nothing secret) in `<local data>/team/<service>/<key>.json`; the enabled projects in
//! `<config>/team.json`, per service.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::future::{select, Either};
use futures_util::{SinkExt as _, StreamExt as _};
use hangar_team_crypto::identity::DeviceIdentity;
use hangar_team_crypto::{Entropy as _, OsEntropy};
use hangar_team_protocol::ids::{DeviceId, MemberId, TeamId, Ticket};
use hangar_team_protocol::wire::{encode_client, Holder, Role};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;
use zeroize::Zeroizing;

use crate::account::{self, AccountError, AccountState, AccountView};
use crate::account_service::AccountService;
use crate::claims::WorktreeClaims;
use crate::lock;
use crate::secrets::SecretStore;
use crate::session::SessionId;
use crate::team::{
    self, device_account, keys_account, team_key, AttentionReason, ConnectionClock, Due,
    LinkedRepo, LocalState, Next, PendingConfirmation, ReconnectPolicy, SessionEnd, StopReason,
    StoredDevice, StoredKeys, TeamConfig, TeamEntry, TeamError, TeamSnapshot, TeamStatus, Timing,
};
use crate::team_engine::{Effect, Engine, EngineSetup};
use crate::team_share::{
    self, LocalSession, LocalShare, SessionLabel, SessionState, TeamActivity, TeammateFile,
};

/// Where this device's sessions come from: the claims' worktrees and the session list.
pub type LocalSource = Box<dyn Fn() -> (Arc<Vec<WorktreeClaims>>, Vec<SessionLabel>) + Send + Sync>;

pub type Emit = Arc<dyn Fn(&TeamSnapshot) + Send + Sync>;

/// A team's turn: the running worker holds it (`team_service::Inner::turns`).
type Turn = Arc<tokio::sync::Mutex<()>>;

/// Frames read ahead of the worker; when it falls behind, the reader stops reading the socket.
const READ_QUEUE: usize = 32;
/// How long a new worker waits for the previous worker of the same team (turned off and on
/// again, signed out and in) to let go of the team before it tries the lock file itself.
const HANDOVER: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub struct Options {
    /// Where `team.json` lives; `None` keeps the enabled projects in memory only.
    pub config_dir: Option<PathBuf>,
    /// Where the per-team state and lock files live; `None` keeps the state in memory only and
    /// takes no lock.
    pub state_dir: Option<PathBuf>,
    pub timing: Timing,
    /// The keychain service the store writes to; names this build's part of `team.json` and its
    /// state folder, so debug and release builds never share a device's state.
    pub keychain_service: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            config_dir: None,
            state_dir: None,
            timing: Timing::default(),
            keychain_service: team::KEYCHAIN_SERVICE.to_owned(),
        }
    }
}

enum Command {
    Confirm(String, oneshot::Sender<Result<(), TeamError>>),
    Reject(String, oneshot::Sender<Result<(), TeamError>>),
    RemoveMember(MemberId, oneshot::Sender<Result<(), TeamError>>),
    RemoveDevice(Holder, oneshot::Sender<Result<(), TeamError>>),
    /// What the project shares from this device now (part 2b).
    Share(LocalShare),
    Stop,
}

/// What the reader of one connection hands the worker.
enum Frame {
    Text(String),
    Closed(Option<u16>),
}

struct Team {
    status: TeamStatus,
    pending: Vec<PendingConfirmation>,
    activity: TeamActivity,
    /// The teammates' files, for the agent gate and the agent's context.
    files: Vec<TeammateFile>,
    /// The share last handed to the worker.
    shared: Option<LocalShare>,
    tx: Option<mpsc::UnboundedSender<Command>>,
    worker: Option<tauri::async_runtime::JoinHandle<()>>,
    /// Rises with every start, so an ended worker does not overwrite a newer one's status.
    run: u64,
}

#[derive(Clone)]
pub struct TeamService {
    inner: Arc<Inner>,
}

struct Inner {
    account: AccountService,
    store: Arc<dyn SecretStore>,
    emit: Emit,
    options: Options,
    /// By (server address, project id).
    teams: Mutex<BTreeMap<(String, String), Team>>,
    /// The enabled projects when there is no config folder.
    memory_config: Mutex<TeamConfig>,
    next_run: Mutex<u64>,
    /// The server address the workers run for; a change stops them and starts the new server's.
    base: Mutex<String>,
    /// This device's running agent sessions in repositories, all projects (`set_local`).
    local: Mutex<Vec<LocalSession>>,
    /// What the window says each session is doing.
    states: Mutex<BTreeMap<SessionId, SessionState>>,
    source: Mutex<Option<LocalSource>>,
    /// One turn per team: held by its worker for its whole run, so a new worker of the same team
    /// waits for the old one to let go instead of meeting its lock file.
    turns: Mutex<BTreeMap<(String, String), Turn>>,
}

/// What the website says about the person and the project.
enum Website {
    Member(Role),
    /// The project is not in the person's list (deleted, or the person left it).
    Gone,
    SignedOut,
    /// No answer; nothing can be concluded.
    Unreachable,
}

fn map_role(role: account::Role) -> Role {
    match role {
        account::Role::Owner => Role::Owner,
        account::Role::Admin => Role::Admin,
        account::Role::Member | account::Role::Other => Role::Member,
    }
}

fn jitter() -> f64 {
    let mut bytes = [0u8; 4];
    if OsEntropy.fill(&mut bytes).is_err() {
        return 0.5;
    }
    f64::from(u32::from_be_bytes(bytes)) / 4_294_967_296.0
}

fn attention(reason: AttentionReason) -> SessionEnd {
    SessionEnd::Stop(StopReason::Attention(reason))
}

impl TeamService {
    /// The service follows `account`: a sign-out stops the running teams at once, a sign-in
    /// restarts teams that stopped for want of one, and
    /// a server change stops the old server's workers and starts the new one's.
    pub fn new(
        account: AccountService,
        store: Arc<dyn SecretStore>,
        emit: Emit,
        options: Options,
    ) -> Self {
        let base = account.view().base_url;
        let service = Self {
            inner: Arc::new(Inner {
                account,
                store,
                emit,
                options,
                teams: Mutex::new(BTreeMap::new()),
                memory_config: Mutex::new(TeamConfig::default()),
                next_run: Mutex::new(0),
                base: Mutex::new(base),
                local: Mutex::new(Vec::new()),
                states: Mutex::new(BTreeMap::new()),
                source: Mutex::new(None),
                turns: Mutex::new(BTreeMap::new()),
            }),
        };
        // Weak: the account outlives no team service it would keep alive.
        let weak = Arc::downgrade(&service.inner);
        service
            .inner
            .account
            .subscribe(Arc::new(move |view: &AccountView| {
                if let Some(inner) = weak.upgrade() {
                    TeamService { inner }.account_changed(view);
                }
            }));
        service
    }

    fn base(&self) -> String {
        self.inner.account.view().base_url
    }

    fn service_name(&self) -> &str {
        &self.inner.options.keychain_service
    }

    fn config(&self) -> Result<TeamConfig, TeamError> {
        match &self.inner.options.config_dir {
            Some(dir) => {
                TeamConfig::load(dir).map_err(|error| TeamError::Config(error.to_string()))
            }
            None => Ok(lock(&self.inner.memory_config).clone()),
        }
    }

    /// The enabled projects; none while `team.json` cannot be read (it stays untouched).
    fn enabled(&self, base: &str) -> Vec<String> {
        match self.config() {
            Ok(config) => config.enabled_for(self.service_name(), base).to_vec(),
            Err(error) => {
                eprintln!("hangar: team settings not read: {error}");
                Vec::new()
            }
        }
    }

    /// Changes the stored choices with `change`; a file that cannot be read is never written
    /// over, so the other projects' and the other build's choices survive.
    fn update_config(&self, change: impl FnOnce(&mut TeamConfig) -> bool) -> Result<(), TeamError> {
        match &self.inner.options.config_dir {
            Some(dir) => {
                // Reread first: the other build's part of the file stays as it is.
                let mut config =
                    TeamConfig::load(dir).map_err(|error| TeamError::Config(error.to_string()))?;
                if change(&mut config) {
                    config
                        .save(dir)
                        .map_err(|error| TeamError::Config(error.to_string()))?;
                }
            }
            None => {
                change(&mut lock(&self.inner.memory_config));
            }
        }
        Ok(())
    }

    fn set_enabled(&self, base: &str, project: &str, on: bool) -> Result<(), TeamError> {
        let service = self.service_name().to_owned();
        self.update_config(|config| config.set(&service, base, project, on))
    }

    /// The repositories linked to `project`; none while `team.json` cannot be read.
    fn linked(&self, base: &str, project: &str) -> Vec<LinkedRepo> {
        self.config()
            .map(|config| config.linked(self.service_name(), base, project).to_vec())
            .unwrap_or_default()
    }

    /// The person's display name for the teammates: the name the website keeps for the account,
    /// empty without one (the teammates then see the short member id). Never the e-mail address:
    /// it is not the team's to read.
    fn person(&self) -> String {
        match self.inner.account.view().state {
            AccountState::SignedIn { user: Some(me) } => me.name.unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// What `project` shares: the sessions in its linked repository folders.
    fn share_for(&self, base: &str, project: &str) -> LocalShare {
        let linked = self.linked(base, project);
        let sessions = lock(&self.inner.local)
            .iter()
            .filter(|s| linked.iter().any(|r| r.shares(&s.repo, &s.folder)))
            .cloned()
            .collect();
        LocalShare {
            person: self.person(),
            sessions,
        }
    }

    /// Hands every running worker its project's share when it changed.
    fn reshare(&self) {
        let base = self.base();
        let running: Vec<String> = lock(&self.inner.teams)
            .iter()
            .filter(|((b, _), team)| *b == base && team.tx.is_some())
            .map(|((_, project), _)| project.clone())
            .collect();
        for project in running {
            let share = self.share_for(&base, &project);
            let mut teams = lock(&self.inner.teams);
            let Some(team) = teams.get_mut(&(base.clone(), project)) else {
                continue;
            };
            if team.shared.as_ref() == Some(&share) {
                continue;
            }
            if let Some(tx) = &team.tx {
                let _ = tx.send(Command::Share(share.clone()));
            }
            team.shared = Some(share);
        }
    }

    /// This device's running agent sessions in git repositories (all of them; each project gets
    /// only those in its linked repositories). Called whenever the claims or the window's
    /// session states change.
    pub fn set_local(&self, sessions: Vec<LocalSession>) {
        let changed = {
            let mut local = lock(&self.inner.local);
            let changed = *local != sessions;
            *local = sessions;
            changed
        };
        if changed {
            self.reshare();
            self.publish();
        }
    }

    /// Where [`Self::refresh_local`] reads this device's sessions from.
    pub fn set_local_source(
        &self,
        source: impl Fn() -> (Arc<Vec<WorktreeClaims>>, Vec<SessionLabel>) + Send + Sync + 'static,
    ) {
        *lock(&self.inner.source) = Some(Box::new(source));
        self.refresh_local();
    }

    /// Reads the sessions again (the claims changed, or the window's states).
    pub fn refresh_local(&self) {
        let read = lock(&self.inner.source).as_ref().map(|source| source());
        let Some((worktrees, labels)) = read else {
            return;
        };
        let states = lock(&self.inner.states).clone();
        self.set_local(team_share::local_sessions(&worktrees, &labels, &states));
    }

    /// What the window's attention logic says each session is doing (working, waiting, idle).
    pub fn set_session_states(&self, states: BTreeMap<SessionId, SessionState>) {
        let changed = {
            let mut current = lock(&self.inner.states);
            let changed = *current != states;
            *current = states;
            changed
        };
        if changed {
            self.refresh_local();
        }
    }

    /// Links (or unlinks) the clone at `folder` of the repository with root commit `root` to
    /// `project`: its sessions' presence and claims go to the team from now on (or no longer).
    /// Only a project with sharing on can be linked to, and only a folder of a running session.
    pub fn link_repo(
        &self,
        project: &str,
        root: &str,
        folder: &str,
        on: bool,
    ) -> Result<(), TeamError> {
        // The same check as the team's id, so nothing the protocol refuses lands in team.json.
        TeamId::new(project).map_err(|_| TeamError::UnknownProject)?;
        let base = self.base();
        if on && !self.enabled(&base).iter().any(|p| p == project) {
            return Err(TeamError::UnknownProject);
        }
        let name = lock(&self.inner.local)
            .iter()
            .find(|s| s.repo == root && s.folder == folder)
            .map(|s| s.repo_name.clone());
        let linked = self.linked(&base, project);
        let repo = match (name, linked.iter().find(|r| r.shares(root, folder))) {
            (_, Some(known)) => known.clone(),
            (Some(name), None) => LinkedRepo {
                root: root.to_owned(),
                folder: Some(folder.to_owned()),
                name,
            },
            // Only a repository folder of a running session can be linked.
            (None, None) if on => return Err(TeamError::UnknownRepository),
            (None, None) => return Ok(()),
        };
        let service = self.service_name().to_owned();
        self.update_config(|config| config.link(&service, &base, project, &repo, on))?;
        self.reshare();
        self.publish();
        Ok(())
    }

    /// Every file a teammate's session is changing in the repository with root commit `repo`,
    /// over the running teams of the current server whose project this device linked the clone
    /// at `folder` to. A teammate of another project, or of a project only another clone is
    /// linked to, does not stop an edit here.
    pub fn teammate_files(&self, repo: &str, folder: &str) -> Vec<TeammateFile> {
        let base = self.base();
        let config = self.config().unwrap_or_default();
        let service = self.service_name();
        lock(&self.inner.teams)
            .iter()
            .filter(|((b, project), _)| {
                *b == base
                    && config
                        .linked(service, &base, project)
                        .iter()
                        .any(|r| r.shares(repo, folder))
            })
            .flat_map(|(_, team)| team.files.iter())
            .filter(|file| file.repo == repo)
            .cloned()
            .collect()
    }

    fn turn(&self, base: &str, project: &str) -> Turn {
        let mut turns = lock(&self.inner.turns);
        Arc::clone(
            turns
                .entry((base.to_owned(), project.to_owned()))
                .or_default(),
        )
    }

    /// Starts the workers of every enabled project of the current server (at app start and
    /// after a server change).
    pub fn start_enabled(&self) {
        let base = self.base();
        for project in self.enabled(&base) {
            self.spawn(&base, &project);
        }
    }

    /// Follows the account's view (`AccountService::subscribe`). The account publishes from
    /// several threads, so a view can arrive after a newer one (a sign-out's after the next
    /// sign-in's); the service acts on the account's current view, never on a stale one.
    fn account_changed(&self, _published: &AccountView) {
        let current = self.inner.account.view();
        let view = &current;
        let changed = {
            let mut base = lock(&self.inner.base);
            let changed = *base != view.base_url;
            base.clone_from(&view.base_url);
            changed
        };
        if changed {
            self.stop_other_servers(&view.base_url);
            self.start_enabled();
            self.publish();
            return;
        }
        match view.state {
            AccountState::SignedIn { .. } => self.restart_signed_out(&view.base_url),
            // Signed out (by the person, or the server ended the sign-in): the connections close
            // now instead of at the next ticket renewal, which is up to 5 minutes away.
            AccountState::Idle { .. } | AccountState::Failed { .. } => {
                self.stop_signed_out(&view.base_url);
            }
            AccountState::WaitingForUser { .. } => {}
        }
    }

    /// Stops the running teams of `base` with [`AttentionReason::SignedOut`]; the next sign-in
    /// starts them again ([`Self::restart_signed_out`]).
    fn stop_signed_out(&self, base: &str) {
        let changed = {
            let mut teams = lock(&self.inner.teams);
            let mut changed = false;
            for ((team_base, _), team) in teams.iter_mut() {
                if team_base != base {
                    continue;
                }
                if let Some(tx) = team.tx.take() {
                    let _ = tx.send(Command::Stop);
                    team.worker = None;
                    team.status = TeamStatus::NeedsAttention {
                        reason: AttentionReason::SignedOut,
                    };
                    team.pending.clear();
                    team.activity = TeamActivity::default();
                    team.files.clear();
                    changed = true;
                }
            }
            changed
        };
        if changed {
            self.publish();
        }
    }

    /// Stops and forgets the teams of every server but `base`.
    fn stop_other_servers(&self, base: &str) {
        let mut teams = lock(&self.inner.teams);
        teams.retain(|(team_base, _), team| {
            if team_base == base {
                return true;
            }
            if let Some(tx) = team.tx.take() {
                let _ = tx.send(Command::Stop);
            }
            false
        });
    }

    /// Starts again the enabled teams of `base` that stopped because nobody was signed in, or
    /// because another account was signed in.
    fn restart_signed_out(&self, base: &str) {
        let waiting = |status: &TeamStatus| {
            matches!(
                status,
                TeamStatus::NeedsAttention {
                    reason: AttentionReason::SignedOut | AttentionReason::OtherAccount
                }
            )
        };
        let stopped: Vec<String> = {
            let teams = lock(&self.inner.teams);
            teams
                .iter()
                .filter(|((b, _), team)| b == base && team.tx.is_none() && waiting(&team.status))
                .map(|((_, project), _)| project.clone())
                .collect()
        };
        if stopped.is_empty() {
            return;
        }
        let enabled = self.enabled(base);
        for project in stopped.iter().filter(|p| enabled.contains(p)) {
            self.spawn(base, project);
        }
    }

    pub fn snapshot(&self) -> TeamSnapshot {
        let base = self.base();
        let config = self.config().unwrap_or_default();
        let mut snapshot = TeamSnapshot {
            repos: team_share::repo_choices(&lock(&self.inner.local)),
            ..TeamSnapshot::default()
        };
        let teams = lock(&self.inner.teams);
        for ((team_base, project), team) in teams.iter() {
            if *team_base != base {
                continue;
            }
            if team.status != TeamStatus::Off {
                let mut activity = team.activity.clone();
                activity.linked = config.linked(self.service_name(), &base, project).to_vec();
                snapshot.teams.push(TeamEntry {
                    project_id: project.clone(),
                    status: team.status.clone(),
                    activity,
                });
            }
            snapshot.pending.extend(team.pending.iter().cloned());
        }
        snapshot
    }

    fn publish(&self) {
        let snapshot = self.snapshot();
        (self.inner.emit)(&snapshot);
    }

    pub fn status(&self, project: &str) -> TeamStatus {
        let key = (self.base(), project.to_owned());
        lock(&self.inner.teams)
            .get(&key)
            .map_or(TeamStatus::Off, |team| team.status.clone())
    }

    pub fn pending(&self) -> Vec<PendingConfirmation> {
        self.snapshot().pending
    }

    /// Turns "Share with team" on for `project`: remembered, and the worker starts.
    pub async fn enable(&self, project: &str) -> Result<TeamStatus, TeamError> {
        if !matches!(
            self.inner.account.status().await.state,
            AccountState::SignedIn { .. }
        ) {
            return Err(TeamError::NotSignedIn);
        }
        let projects = self
            .inner
            .account
            .projects()
            .await
            .map_err(|error| match error {
                AccountError::NotSignedIn | AccountError::SessionEnded => TeamError::NotSignedIn,
                _ => TeamError::Busy,
            })?;
        if !projects.iter().any(|p| p.id == project) {
            return Err(TeamError::UnknownProject);
        }
        let base = self.base();
        self.set_enabled(&base, project, true)?;
        self.spawn(&base, project);
        Ok(self.status(project))
    }

    /// Turns sharing off: the connection closes; the device keeps its keys for a later return.
    pub async fn disable(&self, project: &str) -> Result<TeamStatus, TeamError> {
        let base = self.base();
        self.set_enabled(&base, project, false)?;
        let worker = {
            let mut teams = lock(&self.inner.teams);
            teams.get_mut(&(base, project.to_owned())).and_then(|team| {
                team.status = TeamStatus::Off;
                team.pending.clear();
                team.activity = TeamActivity::default();
                team.files.clear();
                if let Some(tx) = team.tx.take() {
                    let _ = tx.send(Command::Stop);
                }
                team.worker.take()
            })
        };
        if let Some(worker) = worker {
            let _ = tokio::time::timeout(Duration::from_secs(3), worker).await;
        }
        self.publish();
        Ok(TeamStatus::Off)
    }

    /// Ends every connection (app exit), waiting a moment for the close frames.
    pub fn shutdown(&self) {
        let workers: Vec<_> = {
            let mut teams = lock(&self.inner.teams);
            teams
                .values_mut()
                .filter_map(|team| {
                    if let Some(tx) = team.tx.take() {
                        let _ = tx.send(Command::Stop);
                    }
                    team.worker.take()
                })
                .collect()
        };
        // Inside the runtime (never at app exit) the workers close on their own.
        if workers.is_empty() || tokio::runtime::Handle::try_current().is_ok() {
            return;
        }
        tauri::async_runtime::block_on(async move {
            let all = futures_util::future::join_all(workers);
            let _ = tokio::time::timeout(Duration::from_secs(2), all).await;
        });
    }

    fn spawn(&self, base: &str, project: &str) {
        let run = {
            let mut next = lock(&self.inner.next_run);
            *next += 1;
            *next
        };
        let (tx, rx) = mpsc::unbounded_channel();
        {
            let mut teams = lock(&self.inner.teams);
            let team = teams
                .entry((base.to_owned(), project.to_owned()))
                .or_insert_with(|| Team {
                    status: TeamStatus::Off,
                    pending: Vec::new(),
                    activity: TeamActivity::default(),
                    files: Vec::new(),
                    shared: None,
                    tx: None,
                    worker: None,
                    run,
                });
            if team.tx.as_ref().is_some_and(|tx| !tx.is_closed()) {
                return;
            }
            team.run = run;
            team.status = TeamStatus::Connecting { retrying: false };
            team.pending.clear();
            team.activity = TeamActivity::default();
            team.files.clear();
            team.shared = None;
            team.tx = Some(tx);
        }
        let worker = Worker {
            service: self.clone(),
            base: base.to_owned(),
            project: project.to_owned(),
            key: team_key(base, project),
            run,
            rx,
            engine: None,
            share: LocalShare::default(),
            policy: ReconnectPolicy::default(),
            _lock: None,
            writer_closed: false,
        };
        let handle = tauri::async_runtime::spawn(worker.run());
        if let Some(team) = lock(&self.inner.teams).get_mut(&(base.to_owned(), project.to_owned()))
        {
            if team.run == run {
                team.worker = Some(handle);
            }
        }
        self.reshare();
        self.publish();
    }

    async fn command(
        &self,
        id: &str,
        make: impl FnOnce(oneshot::Sender<Result<(), TeamError>>) -> Command,
    ) -> Result<(), TeamError> {
        let base = self.base();
        let tx = {
            let teams = lock(&self.inner.teams);
            teams
                .iter()
                .find(|((b, _), team)| *b == base && team.pending.iter().any(|p| p.id == id))
                .and_then(|(_, team)| team.tx.clone())
                .ok_or(TeamError::UnknownRequest)?
        };
        Self::send_command(tx, make).await
    }

    async fn send_command(
        tx: mpsc::UnboundedSender<Command>,
        make: impl FnOnce(oneshot::Sender<Result<(), TeamError>>) -> Command,
    ) -> Result<(), TeamError> {
        let (reply, answer) = oneshot::channel();
        tx.send(make(reply)).map_err(|_| TeamError::Busy)?;
        answer.await.map_err(|_| TeamError::Busy)?
    }

    pub async fn confirm(&self, request: &str) -> Result<(), TeamError> {
        let id = request.to_owned();
        self.command(request, move |reply| Command::Confirm(id, reply))
            .await
    }

    pub async fn reject(&self, request: &str) -> Result<(), TeamError> {
        let id = request.to_owned();
        self.command(request, move |reply| Command::Reject(id, reply))
            .await
    }

    fn team_tx(&self, project: &str) -> Result<mpsc::UnboundedSender<Command>, TeamError> {
        let key = (self.base(), project.to_owned());
        lock(&self.inner.teams)
            .get(&key)
            .and_then(|team| team.tx.clone())
            .ok_or(TeamError::UnknownProject)
    }

    /// Removes a member from a team (owner or admin). Not a command yet (step F part 2b).
    pub async fn remove_member(&self, project: &str, member: &str) -> Result<(), TeamError> {
        let member = MemberId::new(member).map_err(|_| TeamError::UnknownRequest)?;
        let tx = self.team_tx(project)?;
        Self::send_command(tx, move |reply| Command::RemoveMember(member, reply)).await
    }

    /// Removes another device from a team. Not a command yet (step F part 2b).
    pub async fn remove_device(
        &self,
        project: &str,
        member: &str,
        device: &str,
    ) -> Result<(), TeamError> {
        let holder = Holder {
            member: MemberId::new(member).map_err(|_| TeamError::UnknownRequest)?,
            device: DeviceId::new(device).map_err(|_| TeamError::UnknownRequest)?,
        };
        let tx = self.team_tx(project)?;
        Self::send_command(tx, move |reply| Command::RemoveDevice(holder, reply)).await
    }

    /// Sets a worker's view, unless a newer worker took over the team.
    fn set_view(&self, base: &str, project: &str, run: u64, view: View) {
        let changed = {
            let mut teams = lock(&self.inner.teams);
            match teams.get_mut(&(base.to_owned(), project.to_owned())) {
                Some(team) if team.run == run && team.tx.is_some() => {
                    let changed = team.status != view.status
                        || team.pending != view.pending
                        || team.activity != view.activity;
                    team.status = view.status;
                    team.pending = view.pending;
                    team.activity = view.activity;
                    team.files = view.files;
                    changed
                }
                _ => false,
            }
        };
        if changed {
            self.publish();
        }
    }

    /// A stopped worker's final status (it stays until sharing is turned off and on, or a
    /// sign-in restarts it).
    fn finish(&self, base: &str, project: &str, run: u64, status: TeamStatus) {
        {
            let mut teams = lock(&self.inner.teams);
            if let Some(team) = teams.get_mut(&(base.to_owned(), project.to_owned())) {
                if team.run == run && team.tx.is_some() {
                    team.status = status;
                    team.pending.clear();
                    team.activity = TeamActivity::default();
                    team.files.clear();
                    team.tx = None;
                    team.worker = None;
                }
            }
        }
        self.publish();
    }

    // -----------------------------------------------------------------------------------------
    // Keychain, off the async threads (it may block: D-Bus, a prompt on macOS)
    // -----------------------------------------------------------------------------------------

    async fn keychain<T: Send + 'static>(
        &self,
        work: impl FnOnce(&dyn SecretStore) -> Result<T, crate::secrets::SecretError> + Send + 'static,
    ) -> Result<T, TeamError> {
        let store = Arc::clone(&self.inner.store);
        tauri::async_runtime::spawn_blocking(move || work(store.as_ref()))
            .await
            .map_err(|error| TeamError::Keychain(error.to_string()))?
            .map_err(|error| TeamError::Keychain(error.to_string()))
    }
}

// ---------------------------------------------------------------------------------------------
// The worker of one team
// ---------------------------------------------------------------------------------------------

/// What a worker shows of its team.
struct View {
    status: TeamStatus,
    pending: Vec<PendingConfirmation>,
    activity: TeamActivity,
    files: Vec<TeammateFile>,
}

struct Worker {
    service: TeamService,
    base: String,
    project: String,
    key: String,
    run: u64,
    rx: mpsc::UnboundedReceiver<Command>,
    engine: Option<Engine>,
    /// What the project shares from this device; handed to the engine once it exists.
    share: LocalShare,
    policy: ReconnectPolicy,
    /// A send on this connection failed (often: the relay's close frame was already read).
    /// Nothing more is sent, but the frames read before the close are still handled.
    writer_closed: bool,
    /// The team's lock file, held while this worker runs (closing it releases the lock).
    _lock: Option<std::fs::File>,
}

type Writer = futures_util::stream::SplitSink<crate::team_relay::Socket, Message>;

/// Why the worker leaves its loop for good.
enum Exit {
    /// Turned off or the app quits.
    Stopped,
    Ended(StopReason),
}

/// What wakes a live connection.
enum Wake {
    Command(Option<Command>),
    Frame(Option<Frame>),
    Due,
}

impl Worker {
    fn timing(&self) -> Timing {
        self.service.inner.options.timing
    }

    fn view(&self) {
        let view = match &self.engine {
            Some(engine) => View {
                status: engine.status(self.policy.retrying()),
                pending: engine.pending(),
                activity: engine.activity(),
                files: engine.teammate_files(),
            },
            None => View {
                status: TeamStatus::Connecting {
                    retrying: self.policy.retrying(),
                },
                pending: Vec::new(),
                activity: TeamActivity::default(),
                files: Vec::new(),
            },
        };
        self.service
            .set_view(&self.base, &self.project, self.run, view);
    }

    fn show(&self, status: TeamStatus) {
        self.service.set_view(
            &self.base,
            &self.project,
            self.run,
            View {
                status,
                pending: Vec::new(),
                activity: TeamActivity::default(),
                files: Vec::new(),
            },
        );
    }

    async fn run(mut self) {
        // The previous worker of this team (turned off and on again, signed out and in) lets go
        // first; it closes within moments of its stop. Past the wait, the lock file decides.
        let turn = self.service.turn(&self.base, &self.project);
        let _turn = tokio::time::timeout(HANDOVER, turn.lock_owned()).await.ok();
        let exit = match self.take_lock().await {
            Ok(()) => self.connect_loop().await,
            Err(exit) => exit,
        };
        match exit {
            Exit::Stopped => {}
            Exit::Ended(reason) => {
                self.service
                    .finish(&self.base, &self.project, self.run, reason.status());
            }
        }
    }

    /// Takes the team's lock file; while another window holds it, shows so and tries again.
    async fn take_lock(&mut self) -> Result<(), Exit> {
        let Some(dir) = self.service.inner.options.state_dir.clone() else {
            return Ok(());
        };
        let path = team::lock_path(&dir, self.service.service_name(), &self.key);
        loop {
            let opened = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| {
                    std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create(true)
                        .truncate(false)
                        .open(&path)
                });
            let file = match opened {
                Ok(file) => file,
                Err(error) => {
                    // Without the file the keychain and the relay still work; only the guard
                    // against a second window is missing.
                    eprintln!("hangar: team lock file not opened: {error}");
                    return Ok(());
                }
            };
            match file.try_lock() {
                Ok(()) => {
                    self._lock = Some(file);
                    return Ok(());
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    self.show(TeamStatus::NeedsAttention {
                        reason: AttentionReason::OtherInstance,
                    });
                    if self.wait(self.timing().lock_retry).await {
                        return Err(Exit::Stopped);
                    }
                }
                Err(std::fs::TryLockError::Error(error)) => {
                    eprintln!("hangar: team lock file not locked: {error}");
                    return Ok(());
                }
            }
        }
    }

    async fn connect_loop(&mut self) -> Exit {
        loop {
            self.view();
            let end = match self.connect_once().await {
                Ok(end) => end,
                Err(exit) => return exit,
            };
            let next = match self.policy.after(end, jitter()) {
                Next::Stop(StopReason::ProjectDeleted) => self.confirm_deleted().await,
                next => next,
            };
            match next {
                Next::Stop(reason) => return Exit::Ended(reason),
                Next::Reconnect { delay } => {
                    let delay = delay.mul_f64(self.timing().backoff_unit.as_secs_f64());
                    self.view();
                    if self.wait(delay).await {
                        return Exit::Stopped;
                    }
                }
            }
        }
    }

    /// The relay closed with "project deleted" (4404), which it may say falsely: the keys go
    /// only when the website no longer lists the project.
    async fn confirm_deleted(&mut self) -> Next {
        match self.website().await {
            Website::Gone => {
                self.retire().await;
                Next::Stop(StopReason::ProjectDeleted)
            }
            Website::Member(_) => {
                eprintln!("hangar: the relay called a project deleted that the website lists");
                Next::Stop(StopReason::Attention(AttentionReason::Refused))
            }
            Website::SignedOut => Next::Stop(StopReason::Attention(AttentionReason::SignedOut)),
            Website::Unreachable => self.policy.after(SessionEnd::Lost, jitter()),
        }
    }

    /// Waits `delay` between connections; `true` when told to stop meanwhile.
    async fn wait(&mut self, delay: Duration) -> bool {
        let until = tokio::time::Instant::now() + delay;
        loop {
            match tokio::time::timeout_at(until, self.rx.recv()).await {
                Err(_) => return false,
                Ok(None) => return true,
                Ok(Some(command)) => {
                    if self.refuse(command) {
                        return true;
                    }
                }
            }
        }
    }

    /// Answers a command while no connection runs; `true` for `Stop`. A new share is kept for
    /// the next connection.
    fn refuse(&mut self, command: Command) -> bool {
        match command {
            Command::Stop => true,
            Command::Share(share) => {
                if let Some(engine) = &mut self.engine {
                    // Not connected: the engine only keeps it and sends it after `welcome`.
                    let _ = engine.set_share(share.clone());
                }
                self.share = share;
                false
            }
            Command::Confirm(_, reply)
            | Command::Reject(_, reply)
            | Command::RemoveMember(_, reply)
            | Command::RemoveDevice(_, reply) => {
                let _ = reply.send(Err(TeamError::Busy));
                false
            }
        }
    }

    async fn wipe(&self) {
        let device = device_account(&self.key);
        let keys = keys_account(&self.key);
        let wiped = self
            .service
            .keychain(move |store| {
                store.delete(&device)?;
                store.delete(&keys)
            })
            .await;
        if let Err(error) = wiped {
            eprintln!("hangar: team keys not removed: {error}");
        }
        if let Some(dir) = &self.service.inner.options.state_dir {
            LocalState::remove(dir, self.service.service_name(), &self.key);
        }
    }

    /// The team is over for this device: its keys and state go, and sharing turns off, so the
    /// next start does not register a new identity unasked.
    async fn retire(&mut self) {
        self.wipe().await;
        self.engine = None;
        if let Err(error) = self.service.set_enabled(&self.base, &self.project, false) {
            eprintln!("hangar: team sharing not turned off: {error}");
        }
    }

    /// The person and the project on the website.
    async fn website(&self) -> Website {
        match self.service.inner.account.projects().await {
            Ok(projects) => projects
                .iter()
                .find(|p| p.id == self.project)
                .map_or(Website::Gone, |p| Website::Member(map_role(p.role))),
            Err(AccountError::NotSignedIn | AccountError::SessionEnded) => Website::SignedOut,
            Err(error) => {
                eprintln!("hangar: team project list not read: {error}");
                Website::Unreachable
            }
        }
    }

    /// The engine, with this device's identity from the keychain (created on first use).
    async fn ensure_engine(&mut self, role: Role) -> Result<(), SessionEnd> {
        if let Some(engine) = &mut self.engine {
            // Not synced on a new connection yet, so this only notes the role.
            let _ = engine.set_website_role(role);
            return Ok(());
        }
        let keychain = attention(AttentionReason::Keychain);
        let device_name = device_account(&self.key);
        let keys_name = keys_account(&self.key);
        let (stored, keys) = self
            .service
            .keychain(move |store| Ok((store.get(&device_name)?, store.get(&keys_name)?)))
            .await
            .map_err(|error| {
                eprintln!("hangar: team keys not read: {error}");
                keychain
            })?;
        let existing = stored.is_some();
        let (device, member, identity) = match stored {
            Some(text) => {
                // An entry that is there but unreadable stops: replacing it would make this a
                // new device and drop the old one's place in the team unasked.
                let Some(stored) = StoredDevice::decode(&text) else {
                    eprintln!("hangar: team device entry unreadable");
                    return Err(keychain);
                };
                let identity = DeviceIdentity::import(&stored.secret).map_err(|_| keychain)?;
                let member = match stored.member {
                    Some(member) => Some(MemberId::new(member).map_err(|_| keychain)?),
                    None => None,
                };
                (stored.device, member, identity)
            }
            None => {
                // First use on this server and project: a new device. Stored before it is used,
                // so the relay never sees a device this one forgets.
                let identity = DeviceIdentity::generate(&mut OsEntropy).map_err(|_| keychain)?;
                let device =
                    team::new_device_id(|bytes| OsEntropy.fill(bytes)).map_err(|_| keychain)?;
                let text = StoredDevice {
                    device: device.clone(),
                    member: None,
                    secret: identity.export(),
                }
                .encode();
                let name = device_account(&self.key);
                // A new device starts without old secrets or a head.
                let keys_name = keys_account(&self.key);
                self.service
                    .keychain(move |store| {
                        store.set(&name, &text)?;
                        store.delete(&keys_name)
                    })
                    .await
                    .map_err(|error| {
                        eprintln!("hangar: team device not stored: {error}");
                        keychain
                    })?;
                (device, None, identity)
            }
        };
        // An unreadable keys entry stops instead of starting over: dropping it would forget the
        // verified log head, and a relay could then rewrite the history unnoticed.
        let keys = match (existing, keys.as_deref()) {
            (true, Some(text)) => StoredKeys::decode(text).ok_or_else(|| {
                eprintln!("hangar: team keys entry unreadable");
                keychain
            })?,
            _ => StoredKeys::default(),
        };
        let local = match (existing, &self.service.inner.options.state_dir) {
            (true, Some(dir)) => LocalState::load(dir, self.service.service_name(), &self.key)
                .map_err(|error| {
                    eprintln!("hangar: team state not read: {error}");
                    attention(AttentionReason::Storage)
                })?,
            _ => LocalState::default(),
        };
        let team =
            TeamId::new(self.project.clone()).map_err(|_| attention(AttentionReason::Refused))?;
        let device = DeviceId::new(device).map_err(|_| keychain)?;
        let mut engine = Engine::new(EngineSetup {
            project: self.project.clone(),
            team,
            device,
            identity,
            member,
            website_role: role,
            keys,
            local,
            now: Box::new(crate::updates::now_millis),
            entropy: Box::new(OsEntropy),
        });
        let _ = engine.set_share(self.share.clone());
        self.engine = Some(engine);
        Ok(())
    }

    /// The device acts for another account than the one signed in now (two people on one
    /// keychain): known before connecting when the account already read who is signed in.
    fn other_account(&self) -> bool {
        let Some(bound) = self.engine.as_ref().and_then(Engine::member) else {
            return false;
        };
        match self.service.inner.account.view().state {
            AccountState::SignedIn { user: Some(me) } => me.id != bound.as_str(),
            _ => false,
        }
    }

    async fn ticket(&self) -> Result<account::RelayTicket, SessionEnd> {
        self.service
            .inner
            .account
            .relay_ticket(&self.project)
            .await
            .map_err(|error| match error {
                AccountError::NotSignedIn | AccountError::SessionEnded => {
                    attention(AttentionReason::SignedOut)
                }
                AccountError::BadResponse => attention(AttentionReason::Refused),
                _ => SessionEnd::Lost,
            })
    }

    /// One connection from ticket to close. `Err` leaves the worker (turned off).
    async fn connect_once(&mut self) -> Result<SessionEnd, Exit> {
        let role = match self.website().await {
            Website::Member(role) => role,
            // Not listed: the keys stay, the website may list the project again.
            Website::Gone => return Ok(SessionEnd::Stop(StopReason::Removed)),
            Website::SignedOut => return Ok(attention(AttentionReason::SignedOut)),
            Website::Unreachable => return Ok(SessionEnd::Lost),
        };
        if let Err(end) = self.ensure_engine(role).await {
            return Ok(end);
        }
        if self.other_account() {
            return Ok(attention(AttentionReason::OtherAccount));
        }
        let ticket = match self.ticket().await {
            Ok(ticket) => ticket,
            Err(end) => return Ok(end),
        };
        let Ok(hello_ticket) = Ticket::new(ticket.ticket.as_str()) else {
            return Ok(attention(AttentionReason::Refused));
        };
        let timing = self.timing();
        let socket =
            match crate::team_relay::connect(&ticket.relay_url, timing.connect_timeout).await {
                Ok(socket) => socket,
                Err(error) => {
                    eprintln!("hangar: team relay not reached: {error}");
                    return Ok(SessionEnd::Lost);
                }
            };
        drop(ticket);
        let (mut writer, mut reader) = socket.split();
        // Bounded: a relay sending faster than the worker handles frames waits on its socket.
        let (frame_tx, mut frames) = mpsc::channel(READ_QUEUE);
        let reading = tauri::async_runtime::spawn(async move {
            let mut code = None;
            while let Some(message) = reader.next().await {
                match message {
                    Ok(Message::Text(text)) => {
                        if frame_tx
                            .send(Frame::Text(text.as_str().to_owned()))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Ok(Message::Close(frame)) => {
                        code = frame.map(|f| u16::from(f.code));
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            let _ = frame_tx.send(Frame::Closed(code)).await;
        });
        let opened = Instant::now();
        let now = || u64::try_from(opened.elapsed().as_millis()).unwrap_or(u64::MAX);
        let mut clock = ConnectionClock::new(timing, now());
        let effects = match &mut self.engine {
            Some(engine) => engine.hello(hello_ticket),
            None => return Ok(SessionEnd::Lost),
        };
        self.writer_closed = false;
        let mut result = self.apply(&mut writer, effects).await;
        while result.is_none() {
            let wake =
                tokio::time::Instant::from_std(opened + Duration::from_millis(clock.next_wake()));
            let event = {
                let command = std::pin::pin!(self.rx.recv());
                let frame = std::pin::pin!(frames.recv());
                let timer = std::pin::pin!(tokio::time::sleep_until(wake));
                match select(select(command, frame), timer).await {
                    Either::Left((Either::Left((command, _)), _)) => Wake::Command(command),
                    Either::Left((Either::Right((frame, _)), _)) => Wake::Frame(frame),
                    Either::Right(_) => Wake::Due,
                }
            };
            result = match event {
                Wake::Command(None) => Some(Err(Exit::Stopped)),
                Wake::Command(Some(command)) => self.on_command(&mut writer, command).await,
                Wake::Frame(Some(Frame::Text(text))) => {
                    clock.heard(now());
                    let effects = self
                        .engine
                        .as_mut()
                        .map(|e| e.on_text(&text))
                        .unwrap_or_default();
                    self.apply(&mut writer, effects).await
                }
                Wake::Frame(Some(Frame::Closed(code))) => Some(Ok(SessionEnd::Closed(code))),
                Wake::Frame(None) => Some(Ok(SessionEnd::Lost)),
                Wake::Due => self.on_due(&mut writer, clock.due(now())).await,
            };
        }
        if opened.elapsed() >= timing.stable_after {
            // Long enough to count as settled even if the log check never finished.
            self.policy.settled();
        }
        // Closing: a close frame (1000) when Hangar ends the connection itself.
        let closing = CloseFrame {
            code: CloseCode::Normal,
            reason: "".into(),
        };
        let _ = tokio::time::timeout(
            Duration::from_secs(1),
            writer.send(Message::Close(Some(closing))),
        )
        .await;
        reading.abort();
        match result {
            Some(result) => result,
            None => Ok(SessionEnd::Lost),
        }
    }

    async fn on_command(
        &mut self,
        writer: &mut Writer,
        command: Command,
    ) -> Option<Result<SessionEnd, Exit>> {
        if self.engine.is_none() {
            return self.refuse(command).then_some(Err(Exit::Stopped));
        }
        let engine = self.engine.as_mut()?;
        let (effects, reply) = match command {
            Command::Stop => return Some(Err(Exit::Stopped)),
            Command::Share(share) => {
                self.share = share.clone();
                let effects = engine.set_share(share);
                return self.apply(writer, effects).await;
            }
            Command::Confirm(id, reply) => (engine.confirm(&id), reply),
            Command::Reject(id, reply) => (engine.reject(&id), reply),
            Command::RemoveMember(member, reply) => (engine.remove_member(&member), reply),
            Command::RemoveDevice(holder, reply) => (engine.remove_device(&holder), reply),
        };
        match effects {
            Ok(effects) => {
                let result = self.apply(writer, effects).await;
                let _ = reply.send(Ok(()));
                result
            }
            Err(error) => {
                let _ = reply.send(Err(error));
                None
            }
        }
    }

    async fn on_due(
        &mut self,
        writer: &mut Writer,
        due: Vec<Due>,
    ) -> Option<Result<SessionEnd, Exit>> {
        for due in due {
            let effects = match due {
                Due::Silent => return Some(Ok(SessionEnd::Lost)),
                Due::MaxAge => return Some(Ok(SessionEnd::MaxAge)),
                Due::Renew => match self.ticket().await {
                    Ok(ticket) => match (Ticket::new(ticket.ticket.as_str()), &mut self.engine) {
                        (Ok(fresh), Some(engine)) => engine.renew(fresh),
                        _ => Vec::new(),
                    },
                    Err(SessionEnd::Lost) => Vec::new(),
                    Err(end) => return Some(Ok(end)),
                },
                Due::Ping => self.engine.as_mut().map(Engine::ping).unwrap_or_default(),
                Due::Devices => self
                    .engine
                    .as_mut()
                    .map(Engine::poll_devices)
                    .unwrap_or_default(),
            };
            if let Some(result) = self.apply(writer, effects).await {
                return Some(result);
            }
        }
        None
    }

    /// Carries out the engine's effects in order. A failed keychain or state file write ends the
    /// connection before anything after it happens; after a failed send nothing is sent any
    /// more, and the connection ends with the close the reader delivers.
    async fn apply(
        &mut self,
        writer: &mut Writer,
        effects: Vec<Effect>,
    ) -> Option<Result<SessionEnd, Exit>> {
        let mut queue: VecDeque<Effect> = effects.into();
        let mut changed = false;
        let mut result = None;
        while let Some(effect) = queue.pop_front() {
            match effect {
                Effect::Send(_) if self.writer_closed => {}
                Effect::Send(frame) => {
                    let text = match encode_client(&frame) {
                        Ok(text) => text,
                        Err(error) => {
                            eprintln!("hangar: team frame not encoded: {error}");
                            continue;
                        }
                    };
                    let sent = tokio::time::timeout(
                        self.timing().connect_timeout,
                        writer.send(Message::text(text)),
                    )
                    .await;
                    if !matches!(sent, Ok(Ok(()))) {
                        // The reader delivers the close (or the broken socket) next; until then
                        // the frames already read still count, a removal in particular.
                        self.writer_closed = true;
                    }
                }
                Effect::StoreKeys => {
                    let Some(engine) = &self.engine else { continue };
                    let text: Zeroizing<String> = engine.keys().encode();
                    let name = keys_account(&self.key);
                    let stored = self
                        .service
                        .keychain(move |store| store.set(&name, &text))
                        .await;
                    if let Err(error) = stored {
                        eprintln!("hangar: team keys not stored: {error}");
                        result = Some(Ok(attention(AttentionReason::Keychain)));
                        break;
                    }
                }
                Effect::StoreDevice => {
                    let Some(engine) = &self.engine else { continue };
                    let text = engine.stored_device().encode();
                    let name = device_account(&self.key);
                    let stored = self
                        .service
                        .keychain(move |store| store.set(&name, &text))
                        .await;
                    if let Err(error) = stored {
                        eprintln!("hangar: team device not stored: {error}");
                        result = Some(Ok(attention(AttentionReason::Keychain)));
                        break;
                    }
                }
                Effect::StoreLocal => {
                    if let (Some(engine), Some(dir)) =
                        (&self.engine, &self.service.inner.options.state_dir)
                    {
                        // Like the keychain: nothing after a failed write happens, so a record
                        // never leaves with a counter that was not stored first.
                        let saved =
                            engine
                                .local()
                                .save(dir, self.service.service_name(), &self.key);
                        if let Err(error) = saved {
                            eprintln!("hangar: team state not saved: {error}");
                            result = Some(Ok(SessionEnd::Lost));
                            break;
                        }
                    }
                }
                Effect::WipeDevice => self.retire().await,
                Effect::CheckWebsite => match self.website().await {
                    Website::Member(role) => {
                        let more = self
                            .engine
                            .as_mut()
                            .map(|engine| engine.set_website_role(role))
                            .unwrap_or_default();
                        for effect in more.into_iter().rev() {
                            queue.push_front(effect);
                        }
                    }
                    Website::Gone => {
                        self.retire().await;
                        result = Some(Ok(SessionEnd::Stop(StopReason::Removed)));
                        break;
                    }
                    Website::SignedOut => {
                        result = Some(Ok(attention(AttentionReason::SignedOut)));
                        break;
                    }
                    // The log sync that came with the event decides on its own.
                    Website::Unreachable => {}
                },
                Effect::Settled => {
                    self.policy.settled();
                    changed = true;
                }
                Effect::Changed => changed = true,
                Effect::End(end) => {
                    result = Some(Ok(end));
                    break;
                }
            }
        }
        if changed {
            self.view();
        }
        result
    }
}

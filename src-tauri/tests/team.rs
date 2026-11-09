//! Team connections end to end: `TeamService` workers with real WebSockets against a small relay
//! on 127.0.0.1 built from the protocol and crypto crates (`EventLog`, `LogVerifier::admit`,
//! `admit_rotation`, `admit_device_remove`, `relay::check_wrap`, `check_registered_keys`), and the
//! account service against a mock of the account API (as in `tests/account.rs`). Nothing here
//! talks to a real server or touches the OS keychain: both keychains are in-memory stores. The
//! relay also passes presence on and compares claims with the protocol crate's `ClaimBoard`.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use futures_util::{SinkExt as _, StreamExt as _};
use hangar_lib::account::{AccountView, BaseUrl};
use hangar_lib::account_service::{AccountService, Options as AccountOptions};
use hangar_lib::claims::{ChangeKind, Claim, LineRange, OverlapLevel};
use hangar_lib::secrets::{SecretError, SecretStore};
use hangar_lib::team::{
    self, PendingKind, StoredDevice, StoredKeys, TeamConfig, TeamError, TeamSnapshot, TeamStatus,
    Timing,
};
use hangar_lib::team_service::{Options, TeamService};
use hangar_lib::team_share::{LocalSession, SessionState};
use hangar_team_crypto::identity::DevicePublicKeys;
use hangar_team_crypto::memberlog::LogVerifier;
use hangar_team_protocol::e2e::ChainHead;
use hangar_team_protocol::ids::{
    Ciphertext, DeviceId, Epoch, MemberId, Sealed, StreamId, TeamId, UnixMillis,
};
use hangar_team_protocol::log::{EventLog, Replay};
use hangar_team_protocol::overlap::{ClaimBoard, OverlapNotice, SessionClaims};
use hangar_team_protocol::wire::{
    decode_client, encode_server, Ack, ClientMessage, Cursor, DeviceInfo, DeviceList,
    DeviceRemoved, ErrorBody, ErrorCode, Holder, KeyRotated, KeyWrapped, MemberLogAppend,
    MemberLogAppended, MemberLogEntries, MemberLogEntry, MemberRemoved, MemberRoleChanged,
    PresenceState, PresenceUpdate, ResyncRequired, Role, ServerFrame, ServerMessage, SessionRef,
    Welcome,
};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;
use zeroize::Zeroizing;

const PROJECT: &str = "p1";

fn guard<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ------------------------------------------------------------------------------------------
// In-memory keychain and test folders
// ------------------------------------------------------------------------------------------

#[derive(Default)]
struct MemoryStore {
    values: Mutex<HashMap<String, Zeroizing<String>>>,
    /// Refuses to write the keys entries (epoch secrets and log head).
    refuse_keys: AtomicBool,
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
        if account.starts_with("keys/") && self.refuse_keys.load(Ordering::SeqCst) {
            return Err(SecretError::Failed("refused by the test".to_owned()));
        }
        guard(&self.values).insert(account.to_owned(), Zeroizing::new(value.to_owned()));
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), SecretError> {
        guard(&self.values).remove(account);
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
            .join(format!("team-{name}-{}", std::process::id()));
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
// Mock of the account API: tokens per user, projects with roles, relay tickets
// ------------------------------------------------------------------------------------------

#[derive(Default)]
struct AccountMock {
    roles: BTreeMap<String, &'static str>,
    relay_url: String,
    tickets: u32,
    /// Who confirms the next device flow on the website.
    device_user: String,
    origin: String,
}

#[derive(Clone)]
struct Website {
    origin: String,
    state: Arc<Mutex<AccountMock>>,
}

impl Website {
    fn start(relay_port: u16) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(AccountMock {
            relay_url: format!("ws://127.0.0.1:{relay_port}/"),
            origin: origin.clone(),
            ..AccountMock::default()
        }));
        let shared = Arc::clone(&state);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let state = Arc::clone(&shared);
                std::thread::spawn(move || serve(stream, &state));
            }
        });
        Self { origin, state }
    }

    fn base(&self) -> BaseUrl {
        BaseUrl::parse(&format!("{}/api/v1", self.origin)).unwrap()
    }

    fn tickets(&self) -> u32 {
        guard(&self.state).tickets
    }
}

fn read_request(
    stream: &mut TcpStream,
) -> Option<(String, String, Option<String>, serde_json::Value)> {
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
    Some((method, path, bearer, body))
}

fn respond(stream: &mut TcpStream, status: u16, body: &serde_json::Value) {
    let body = body.to_string();
    let text = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(text.as_bytes());
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Write);
}

/// Tokens name their user: `hgrt_<user>_<n>`, `hgat_<user>_<n>`.
fn user_of(token: &str) -> Option<String> {
    token.split('_').nth(1).map(str::to_owned)
}

fn serve(mut stream: TcpStream, state: &Mutex<AccountMock>) {
    let Some((method, path, bearer, body)) = read_request(&mut stream) else {
        return;
    };
    let user = bearer.as_deref().and_then(user_of);
    match (method.as_str(), path.as_str()) {
        ("POST", "/api/v1/oauth/device") => {
            let origin = guard(state).origin.clone();
            respond(
                &mut stream,
                200,
                &serde_json::json!({
                    "device_code": "mock-device-code",
                    "user_code": "BCDF-GHJK",
                    "verification_uri": format!("{origin}/device"),
                    "expires_in": 600,
                    "interval": 1,
                }),
            );
        }
        ("POST", "/api/v1/oauth/token")
            if body["grant_type"] == "urn:ietf:params:oauth:grant-type:device_code" =>
        {
            let user = guard(state).device_user.clone();
            respond(
                &mut stream,
                200,
                &serde_json::json!({
                    "access_token": format!("hgat_{user}_d"),
                    "token_type": "Bearer",
                    "expires_in": 900,
                    "refresh_token": format!("hgrt_{user}_d"),
                }),
            );
        }
        ("POST", "/api/v1/oauth/token") => {
            let user = body["refresh_token"]
                .as_str()
                .and_then(user_of)
                .unwrap_or_default();
            let n = guard(state).tickets;
            respond(
                &mut stream,
                200,
                &serde_json::json!({
                    "access_token": format!("hgat_{user}_{n}"),
                    "token_type": "Bearer",
                    "expires_in": 900,
                    "refresh_token": format!("hgrt_{user}_{n}"),
                }),
            );
        }
        ("GET", "/api/v1/me") => {
            let user = user.unwrap_or_default();
            respond(
                &mut stream,
                200,
                &serde_json::json!({"id": user, "email": format!("{user}@example.test")}),
            );
        }
        ("GET", "/api/v1/projects") => {
            let role = user.and_then(|u| guard(state).roles.get(&u).copied());
            let projects: Vec<_> = role
                .map(|role| serde_json::json!({"id": PROJECT, "name": "Project", "role": role}))
                .into_iter()
                .collect();
            respond(
                &mut stream,
                200,
                &serde_json::json!({ "projects": projects }),
            );
        }
        ("POST", "/api/v1/relay-ticket") => {
            let mut mock = guard(state);
            let role = user.as_ref().and_then(|u| mock.roles.get(u).copied());
            match (user, role) {
                (Some(user), Some(role)) => {
                    mock.tickets += 1;
                    let ticket = format!("tk.{user}.{role}.{}", mock.tickets);
                    let url = mock.relay_url.clone();
                    drop(mock);
                    respond(
                        &mut stream,
                        200,
                        &serde_json::json!({"ticket": ticket, "expires_at": 1, "relay_url": url}),
                    );
                }
                _ => respond(&mut stream, 404, &serde_json::json!({"code": "not_found"})),
            }
        }
        _ => respond(&mut stream, 404, &serde_json::json!({})),
    }
}

// ------------------------------------------------------------------------------------------
// The relay
// ------------------------------------------------------------------------------------------

struct Conn {
    holder: Holder,
    out: mpsc::UnboundedSender<Message>,
}

struct RelayState {
    team: TeamId,
    stream: StreamId,
    log: EventLog<ServerMessage>,
    verifier: LogVerifier,
    entries: Vec<MemberLogEntry>,
    devices: BTreeMap<Holder, DeviceInfo>,
    /// (target, epoch, sender, data)
    wrapped: Vec<(Holder, Epoch, Holder, Ciphertext)>,
    epoch: Epoch,
    conns: BTreeMap<u64, Conn>,
    next_conn: u64,
    /// The next hellos answered with `err forbidden` (a stale ticket).
    refuse_hellos: u32,
    /// Every accepted hello: holder, its cursor, whether it got a resync.
    hellos: Vec<(Holder, Option<Cursor>, bool)>,
    renewals: u32,
    /// `key.rotate` frames received, admitted or not.
    rotates: u32,
    /// The newest presence per device, for devices that connect later.
    presence: BTreeMap<Holder, ServerMessage>,
    board: ClaimBoard,
    /// `claim.set` frames received.
    claim_sets: u32,
}

#[derive(Clone)]
struct Relay {
    port: u16,
    state: Arc<Mutex<RelayState>>,
}

fn text(
    id: Option<hangar_team_protocol::ids::OpId>,
    seq: Option<u64>,
    team: &TeamId,
    message: ServerMessage,
) -> Message {
    let frame = ServerFrame {
        id,
        seq: seq.map(hangar_team_protocol::ids::Seq),
        team: Some(team.clone()),
        message,
    };
    Message::text(encode_server(&frame).unwrap())
}

fn ack(keys: Option<Vec<Sealed>>) -> ServerMessage {
    ServerMessage::Ack(Ack {
        blob: None,
        content: None,
        keys,
    })
}

fn err(code: ErrorCode) -> ServerMessage {
    ServerMessage::Err(ErrorBody {
        code,
        retry_after_ms: None,
        holder: None,
    })
}

impl RelayState {
    /// `key.rotated` carries only the receiving device's own wrapped key.
    fn for_holder(&self, message: &ServerMessage, holder: &Holder) -> ServerMessage {
        match message {
            ServerMessage::KeyRotated(rotated) => ServerMessage::KeyRotated(KeyRotated {
                epoch: rotated.epoch,
                wrapped: self
                    .wrapped
                    .iter()
                    .find(|w| w.0 == *holder && w.1 == rotated.epoch)
                    .map(|w| w.3.clone()),
            }),
            other => other.clone(),
        }
    }

    fn log_event(&mut self, message: ServerMessage) {
        let seq = self.log.push(message.clone());
        for conn in self.conns.values() {
            let own = self.for_holder(&message, &conn.holder);
            let _ = conn.out.send(text(None, Some(seq.0), &self.team, own));
        }
    }

    fn close(&self, holder: &Holder, code: u16) {
        for conn in self.conns.values().filter(|c| c.holder == *holder) {
            let _ = conn.out.send(Message::Close(Some(CloseFrame {
                code: CloseCode::from(code),
                reason: "".into(),
            })));
        }
    }

    fn store(&mut self, append: &MemberLogAppend, head: ChainHead, by: &Holder) {
        self.entries.push(MemberLogEntry {
            index: append.index,
            hash: head.hash,
            entry: append.entry.clone(),
            by: by.clone(),
            at: UnixMillis(1),
        });
        self.log_event(ServerMessage::MemberLogAppended(MemberLogAppended {
            index: head.index,
            hash: head.hash,
            by: by.clone(),
        }));
    }

    /// Live state (never logged) to the connections of everyone but `from`.
    fn others(&self, from: &Holder, message: &ServerMessage) {
        for conn in self.conns.values().filter(|c| c.holder != *from) {
            let _ = conn.out.send(text(None, None, &self.team, message.clone()));
        }
    }

    fn deliver(&self, notices: Vec<OverlapNotice>) {
        for notice in notices {
            let message = ServerMessage::ClaimOverlap(notice.overlap);
            for conn in self.conns.values().filter(|c| c.holder == notice.to.holder) {
                let _ = conn.out.send(text(None, None, &self.team, message.clone()));
            }
        }
    }

    /// A device's last connection closed: it is offline and its claims end.
    fn gone(&mut self, holder: &Holder) {
        if self.conns.values().any(|c| c.holder == *holder) {
            return;
        }
        self.presence.remove(holder);
        let offline = ServerMessage::PresenceUpdate(PresenceUpdate {
            holder: holder.clone(),
            state: PresenceState::Offline,
            status: None,
            at: UnixMillis(2),
        });
        self.others(holder, &offline);
        let notices = self.board.clear_device(holder);
        self.deliver(notices);
    }

    fn removed(&mut self, holders: &[Holder]) {
        for holder in holders {
            self.devices.remove(holder);
            self.close(holder, hangar_team_protocol::close::CLOSE_REMOVED);
        }
    }
}

impl Relay {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let team = TeamId::new(PROJECT).unwrap();
        let state = Arc::new(Mutex::new(RelayState {
            verifier: LogVerifier::new(team.clone()),
            team,
            stream: StreamId::new("stream-1").unwrap(),
            log: EventLog::new(StreamId::new("stream-1").unwrap(), 1000),
            entries: Vec::new(),
            devices: BTreeMap::new(),
            wrapped: Vec::new(),
            epoch: Epoch(0),
            conns: BTreeMap::new(),
            next_conn: 0,
            refuse_hellos: 0,
            hellos: Vec::new(),
            renewals: 0,
            rotates: 0,
            presence: BTreeMap::new(),
            board: ClaimBoard::new(),
            claim_sets: 0,
        }));
        let shared = Arc::clone(&state);
        tauri::async_runtime::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let state = Arc::clone(&shared);
                tauri::async_runtime::spawn(async move { connection(stream, state).await });
            }
        });
        Self { port, state }
    }

    fn state(&self) -> MutexGuard<'_, RelayState> {
        guard(&self.state)
    }

    fn close(&self, member: &str, device: &str, code: u16) {
        let holder = Holder {
            member: MemberId::new(member).unwrap(),
            device: DeviceId::new(device).unwrap(),
        };
        self.state().close(&holder, code);
    }

    /// An event to everyone, in the event log, as the relay would send it.
    fn broadcast(&self, message: ServerMessage) {
        self.state().log_event(message);
    }

    /// The relay forgets the team: a new membership log can start under the same id.
    fn forget(&self) {
        let mut state = self.state();
        let team = state.team.clone();
        state.verifier = LogVerifier::new(team);
        state.entries.clear();
        state.devices.clear();
        state.wrapped.clear();
        state.epoch = Epoch(0);
    }
}

async fn connection(stream: tokio::net::TcpStream, state: Arc<Mutex<RelayState>>) {
    let Ok(socket) = tokio_tungstenite::accept_async(stream).await else {
        return;
    };
    let (mut sink, mut source) = socket.split();
    let (out, mut outgoing) = mpsc::unbounded_channel::<Message>();
    tauri::async_runtime::spawn(async move {
        while let Some(message) = outgoing.recv().await {
            let close = matches!(message, Message::Close(_));
            if sink.send(message).await.is_err() || close {
                break;
            }
        }
    });
    let id = {
        let mut state = guard(&state);
        state.next_conn += 1;
        state.next_conn
    };
    let mut bound: Option<(Holder, Role)> = None;
    while let Some(Ok(message)) = source.next().await {
        let Message::Text(body) = message else {
            if matches!(message, Message::Close(_)) {
                break;
            }
            continue;
        };
        let Ok(frame) = decode_client(body.as_str()) else {
            continue;
        };
        let mut state = guard(&state);
        handle(&mut state, id, &out, &mut bound, frame);
    }
    let mut state = guard(&state);
    if let Some(conn) = state.conns.remove(&id) {
        state.gone(&conn.holder);
    }
}

fn ticket_of(ticket: &str) -> Option<(String, Role)> {
    let mut parts = ticket.split('.');
    let _ = parts.next()?;
    let user = parts.next()?.to_owned();
    let role = match parts.next()? {
        "owner" => Role::Owner,
        "admin" => Role::Admin,
        _ => Role::Member,
    };
    Some((user, role))
}

fn handle(
    state: &mut RelayState,
    conn: u64,
    out: &mpsc::UnboundedSender<Message>,
    bound: &mut Option<(Holder, Role)>,
    frame: hangar_team_protocol::wire::ClientFrame,
) {
    let team = state.team.clone();
    let id = Some(frame.id.clone());
    let reply = |message: ServerMessage| {
        let _ = out.send(text(id.clone(), None, &team, message));
    };
    if let ClientMessage::Hello(hello) = &frame.message {
        if state.refuse_hellos > 0 {
            state.refuse_hellos -= 1;
            reply(err(ErrorCode::Forbidden));
            return;
        }
        let Some((user, role)) = ticket_of(hello.ticket.as_str()) else {
            reply(err(ErrorCode::Forbidden));
            return;
        };
        let holder = Holder {
            member: MemberId::new(user).unwrap(),
            device: hello.device.clone(),
        };
        *bound = Some((holder.clone(), role));
        state.conns.insert(
            conn,
            Conn {
                holder: holder.clone(),
                out: out.clone(),
            },
        );
        let welcome = ServerMessage::Welcome(Welcome {
            proto: 1,
            stream: state.stream.clone(),
            head: state.log.head(),
            member: holder.member.clone(),
            epoch: state.epoch,
        });
        let replay: Result<Vec<(u64, ServerMessage)>, _> =
            match state.log.replay(hello.last_seq.as_ref()) {
                Replay::Events(events) => Ok(events
                    .into_iter()
                    .map(|(seq, event)| (seq.0, state.for_holder(event, &holder)))
                    .collect()),
                Replay::Resync(reason) => Err(reason),
            };
        // The others' presence, as the relay sends it to every new connection.
        let presence: Vec<ServerMessage> = state
            .presence
            .iter()
            .filter(|(from, _)| **from != holder)
            .map(|(_, update)| update.clone())
            .collect();
        match replay {
            Ok(events) => {
                state.hellos.push((holder, hello.last_seq.clone(), false));
                reply(welcome);
                for (seq, event) in events {
                    let _ = out.send(text(None, Some(seq), &team, event));
                }
                for update in presence {
                    let _ = out.send(text(None, None, &team, update));
                }
            }
            Err(reason) => {
                state.hellos.push((holder, hello.last_seq.clone(), true));
                reply(ServerMessage::ResyncRequired(ResyncRequired {
                    stream: state.stream.clone(),
                    head: state.log.head(),
                    reason,
                }));
                reply(welcome);
                for update in presence {
                    let _ = out.send(text(None, None, &team, update));
                }
            }
        }
        return;
    }
    if matches!(frame.message, ClientMessage::Ping) {
        reply(ack(None));
        return;
    }
    let Some((sender, role)) = bound.clone() else {
        reply(err(ErrorCode::BadRequest));
        return;
    };
    match frame.message {
        ClientMessage::TicketRenew(_) => {
            state.renewals += 1;
            reply(ack(None));
        }
        ClientMessage::DeviceKeys(keys) => {
            state.devices.insert(
                sender.clone(),
                DeviceInfo {
                    holder: sender.clone(),
                    sign: keys.sign,
                    kem: keys.kem,
                    proof: keys.proof,
                    at: UnixMillis(1),
                },
            );
            reply(ack(None));
        }
        ClientMessage::DevicesGet => reply(ServerMessage::Devices(DeviceList {
            devices: state.devices.values().cloned().collect(),
        })),
        ClientMessage::MemberLogGet(get) => {
            let entries: Vec<_> = state
                .entries
                .iter()
                .filter(|e| e.index > get.after)
                .cloned()
                .collect();
            reply(ServerMessage::MemberLogEntries(MemberLogEntries::page(
                entries,
            )));
        }
        ClientMessage::MemberLogAppend(append) => {
            let devices = state.devices.clone();
            let registered = |holder: &Holder| {
                devices.get(holder).map(|d| DevicePublicKeys {
                    sign: d.sign,
                    kem: d.kem,
                })
            };
            let admitted = hangar_team_crypto::relay::check_registered_keys(&append, registered)
                .and_then(|()| state.verifier.admit(&append, &sender, role));
            match admitted {
                Ok(admitted) => {
                    reply(ack(None));
                    state.store(&append, admitted.head, &sender);
                    if let Some(first) = admitted.removed.first() {
                        let member = first.member.clone();
                        state.log_event(ServerMessage::MemberRemoved(MemberRemoved { member }));
                        state.removed(&admitted.removed);
                    }
                }
                Err(error) => reply(err(error.code())),
            }
        }
        ClientMessage::KeyRotate(rotate) => {
            state.rotates += 1;
            let checked = hangar_team_protocol::e2e::check_rotate(role, state.epoch, &rotate)
                .map_err(|e| e.code())
                .and_then(|()| {
                    state
                        .verifier
                        .admit_rotation(&rotate, &sender, role)
                        .map_err(|e| e.code())
                });
            match checked {
                Ok(admitted) => {
                    for wrapped in &rotate.wrapped {
                        state.wrapped.push((
                            wrapped.holder.clone(),
                            rotate.epoch,
                            sender.clone(),
                            wrapped.data.clone(),
                        ));
                    }
                    state.epoch = rotate.epoch;
                    reply(ack(None));
                    state.store(&rotate.log, admitted.head, &sender);
                    state.log_event(ServerMessage::KeyRotated(KeyRotated {
                        epoch: rotate.epoch,
                        wrapped: None,
                    }));
                }
                Err(code) => reply(err(code)),
            }
        }
        ClientMessage::KeyWrap(wrap) => {
            if wrap.epoch != state.epoch {
                reply(err(ErrorCode::StaleEpoch));
                return;
            }
            let from: Vec<Holder> = state
                .wrapped
                .iter()
                .filter(|w| w.0 == wrap.wrapped.holder && w.1 == wrap.epoch)
                .map(|w| w.2.clone())
                .collect();
            let Some(verified) = state.verifier.state() else {
                reply(err(ErrorCode::BadRequest));
                return;
            };
            match hangar_team_crypto::relay::check_wrap(verified, role, &sender, &wrap, &from) {
                Ok(()) => {
                    let target = wrap.wrapped.holder.clone();
                    state
                        .wrapped
                        .push((target.clone(), wrap.epoch, sender, wrap.wrapped.data));
                    reply(ack(None));
                    let note = ServerMessage::KeyWrapped(KeyWrapped {
                        epoch: wrap.epoch,
                        holder: target.clone(),
                    });
                    for conn in state.conns.values().filter(|c| c.holder == target) {
                        let _ = conn.out.send(text(None, None, &team, note.clone()));
                    }
                }
                Err(error) => reply(err(error.code())),
            }
        }
        ClientMessage::KeyGet(get) => {
            let keys: Vec<Sealed> = state
                .wrapped
                .iter()
                .filter(|w| w.0 == sender && w.1 == get.epoch)
                .map(|w| Sealed {
                    epoch: get.epoch,
                    data: w.3.clone(),
                })
                .collect();
            if keys.is_empty() {
                reply(err(ErrorCode::NotFound));
            } else {
                reply(ack(Some(keys)));
            }
        }
        ClientMessage::DeviceRemove(remove) => {
            match state.verifier.admit_device_remove(&remove, &sender, role) {
                Ok(admitted) => {
                    reply(ack(None));
                    state.store(&remove.log, admitted.head, &sender);
                    state.log_event(ServerMessage::DeviceRemoved(DeviceRemoved {
                        holder: remove.holder.clone(),
                    }));
                    state.removed(&admitted.removed);
                }
                Err(error) => reply(err(error.code())),
            }
        }
        ClientMessage::PresenceSet(set) => {
            reply(ack(None));
            let update = ServerMessage::PresenceUpdate(PresenceUpdate {
                holder: sender.clone(),
                state: set.state,
                status: set.status,
                at: UnixMillis(1),
            });
            state.others(&sender, &update);
            state.presence.insert(sender, update);
        }
        ClientMessage::ClaimSet(set) => {
            state.claim_sets += 1;
            reply(ack(None));
            let notices = state.board.set(
                SessionRef {
                    holder: sender,
                    session: set.session,
                },
                SessionClaims {
                    repo: set.repo,
                    epoch: set.epoch,
                    files: set.files,
                    detail: set.detail,
                },
            );
            state.deliver(notices);
        }
        ClientMessage::ClaimClear(clear) => {
            reply(ack(None));
            let notices = state.board.clear(&SessionRef {
                holder: sender,
                session: clear.session,
            });
            state.deliver(notices);
        }
        _ => reply(ack(None)),
    }
}

// ------------------------------------------------------------------------------------------
// Devices: an account and a team service each, with their own keychains and folders
// ------------------------------------------------------------------------------------------

struct Device {
    team: TeamService,
    account: AccountService,
    keychain: Arc<MemoryStore>,
    dir: Arc<TempDir>,
    base: String,
    snapshots: Arc<Mutex<Vec<TeamSnapshot>>>,
}

fn timing() -> Timing {
    Timing {
        renew_every: Duration::from_millis(300),
        max_age: Duration::from_secs(120),
        ping_every: Duration::from_secs(1),
        silence: Duration::from_secs(10),
        devices_every: Duration::from_millis(100),
        connect_timeout: Duration::from_secs(5),
        backoff_unit: Duration::from_millis(20),
        stable_after: Duration::from_secs(30),
        lock_retry: Duration::from_millis(50),
    }
}

impl Device {
    fn new(website: &Website, user: &str, name: &str) -> Self {
        Self::with(
            website,
            user,
            Arc::new(MemoryStore::default()),
            Arc::new(TempDir::new(name)),
        )
    }

    /// A second Hangar window of the same build on the same computer: same keychain, folders.
    fn twin(&self, website: &Website, user: &str) -> Self {
        Self::with(
            website,
            user,
            Arc::clone(&self.keychain),
            Arc::clone(&self.dir),
        )
    }

    fn with(website: &Website, user: &str, keychain: Arc<MemoryStore>, dir: Arc<TempDir>) -> Self {
        let accounts = Arc::new(MemoryStore::default());
        accounts
            .set(website.base().as_str(), &format!("hgrt_{user}_0"))
            .unwrap();
        let account = AccountService::new(
            website.base(),
            accounts,
            Arc::new(|_: &reqwest::Url| Ok(())),
            Arc::new(|_: &AccountView| {}),
            AccountOptions {
                unit: Duration::from_millis(10),
                ..AccountOptions::default()
            },
        );
        let snapshots = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&snapshots);
        let team = TeamService::new(
            account.clone(),
            Arc::clone(&keychain) as Arc<dyn SecretStore>,
            Arc::new(move |snapshot: &TeamSnapshot| guard(&seen).push(snapshot.clone())),
            Options {
                config_dir: Some(dir.0.join("config")),
                state_dir: Some(dir.0.join("local")),
                timing: timing(),
                ..Options::default()
            },
        );
        Self {
            team,
            account,
            keychain,
            dir,
            base: website.base().as_str().to_owned(),
            snapshots,
        }
    }

    fn key(&self) -> String {
        team::team_key(&self.base, PROJECT)
    }

    /// The projects this build shares, as `team.json` says.
    fn enabled(&self) -> Vec<String> {
        TeamConfig::load(&self.dir.0.join("config"))
            .unwrap()
            .enabled_for(team::KEYCHAIN_SERVICE, &self.base)
            .to_vec()
    }

    fn state_file(&self) -> PathBuf {
        team::LocalState::path(
            &self.dir.0.join("local"),
            team::KEYCHAIN_SERVICE,
            &self.key(),
        )
    }

    async fn attention(&self, reason: team::AttentionReason) {
        let want = TeamStatus::NeedsAttention { reason };
        self.until(&format!("{want:?}"), |s| *s == want).await;
    }

    fn keys(&self) -> Option<StoredKeys> {
        self.keychain
            .value(&team::keys_account(&self.key()))
            .and_then(|text| StoredKeys::decode(&text))
    }

    fn has_device(&self) -> bool {
        self.keychain
            .value(&team::device_account(&self.key()))
            .is_some()
    }

    fn status(&self) -> TeamStatus {
        self.team.status(PROJECT)
    }

    async fn until(&self, what: &str, test: impl Fn(&TeamStatus) -> bool) -> TeamStatus {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let status = self.status();
            if test(&status) {
                return status;
            }
            assert!(Instant::now() < deadline, "{what}: still {status:?}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn connected(&self, epoch: u32, members: usize, devices: usize) {
        let want = TeamStatus::Connected {
            epoch,
            members,
            devices,
        };
        self.until(&format!("{want:?}"), |s| *s == want).await;
    }

    async fn waiting(&self) -> Vec<String> {
        match self
            .until("waiting for confirmation", |s| {
                matches!(s, TeamStatus::WaitingForConfirmation { .. })
            })
            .await
        {
            TeamStatus::WaitingForConfirmation { safety_number } => safety_number,
            _ => unreachable!(),
        }
    }

    async fn pending(&self, count: usize) -> Vec<hangar_lib::team::PendingConfirmation> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let pending = self.team.pending();
            if pending.len() == count {
                return pending;
            }
            assert!(Instant::now() < deadline, "pending: {pending:?}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

async fn until(what: &str, test: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !test() {
        assert!(Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn run<F: std::future::Future<Output = ()>>(future: F) {
    tauri::async_runtime::block_on(future);
}

// ------------------------------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------------------------------

#[test]
fn owner_creates_the_team_devices_and_members_join_and_a_removal_rotates() {
    run(async {
        let relay = Relay::start().await;
        let website = Website::start(relay.port);
        {
            let mut mock = guard(&website.state);
            mock.roles.insert("alice".to_owned(), "owner");
            mock.roles.insert("bob".to_owned(), "member");
        }
        // The owner turns sharing on: team and first epoch.
        let a1 = Device::new(&website, "alice", "a1");
        assert_eq!(a1.status(), TeamStatus::Off);
        let started = a1.team.enable(PROJECT).await.unwrap();
        assert!(
            matches!(started, TeamStatus::Connecting { .. }),
            "{started:?}"
        );
        a1.connected(1, 1, 1).await;
        assert!(a1.has_device());
        let a1_keys = a1.keys().unwrap();
        assert!(a1_keys.epochs.contains_key(&Epoch(1)));
        assert_eq!(a1_keys.head.map(|h| h.index), Some(2));
        assert_eq!(a1.enabled(), [PROJECT.to_owned()]);
        assert!(guard(&a1.snapshots).iter().any(|s| s
            .teams
            .iter()
            .any(|t| matches!(t.status, TeamStatus::Connected { .. }))));

        // Alice's second device waits; the first shows the same safety number and endorses it.
        let a2 = Device::new(&website, "alice", "a2");
        a2.team.enable(PROJECT).await.unwrap();
        let shown = a2.waiting().await;
        let pending = a1.pending(1).await;
        assert_eq!(pending[0].kind, PendingKind::OwnDevice);
        assert_eq!(pending[0].member, "alice");
        assert_eq!(pending[0].safety_number, shown);
        a1.team.confirm(&pending[0].id).await.unwrap();
        a2.connected(1, 1, 2).await;
        assert_eq!(
            a2.keys().unwrap().epochs.get(&Epoch(1)),
            a1.keys().unwrap().epochs.get(&Epoch(1)),
            "the second device holds the same epoch secret"
        );

        // Bob, a new member, is added by the owner.
        let b1 = Device::new(&website, "bob", "b1");
        b1.team.enable(PROJECT).await.unwrap();
        b1.waiting().await;
        let pending = a1.pending(1).await;
        assert_eq!(pending[0].kind, PendingKind::NewMember);
        assert_eq!(pending[0].member, "bob");
        // An unknown request is refused.
        assert!(a1.team.confirm("unknown").await.is_err());
        a1.team.confirm(&pending[0].id).await.unwrap();
        b1.connected(1, 2, 3).await;
        a1.connected(1, 2, 3).await;

        // Bob is removed on the website, then from the log: the owner's device rotates; bob's
        // device sees the removal confirmed by the website, wipes his keys and stops sharing.
        guard(&website.state).roles.remove("bob");
        a1.team.remove_member(PROJECT, "bob").await.unwrap();
        b1.until("bob removed", |s| *s == TeamStatus::Removed).await;
        a1.connected(2, 1, 2).await;
        a2.connected(2, 1, 2).await;
        assert_eq!(
            a2.keys().unwrap().epochs.get(&Epoch(2)),
            a1.keys().unwrap().epochs.get(&Epoch(2))
        );
        until("bob's keys wiped", || !b1.has_device()).await;
        assert!(b1.keys().is_none());
        assert!(b1.enabled().is_empty(), "sharing turned off with the keys");

        // Turning sharing off closes the connection and keeps the device.
        assert_eq!(a2.team.disable(PROJECT).await.unwrap(), TeamStatus::Off);
        assert_eq!(a2.status(), TeamStatus::Off);
        assert!(a2.has_device());
        a1.team.shutdown();
    });
}

#[test]
fn reconnects_resume_renew_and_follow_the_close_codes() {
    run(async {
        let relay = Relay::start().await;
        let website = Website::start(relay.port);
        guard(&website.state)
            .roles
            .insert("alice".to_owned(), "owner");
        let a1 = Device::new(&website, "alice", "r1");
        a1.team.enable(PROJECT).await.unwrap();
        a1.connected(1, 1, 1).await;
        let device = guard(&relay.state).hellos[0].0.device.as_str().to_owned();
        // Tickets are renewed on the live connection.
        until("ticket renewed", || relay.state().renewals >= 1).await;

        // A dropped connection reconnects with a fresh ticket and resumes from its cursor.
        let hellos = relay.state().hellos.len();
        let head = relay.state().log.head();
        relay.close("alice", &device, 1001);
        until("reconnected", || relay.state().hellos.len() > hellos).await;
        {
            let state = relay.state();
            let (_, cursor, resynced) = state.hellos.last().unwrap();
            assert_eq!(cursor.as_ref().map(|c| c.seq), Some(head));
            assert!(!resynced, "resumed without a snapshot");
        }
        a1.connected(1, 1, 1).await;

        // A stale ticket on hello: a fresh ticket, one retry, connected again.
        let tickets = website.tickets();
        let hellos = relay.state().hellos.len();
        relay.state().refuse_hellos = 1;
        relay.close("alice", &device, 1011);
        until("retried after forbidden", || {
            relay.state().hellos.len() > hellos
        })
        .await;
        assert!(website.tickets() >= tickets + 2);
        a1.connected(1, 1, 1).await;

        // 4401: role changed, a fresh ticket at once.
        let hellos = relay.state().hellos.len();
        relay.close("alice", &device, 4401);
        until("reconnected after 4401", || {
            relay.state().hellos.len() > hellos
        })
        .await;
        a1.connected(1, 1, 1).await;

        // 4403: removed, no reconnect; the device keeps its keys.
        relay.close("alice", &device, 4403);
        a1.until("removed", |s| *s == TeamStatus::Removed).await;
        let hellos = relay.state().hellos.len();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(
            relay.state().hellos.len(),
            hellos,
            "no reconnect after 4403"
        );
        assert!(a1.has_device());

        // Turned off and on again it connects. A 4404 for a project the website still lists is
        // the relay's word alone: nothing is deleted.
        a1.team.disable(PROJECT).await.unwrap();
        a1.team.enable(PROJECT).await.unwrap();
        a1.connected(1, 1, 1).await;
        relay.close("alice", &device, 4404);
        a1.attention(team::AttentionReason::Refused).await;
        assert!(a1.has_device());
        assert_eq!(a1.enabled(), [PROJECT.to_owned()]);

        // Deleted on the website too: the team's local state goes and sharing turns off.
        a1.team.disable(PROJECT).await.unwrap();
        a1.team.enable(PROJECT).await.unwrap();
        a1.connected(1, 1, 1).await;
        guard(&website.state).roles.remove("alice");
        relay.close("alice", &device, 4404);
        a1.until("deleted", |s| *s == TeamStatus::ProjectDeleted)
            .await;
        until("local state dropped", || !a1.has_device()).await;
        assert!(a1.enabled().is_empty());
    });
}

#[test]
fn an_unreadable_keys_entry_stops_instead_of_forgetting_the_verified_history() {
    run(async {
        let relay = Relay::start().await;
        let website = Website::start(relay.port);
        guard(&website.state)
            .roles
            .insert("alice".to_owned(), "owner");
        let a1 = Device::new(&website, "alice", "k1");
        a1.team.enable(PROJECT).await.unwrap();
        a1.connected(1, 1, 1).await;
        a1.team.disable(PROJECT).await.unwrap();

        // Starting over would drop the verified log head; the team stops for the person instead.
        a1.keychain
            .set(&team::keys_account(&a1.key()), "garbage")
            .unwrap();
        a1.team.enable(PROJECT).await.unwrap();
        a1.until("keychain attention", |s| {
            *s == TeamStatus::NeedsAttention {
                reason: team::AttentionReason::Keychain,
            }
        })
        .await;
        assert!(a1.has_device(), "the device identity stays");
        assert_eq!(
            a1.keychain.value(&team::keys_account(&a1.key())).as_deref(),
            Some("garbage"),
            "the entry is left for the person to look at"
        );
        a1.team.shutdown();
    });
}

#[test]
fn an_unreadable_device_entry_stops_and_is_never_overwritten() {
    run(async {
        let relay = Relay::start().await;
        let website = Website::start(relay.port);
        guard(&website.state)
            .roles
            .insert("alice".to_owned(), "owner");
        let a1 = Device::new(&website, "alice", "d1");
        a1.keychain
            .set(&team::device_account(&a1.key()), "garbage")
            .unwrap();
        a1.team.enable(PROJECT).await.unwrap();
        a1.until("keychain attention", |s| {
            *s == TeamStatus::NeedsAttention {
                reason: team::AttentionReason::Keychain,
            }
        })
        .await;
        assert_eq!(
            a1.keychain
                .value(&team::device_account(&a1.key()))
                .as_deref(),
            Some("garbage"),
            "the device entry is not replaced by a new identity"
        );
        assert!(relay.state().hellos.is_empty(), "no device registered");
        a1.team.shutdown();
    });
}

#[test]
fn relay_events_without_a_log_entry_or_the_website_wipe_nothing() {
    run(async {
        let relay = Relay::start().await;
        let website = Website::start(relay.port);
        guard(&website.state)
            .roles
            .insert("alice".to_owned(), "owner");
        let a1 = Device::new(&website, "alice", "e1");
        a1.team.enable(PROJECT).await.unwrap();
        a1.connected(1, 1, 1).await;
        let holder = relay.state().hellos[0].0.clone();
        let hellos = relay.state().hellos.len();
        // The relay claims a removal and a lower role; neither the log nor the website agrees.
        relay.broadcast(ServerMessage::DeviceRemoved(DeviceRemoved {
            holder: holder.clone(),
        }));
        relay.broadcast(ServerMessage::MemberRemoved(MemberRemoved {
            member: holder.member.clone(),
        }));
        relay.broadcast(ServerMessage::MemberRoleChanged(MemberRoleChanged {
            member: holder.member.clone(),
            role: Role::Member,
        }));
        tokio::time::sleep(Duration::from_millis(500)).await;
        a1.connected(1, 1, 1).await;
        assert!(a1.has_device());
        assert!(a1.keys().unwrap().epochs.contains_key(&Epoch(1)));
        assert_eq!(a1.enabled(), [PROJECT.to_owned()]);
        assert_eq!(relay.state().hellos.len(), hellos, "no reconnect either");
        a1.team.shutdown();
    });
}

#[test]
fn a_sign_in_restarts_signed_out_teams_and_a_server_change_stops_the_old_ones() {
    run(async {
        let relay = Relay::start().await;
        let website = Website::start(relay.port);
        guard(&website.state)
            .roles
            .insert("alice".to_owned(), "owner");
        let a1 = Device::new(&website, "alice", "s1");
        a1.team.enable(PROJECT).await.unwrap();
        a1.connected(1, 1, 1).await;

        // Signed out: the connection closes at once, not only at the next ticket renewal.
        a1.account.sign_out().await.unwrap();
        assert_eq!(
            a1.status(),
            TeamStatus::NeedsAttention {
                reason: team::AttentionReason::SignedOut
            }
        );
        until("the signed out connection closed", || {
            relay.state().conns.is_empty()
        })
        .await;
        // Signed in again (device flow): the team connects on its own.
        guard(&website.state).device_user = "alice".to_owned();
        a1.account.sign_in().await.unwrap();
        a1.connected(1, 1, 1).await;

        // Another server: the old server's connection closes.
        let other_relay = Relay::start().await;
        let other = Website::start(other_relay.port);
        a1.account
            .set_base_url(Some(other.base().as_str().to_owned()))
            .await
            .unwrap();
        until("the old connection closed", || {
            relay.state().conns.is_empty()
        })
        .await;
        assert_eq!(a1.team.status(PROJECT), TeamStatus::Off);
        assert!(a1.team.snapshot().teams.is_empty());
        a1.team.shutdown();
    });
}

#[test]
fn a_second_window_waits_while_the_first_runs_the_team() {
    run(async {
        let relay = Relay::start().await;
        let website = Website::start(relay.port);
        guard(&website.state)
            .roles
            .insert("alice".to_owned(), "owner");
        let first = Device::new(&website, "alice", "w1");
        first.team.enable(PROJECT).await.unwrap();
        first.connected(1, 1, 1).await;
        let hellos = relay.state().hellos.len();

        let second = first.twin(&website, "alice");
        second.team.enable(PROJECT).await.unwrap();
        second.attention(team::AttentionReason::OtherInstance).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            relay.state().hellos.len(),
            hellos,
            "only one window connects"
        );

        // The first window turns sharing off: the second takes over with the same device.
        first.team.disable(PROJECT).await.unwrap();
        second.connected(1, 1, 1).await;
        {
            let state = relay.state();
            assert_eq!(state.hellos.last().unwrap().0, state.hellos[0].0);
        }
        second.team.shutdown();
    });
}

/// The clone of the shared repository on each test device (its git common folder key).
const FOLDER: &str = "c:/src/shop/.git";
/// Another clone of the same repository (a private fork, say) on the same device.
const FORK: &str = "c:/src/shop-fork/.git";

/// One running session of this device in the repository `root`, changing `files`.
fn local(id: u64, agent: &str, root: &str, files: &[(&str, u32, u32)]) -> LocalSession {
    local_in(id, agent, root, FOLDER, files)
}

fn local_in(
    id: u64,
    agent: &str,
    root: &str,
    folder: &str,
    files: &[(&str, u32, u32)],
) -> LocalSession {
    LocalSession {
        id,
        agent: agent.to_owned(),
        title: format!("{agent} at work"),
        state: SessionState::Working,
        repo: root.to_owned(),
        folder: folder.to_owned(),
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

#[test]
fn teammates_see_each_others_sessions_and_a_shared_file_down_to_the_lines() {
    run(async {
        let relay = Relay::start().await;
        let website = Website::start(relay.port);
        {
            let mut mock = guard(&website.state);
            mock.roles.insert("alice".to_owned(), "owner");
            mock.roles.insert("bob".to_owned(), "member");
        }
        let a1 = Device::new(&website, "alice", "p-a1");
        a1.team.enable(PROJECT).await.unwrap();
        a1.connected(1, 1, 1).await;
        let b1 = Device::new(&website, "bob", "p-b1");
        b1.team.enable(PROJECT).await.unwrap();
        b1.waiting().await;
        let pending = a1.pending(1).await;
        a1.team.confirm(&pending[0].id).await.unwrap();
        b1.connected(1, 2, 2).await;

        let root = "a".repeat(40);
        a1.team.set_local(vec![
            local(1, "Claude Code", &root, &[("src/auth.rs", 10, 12)]),
            // A fork with the same root commit in another folder: never shared by the link
            // of the first folder.
            local_in(2, "Aider", &root, FORK, &[("src/private.rs", 1, 2)]),
        ]);
        b1.team.set_local(vec![local(
            5,
            "Codex",
            &root,
            &[("src/Auth.rs", 14, 15), ("docs/notes.md", 1, 1)],
        )]);
        // The repository is not linked yet: each device is present, but shares no session.
        until("bob sees alice", || {
            b1.team
                .snapshot()
                .teams
                .first()
                .is_some_and(|t| t.activity.peers.len() == 1)
        })
        .await;
        assert!(b1.team.snapshot().teams[0].activity.peers[0]
            .sessions
            .is_empty());
        assert_eq!(relay.state().claim_sets, 0, "no claims before linking");
        assert_eq!(b1.team.snapshot().repos[0].root, root);
        assert_eq!(b1.team.snapshot().repos[0].folder, FOLDER);
        assert_eq!(a1.team.snapshot().repos.len(), 2, "each folder is a choice");
        // Only a project with sharing on can be linked, and only with a valid project id.
        assert!(matches!(
            a1.team.link_repo("other", &root, FOLDER, true),
            Err(TeamError::UnknownProject)
        ));
        assert!(matches!(
            a1.team.link_repo(&"x".repeat(500), &root, FOLDER, true),
            Err(TeamError::UnknownProject)
        ));

        // Both link it: presence lists the sessions, the relay matches the file by its keyed
        // hash and each side refines the overlap to the lines with the other's sealed detail.
        a1.team.link_repo(PROJECT, &root, FOLDER, true).unwrap();
        b1.team.link_repo(PROJECT, &root, FOLDER, true).unwrap();
        until("bob sees alice's session", || {
            b1.team.snapshot().teams[0].activity.peers[0]
                .sessions
                .iter()
                .any(|s| s.agent == "Claude Code" && s.files == ["src/auth.rs"])
        })
        .await;
        until("alice sees the overlapping lines", || {
            a1.team.snapshot().teams[0]
                .activity
                .overlaps
                .iter()
                .any(|o| {
                    o.session_id == 1
                        && o.level == OverlapLevel::OverlappingLines
                        && o.lines == [LineRange { start: 10, end: 15 }]
                        && o.agent == "Codex"
                })
        })
        .await;
        // Only the linked folder: the fork's session stays on this device.
        let seen = b1.team.snapshot().teams[0].activity.peers[0].clone();
        assert_eq!(seen.sessions.len(), 1, "{seen:?}");
        // The website lists no display name: the teammates get none, never the e-mail.
        assert_eq!(seen.name, "");
        let files = a1.team.teammate_files(&root, FOLDER);
        let auth = files.iter().find(|f| f.key == "src/auth.rs").unwrap();
        assert_eq!(
            auth.who, "#bob (Codex)",
            "the member id stands in for the name"
        );
        assert!(a1.team.teammate_files(&"c".repeat(40), FOLDER).is_empty());
        assert!(
            a1.team.teammate_files(&root, FORK).is_empty(),
            "a clone not linked to the project is not paused by its teammates"
        );
        assert_eq!(a1.team.snapshot().teams[0].activity.linked[0].name, "shop");

        // A link from before folders were bound (root commit only) can be removed on its own;
        // the folder link stays.
        let config_dir = a1.dir.0.join("config");
        let mut config = TeamConfig::load(&config_dir).unwrap();
        let legacy = team::LinkedRepo {
            root: "d".repeat(40),
            folder: None,
            name: "old".to_owned(),
        };
        config.link(team::KEYCHAIN_SERVICE, &a1.base, PROJECT, &legacy, true);
        config.save(&config_dir).unwrap();
        assert_eq!(a1.team.snapshot().teams[0].activity.linked.len(), 2);
        a1.team.unlink_legacy_repo(PROJECT, &legacy.root).unwrap();
        let linked = a1.team.snapshot().teams[0].activity.linked.clone();
        assert_eq!(linked.len(), 1, "{linked:?}");
        assert_eq!(linked[0].folder.as_deref(), Some(FOLDER));

        // Bob turns sharing off: his device goes offline, the overlap ends on Alice's device.
        b1.team.disable(PROJECT).await.unwrap();
        until("bob gone", || {
            let snapshot = a1.team.snapshot();
            snapshot.teams[0].activity.peers.is_empty()
                && snapshot.teams[0].activity.overlaps.is_empty()
        })
        .await;
        assert!(a1.team.teammate_files(&root, FOLDER).is_empty());
        a1.team.shutdown();
    });
}

#[test]
fn a_device_never_acts_for_another_person_on_the_same_keychain() {
    run(async {
        let relay = Relay::start().await;
        let website = Website::start(relay.port);
        {
            let mut mock = guard(&website.state);
            mock.roles.insert("alice".to_owned(), "owner");
            mock.roles.insert("bob".to_owned(), "member");
        }
        let a1 = Device::new(&website, "alice", "other-account");
        a1.team.enable(PROJECT).await.unwrap();
        a1.connected(1, 1, 1).await;
        let device = a1.keychain.value(&team::device_account(&a1.key())).unwrap();
        assert_eq!(
            StoredDevice::decode(&device).unwrap().member.as_deref(),
            Some("alice"),
            "bound to the account of its first welcome"
        );
        let keys = a1.keychain.value(&team::keys_account(&a1.key())).unwrap();

        // Bob signs in on the same computer and keychain: the team waits, nothing is touched.
        a1.account.sign_out().await.unwrap();
        guard(&website.state).device_user = "bob".to_owned();
        a1.account.sign_in().await.unwrap();
        a1.attention(team::AttentionReason::OtherAccount).await;
        assert_eq!(
            a1.keychain.value(&team::device_account(&a1.key())),
            Some(device.clone())
        );
        assert_eq!(
            a1.keychain.value(&team::keys_account(&a1.key())),
            Some(keys)
        );
        assert_eq!(a1.enabled(), [PROJECT.to_owned()], "sharing stays on");

        // Alice signs in again: her device connects as before.
        a1.account.sign_out().await.unwrap();
        guard(&website.state).device_user = "alice".to_owned();
        a1.account.sign_in().await.unwrap();
        a1.connected(1, 1, 1).await;
        assert_eq!(
            a1.keychain.value(&team::device_account(&a1.key())),
            Some(device)
        );
        a1.team.shutdown();
    });
}

#[test]
fn an_unreadable_settings_file_is_never_written_over() {
    run(async {
        let relay = Relay::start().await;
        let website = Website::start(relay.port);
        guard(&website.state)
            .roles
            .insert("alice".to_owned(), "owner");
        let a1 = Device::new(&website, "alice", "settings-broken");
        let config = a1.dir.0.join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join(team::CONFIG_FILE), "{ broken").unwrap();
        let error = a1.team.enable(PROJECT).await.unwrap_err();
        assert!(matches!(error, TeamError::Config(_)), "{error:?}");
        assert_eq!(
            std::fs::read_to_string(config.join(team::CONFIG_FILE)).unwrap(),
            "{ broken"
        );
        assert_eq!(a1.status(), TeamStatus::Off);
        assert!(relay.state().hellos.is_empty());
    });
}

#[test]
fn signing_out_and_in_again_hands_the_team_over_without_a_wait() {
    run(async {
        let relay = Relay::start().await;
        let website = Website::start(relay.port);
        guard(&website.state)
            .roles
            .insert("alice".to_owned(), "owner");
        let a1 = Device::new(&website, "alice", "handover");
        a1.team.enable(PROJECT).await.unwrap();
        a1.connected(1, 1, 1).await;
        for _ in 0..3 {
            let seen = guard(&a1.snapshots).len();
            a1.account.sign_out().await.unwrap();
            guard(&website.state).device_user = "alice".to_owned();
            a1.account.sign_in().await.unwrap();
            a1.connected(1, 1, 1).await;
            let other = TeamStatus::NeedsAttention {
                reason: team::AttentionReason::OtherInstance,
            };
            assert!(
                !guard(&a1.snapshots)[seen..]
                    .iter()
                    .any(|s| s.teams.iter().any(|t| t.status == other)),
                "the new worker met the old one's lock file"
            );
        }
        a1.team.shutdown();
    });
}

#[test]
fn a_refused_keys_write_sends_no_key_rotate() {
    run(async {
        let relay = Relay::start().await;
        let website = Website::start(relay.port);
        guard(&website.state)
            .roles
            .insert("alice".to_owned(), "owner");
        let a1 = Device::new(&website, "alice", "f1");
        a1.keychain.refuse_keys.store(true, Ordering::SeqCst);
        a1.team.enable(PROJECT).await.unwrap();
        a1.attention(team::AttentionReason::Keychain).await;
        assert_eq!(
            relay.state().rotates,
            0,
            "no epoch without its stored secret"
        );
        assert_eq!(relay.state().epoch, Epoch(0));
        a1.team.shutdown();
    });
}

#[test]
fn a_failed_state_write_ends_the_connection_before_anything_else() {
    run(async {
        let relay = Relay::start().await;
        let website = Website::start(relay.port);
        guard(&website.state)
            .roles
            .insert("alice".to_owned(), "owner");
        let a1 = Device::new(&website, "alice", "f2");
        // A folder where the state file belongs: every write fails.
        std::fs::create_dir_all(a1.state_file()).unwrap();
        a1.team.enable(PROJECT).await.unwrap();
        until("reconnected after the failed write", || {
            relay.state().hellos.len() >= 2
        })
        .await;
        assert!(
            relay.state().devices.is_empty(),
            "nothing was sent after the failed write"
        );
        assert!(!matches!(a1.status(), TeamStatus::Connected { .. }));
        a1.team.shutdown();
    });
}

#[test]
fn a_rewritten_history_stops_the_team() {
    run(async {
        let relay = Relay::start().await;
        let website = Website::start(relay.port);
        guard(&website.state)
            .roles
            .insert("alice".to_owned(), "owner");
        let a1 = Device::new(&website, "alice", "h1");
        a1.team.enable(PROJECT).await.unwrap();
        a1.connected(1, 1, 1).await;
        let verified = a1.keys().unwrap().head;
        a1.team.disable(PROJECT).await.unwrap();

        // The relay forgets the team, and another device builds a new log under its id.
        relay.forget();
        let m1 = Device::new(&website, "alice", "h2");
        m1.team.enable(PROJECT).await.unwrap();
        m1.connected(1, 1, 1).await;

        a1.team.enable(PROJECT).await.unwrap();
        a1.attention(team::AttentionReason::HistoryMismatch).await;
        assert_eq!(a1.keys().unwrap().head, verified, "the verified head stays");
        m1.team.shutdown();
        a1.team.shutdown();
    });
}

#[test]
fn an_unreadable_state_file_stops_instead_of_starting_empty() {
    run(async {
        let relay = Relay::start().await;
        let website = Website::start(relay.port);
        guard(&website.state)
            .roles
            .insert("alice".to_owned(), "owner");
        let a1 = Device::new(&website, "alice", "u1");
        a1.team.enable(PROJECT).await.unwrap();
        a1.connected(1, 1, 1).await;
        a1.team.disable(PROJECT).await.unwrap();
        std::fs::write(a1.state_file(), "{ not json").unwrap();
        a1.team.enable(PROJECT).await.unwrap();
        a1.attention(team::AttentionReason::Storage).await;
        assert_eq!(
            std::fs::read_to_string(a1.state_file()).unwrap(),
            "{ not json",
            "the file is left for the person"
        );
        a1.team.shutdown();
    });
}

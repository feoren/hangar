//! Push to the user's phone through ntfy or Pushover when a session waits, is done or failed
//! (contract: docs/wiki/pages/syntheses/session-resources-and-phone-push.md).
//!
//! Opt-in: the provider is `Off` until the user sets one up, and nothing leaves the machine
//! before. The window decides when a session's state is worth a push (and keeps quiet hours); the
//! backend checks the chosen events and builds the message itself from the session's program and
//! running number (or its name, if the user allows that), never from terminal content. It sends
//! at most one push per session every [`MIN_GAP`] and [`MAX_PER_MINUTE`] in all, over https only,
//! without redirects, with a short time limit and no retries.
//!
//! The provider, the ntfy address and the events live in `push.json` in the config folder. The
//! ntfy access token and both Pushover keys live only in the OS keychain (service
//! [`KEYCHAIN_SERVICE`]); they never reach a file, a log, an error text or the window, which
//! only learns whether each one is stored. The ntfy token is bound to the origin it was entered
//! for: another scheme, host or port needs the token again.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize, Serializer};
use zeroize::{Zeroize as _, Zeroizing};

use crate::lock;
use crate::secrets::{SecretError, SecretStore};
use crate::session::SessionId;

/// Keychain service of the push secrets, apart from the profile secrets (`secrets::SERVICE`), so
/// no profile variable can share an entry with them.
pub const KEYCHAIN_SERVICE: &str = "hangar-push";
pub const CONFIG_FILE: &str = "push.json";
/// At most one push per session in this time, whatever the window asks.
pub const MIN_GAP: Duration = Duration::from_secs(30);
/// At most this many pushes per minute in all, tests included.
pub const MAX_PER_MINUTE: usize = 10;
/// At most one test push in this time.
pub const TEST_GAP: Duration = Duration::from_secs(5);
/// Total time limit of one push, connection included.
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// Without an access token, anyone who knows an ntfy topic reads along: it must be this long.
pub const MIN_OPEN_TOPIC_CHARS: usize = 16;
pub const PUSHOVER_ENDPOINT: &str = "https://api.pushover.net/1/messages.json";
/// Placeholder for the session in a message template.
pub const NAME_PLACEHOLDER: &str = "{name}";
/// Title of every push; the message itself names the session.
const TITLE: &str = "Hangar";
const MAX_URL_CHARS: usize = 512;
const MAX_TOPIC_CHARS: usize = 64;
const MAX_TOKEN_CHARS: usize = 256;
const MAX_KEY_CHARS: usize = 64;
const MAX_TEXT_CHARS: usize = 200;
const MAX_TEMPLATE_CHARS: usize = 160;
const RATE_WINDOW: Duration = Duration::from_secs(60);

const NTFY_TOKEN: &str = "ntfy-token";
const PUSHOVER_USER: &str = "pushover-user";
const PUSHOVER_TOKEN: &str = "pushover-token";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Off,
    Ntfy,
    Pushover,
}

/// The session states a push can tell about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PushEvent {
    /// The agent waits for the user.
    Waiting,
    /// The program ended with exit code 0.
    Done,
    /// The program ended with another exit code.
    Failed,
}

/// Which events are sent; all of them once a provider is set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Events {
    pub waiting: bool,
    pub done: bool,
    pub failed: bool,
}

impl Default for Events {
    fn default() -> Self {
        Self {
            waiting: true,
            done: true,
            failed: true,
        }
    }
}

impl Events {
    pub fn allows(self, event: PushEvent) -> bool {
        match event {
            PushEvent::Waiting => self.waiting,
            PushEvent::Done => self.done,
            PushEvent::Failed => self.failed,
        }
    }
}

/// `push.json`: everything but the secrets.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PushConfig {
    pub provider: Provider,
    /// `https://host/topic`; kept when another provider is chosen.
    pub ntfy_url: String,
    /// Origin (`https://host:port`) the stored ntfy token was entered for; empty without one.
    pub ntfy_token_origin: String,
    /// The message names the session by its name, which may hold the first prompt, instead of
    /// its program and running number.
    pub include_title: bool,
    pub events: Events,
}

/// What the window sees: the settings and whether each secret is stored, never a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushView {
    pub provider: Provider,
    pub ntfy_url: String,
    pub include_title: bool,
    pub events: Events,
    pub ntfy_token: bool,
    pub pushover_user: bool,
    pub pushover_token: bool,
}

/// A save from the window. Each secret: `None` keeps the stored one, an empty text removes it,
/// any other text replaces it. The secrets are wiped when the input is dropped; `Debug` names
/// only whether each one is given.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushInput {
    pub provider: Provider,
    pub ntfy_url: String,
    #[serde(default)]
    pub include_title: bool,
    pub events: Events,
    pub ntfy_token: Option<String>,
    pub pushover_user: Option<String>,
    pub pushover_token: Option<String>,
}

impl std::fmt::Debug for PushInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let given = |value: &Option<String>| value.as_ref().map(|_| "<redacted>");
        f.debug_struct("PushInput")
            .field("provider", &self.provider)
            .field("ntfy_url", &self.ntfy_url)
            .field("include_title", &self.include_title)
            .field("events", &self.events)
            .field("ntfy_token", &given(&self.ntfy_token))
            .field("pushover_user", &given(&self.pushover_user))
            .field("pushover_token", &given(&self.pushover_token))
            .finish()
    }
}

impl Drop for PushInput {
    fn drop(&mut self) {
        for secret in [
            &mut self.ntfy_token,
            &mut self.pushover_user,
            &mut self.pushover_token,
        ]
        .into_iter()
        .flatten()
        {
            secret.zeroize();
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PushError {
    #[error("The ntfy address must be an https URL ending in a topic")]
    InvalidUrl,
    #[error("The access token is not valid")]
    InvalidToken,
    #[error("A Pushover key is not valid")]
    InvalidKey,
    #[error("The provider is not set up completely")]
    Incomplete,
    #[error("The stored access token belongs to another server; enter it again")]
    TokenOrigin,
    #[error("Without an access token the topic must be at least 16 characters long")]
    WeakTopic,
    #[error("The message template is not valid")]
    InvalidTemplate,
    #[error("No such session")]
    UnknownSession,
    #[error("Too many pushes in a short time")]
    RateLimited,
    #[error("Keychain: {0}")]
    Keychain(#[from] SecretError),
    #[error("The push settings could not be saved: {0}")]
    Storage(String),
    #[error("The push service could not be reached")]
    Network,
    #[error("The push service refused the token or key")]
    Unauthorized,
    #[error("The push service answered with status {0}")]
    Rejected(u16),
    #[error("Background task aborted")]
    Task,
}

impl PushError {
    /// Stable name for the window, which shows its own translated text.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidUrl => "invalidUrl",
            Self::InvalidToken => "invalidToken",
            Self::InvalidKey => "invalidKey",
            Self::Incomplete => "incomplete",
            Self::TokenOrigin => "tokenOrigin",
            Self::WeakTopic => "weakTopic",
            Self::InvalidTemplate => "invalidTemplate",
            Self::UnknownSession => "unknownSession",
            Self::RateLimited => "rateLimited",
            Self::Keychain(_) => "keychain",
            Self::Storage(_) => "storage",
            Self::Network => "network",
            Self::Unauthorized => "unauthorized",
            Self::Rejected(_) => "rejected",
            Self::Task => "task",
        }
    }
}

impl Serialize for PushError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.kind())
    }
}

// ------------------------------------------------------------------------------------------
// Validation
// ------------------------------------------------------------------------------------------

/// The ntfy address: https, a host, no user info, query or fragment, and a topic of letters,
/// digits, `-` and `_` (at most 64, as ntfy allows) as its last path segment (a server may sit
/// under a path prefix).
pub fn ntfy_url(text: &str) -> Result<Url, PushError> {
    let text = text.trim();
    if text.chars().count() > MAX_URL_CHARS {
        return Err(PushError::InvalidUrl);
    }
    let url = Url::parse(text).map_err(|_| PushError::InvalidUrl)?;
    let plain = url.scheme() == "https"
        && url.host_str().is_some_and(|host| !host.is_empty())
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none();
    let topic = topic(&url);
    let topic_ok = !topic.is_empty()
        && topic.len() <= MAX_TOPIC_CHARS
        && topic
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if plain && topic_ok {
        Ok(url)
    } else {
        Err(PushError::InvalidUrl)
    }
}

fn topic(url: &Url) -> &str {
    url.path_segments()
        .and_then(|mut segments| segments.next_back())
        .unwrap_or_default()
}

/// `https://host:port`, what the ntfy token is bound to.
fn origin(url: &Url) -> String {
    url.origin().ascii_serialization()
}

/// An ntfy access token: visible ASCII without spaces.
fn check_token(token: &str) -> Result<(), PushError> {
    let ok = !token.is_empty()
        && token.len() <= MAX_TOKEN_CHARS
        && token.bytes().all(|b| b.is_ascii_graphic());
    ok.then_some(()).ok_or(PushError::InvalidToken)
}

/// A Pushover user key or application token: letters and digits.
fn check_key(key: &str) -> Result<(), PushError> {
    let ok = !key.is_empty()
        && key.len() <= MAX_KEY_CHARS
        && key.bytes().all(|b| b.is_ascii_alphanumeric());
    ok.then_some(()).ok_or(PushError::InvalidKey)
}

/// The message as sent: no control characters, at most [`MAX_TEXT_CHARS`], never empty.
pub fn clean_text(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let cleaned: String = cleaned.trim().chars().take(MAX_TEXT_CHARS).collect();
    if cleaned.is_empty() {
        TITLE.to_owned()
    } else {
        cleaned
    }
}

/// The window's translated template ("{name} is waiting for you") with the session's name: one
/// placeholder, no control characters, short. The window chooses the words, the backend the name.
pub fn fill(template: &str, name: &str) -> Result<String, PushError> {
    let template = template.trim();
    let ok = template.chars().count() <= MAX_TEMPLATE_CHARS
        && template.matches(NAME_PLACEHOLDER).count() == 1
        && !template.chars().any(char::is_control);
    if !ok {
        return Err(PushError::InvalidTemplate);
    }
    Ok(clean_text(&template.replace(NAME_PLACEHOLDER, name)))
}

/// How a push can name a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionNames {
    /// Program and running number among its sessions, "Claude Code (3)".
    pub program: String,
    /// The session's name in the sidebar; may hold the first prompt.
    pub title: String,
}

/// The names of session `id` among `sessions` (id, profile id, title); `None` for an unknown id.
/// The number counts the sessions of the same program in start order.
pub fn session_names(
    sessions: &[(SessionId, String, String)],
    programs: &HashMap<String, String>,
    id: SessionId,
) -> Option<SessionNames> {
    let (_, profile, title) = sessions.iter().find(|(other, ..)| *other == id)?;
    let mut same: Vec<SessionId> = sessions
        .iter()
        .filter(|(_, other, _)| other == profile)
        .map(|(other, ..)| *other)
        .collect();
    same.sort_unstable();
    let number = same.iter().position(|other| *other == id)? + 1;
    let program = programs.get(profile).unwrap_or(profile);
    Some(SessionNames {
        program: format!("{program} ({number})"),
        title: title.clone(),
    })
}

// ------------------------------------------------------------------------------------------
// Requests
// ------------------------------------------------------------------------------------------

/// One push, ready to send. No `Debug`: it carries secrets.
pub struct PushRequest {
    pub url: Url,
    pub content_type: &'static str,
    /// ntfy access token, sent as `Authorization: Bearer`.
    pub bearer: Option<Zeroizing<String>>,
    /// Further headers (ntfy's `Title`).
    pub headers: Vec<(&'static str, &'static str)>,
    pub body: Zeroizing<Vec<u8>>,
}

/// ntfy: the message is the body, the title a header. The server's cache stays on: phones that
/// are not connected at that moment (the iOS app polls) would otherwise never get the push, and
/// the text names only the program and number unless the user chose to send the session name.
pub fn ntfy_request(url: Url, token: Option<Zeroizing<String>>, text: &str) -> PushRequest {
    PushRequest {
        url,
        content_type: "text/plain; charset=utf-8",
        bearer: token,
        headers: vec![("Title", TITLE)],
        body: Zeroizing::new(clean_text(text).into_bytes()),
    }
}

/// Pushover: one form with both keys, the title and the message.
pub fn pushover_request(endpoint: Url, user: &str, token: &str, text: &str) -> PushRequest {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("token", token)
        .append_pair("user", user)
        .append_pair("title", TITLE)
        .append_pair("message", &clean_text(text))
        .finish();
    PushRequest {
        url: endpoint,
        content_type: "application/x-www-form-urlencoded",
        bearer: None,
        headers: Vec::new(),
        body: Zeroizing::new(body.into_bytes()),
    }
}

/// The client every push goes through: https only, no redirects (a redirect could carry the
/// token elsewhere), [`TIMEOUT`].
pub fn client() -> Result<reqwest::Client, PushError> {
    build_client(true)
}

fn build_client(https_only: bool) -> Result<reqwest::Client, PushError> {
    crate::updates::ensure_crypto_provider();
    let mut builder = reqwest::Client::builder()
        .user_agent(format!("Hangar/{}", env!("CARGO_PKG_VERSION")))
        .timeout(TIMEOUT)
        .connect_timeout(TIMEOUT)
        .https_only(https_only)
        .redirect(reqwest::redirect::Policy::none());
    if !https_only {
        // Tests only: their server on 127.0.0.1 must not go through a system proxy.
        builder = builder.no_proxy();
    }
    builder.build().map_err(|_| PushError::Network)
}

/// Sends once. The texts of network errors are dropped: they may name the topic.
pub async fn deliver(client: &reqwest::Client, mut request: PushRequest) -> Result<(), PushError> {
    // reqwest takes the body over; its copies are not wiped, Hangar's own buffer is.
    let body = std::mem::take(&mut *request.body);
    let mut builder = client
        .post(request.url.clone())
        .header(reqwest::header::CONTENT_TYPE, request.content_type)
        .body(body);
    if let Some(token) = &request.bearer {
        builder = builder.bearer_auth(token.as_str());
    }
    for (name, value) in &request.headers {
        builder = builder.header(*name, *value);
    }
    let status = builder
        .send()
        .await
        .map_err(|_| PushError::Network)?
        .status();
    answer(status)
}

fn answer(status: StatusCode) -> Result<(), PushError> {
    if status.is_success() {
        Ok(())
    } else if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        Err(PushError::Unauthorized)
    } else {
        Err(PushError::Rejected(status.as_u16()))
    }
}

// ------------------------------------------------------------------------------------------
// Limits
// ------------------------------------------------------------------------------------------

/// When each session's last push went out.
#[derive(Debug, Default)]
pub struct Debounce {
    last: HashMap<SessionId, Instant>,
}

impl Debounce {
    /// Whether a push for `id` may go out `now`; if so, it counts as sent.
    pub fn allow(&mut self, id: SessionId, now: Instant) -> bool {
        self.last
            .retain(|_, at| now.saturating_duration_since(*at) < MIN_GAP);
        if self.last.contains_key(&id) {
            return false;
        }
        self.last.insert(id, now);
        true
    }
}

/// At most [`MAX_PER_MINUTE`] pushes in any minute, whatever session or test they are for.
#[derive(Debug, Default)]
pub struct RateLimit {
    sent: VecDeque<Instant>,
}

impl RateLimit {
    /// Whether one more push may go out `now`; if so, it counts as sent.
    pub fn allow(&mut self, now: Instant) -> bool {
        while self
            .sent
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= RATE_WINDOW)
        {
            self.sent.pop_front();
        }
        if self.sent.len() >= MAX_PER_MINUTE {
            return false;
        }
        self.sent.push_back(now);
        true
    }
}

// ------------------------------------------------------------------------------------------
// Service
// ------------------------------------------------------------------------------------------

pub struct PushService {
    config_dir: PathBuf,
    store: Arc<dyn SecretStore>,
    config: Mutex<PushConfig>,
    debounce: Mutex<Debounce>,
    rate: Mutex<RateLimit>,
    last_test: Mutex<Option<Instant>>,
    /// Built on the first push, so the app start does not set up TLS for a feature that is off.
    client: Mutex<Option<reqwest::Client>>,
}

impl PushService {
    pub fn new(config_dir: &Path, store: Arc<dyn SecretStore>) -> Self {
        Self {
            config_dir: config_dir.to_owned(),
            store,
            config: Mutex::new(load_config(config_dir)),
            debounce: Mutex::default(),
            rate: Mutex::default(),
            last_test: Mutex::default(),
            client: Mutex::default(),
        }
    }

    /// The settings and which secrets are stored. Asks the keychain: blocking.
    pub fn view(&self) -> Result<PushView, PushError> {
        let config = lock(&self.config).clone();
        Ok(PushView {
            provider: config.provider,
            ntfy_url: config.ntfy_url,
            include_title: config.include_title,
            events: config.events,
            ntfy_token: self.store.exists(NTFY_TOKEN)?,
            pushover_user: self.store.exists(PUSHOVER_USER)?,
            pushover_token: self.store.exists(PUSHOVER_TOKEN)?,
        })
    }

    /// Checks everything first, then writes the keychain and `push.json`. A chosen provider must
    /// be complete: an ntfy address, or both Pushover keys. A kept ntfy token must belong to the
    /// address's origin, and without a token the topic must be long enough. Blocking.
    pub fn save(&self, input: PushInput) -> Result<PushView, PushError> {
        let address = input.ntfy_url.trim().to_owned();
        let parsed = if address.is_empty() {
            None
        } else {
            Some(ntfy_url(&address)?)
        };
        let token = secret_change(input.ntfy_token.as_deref(), check_token)?;
        let user = secret_change(input.pushover_user.as_deref(), check_key)?;
        let app = secret_change(input.pushover_token.as_deref(), check_key)?;
        let stored = |change: &Change<'_>, account: &str| -> Result<bool, PushError> {
            Ok(match change {
                Change::Keep => self.store.exists(account)?,
                Change::Remove => false,
                Change::Set(_) => true,
            })
        };
        let current = lock(&self.config).clone();
        let new_origin = parsed.as_ref().map(origin).unwrap_or_default();
        let token_stored = stored(&token, NTFY_TOKEN)?;
        let token_origin = match &token {
            Change::Set(_) if parsed.is_none() => return Err(PushError::Incomplete),
            Change::Set(_) => new_origin,
            Change::Remove => String::new(),
            Change::Keep => {
                // A token must never travel to a server it was not entered for.
                if token_stored && parsed.is_some() && current.ntfy_token_origin != new_origin {
                    return Err(PushError::TokenOrigin);
                }
                current.ntfy_token_origin
            }
        };
        let complete = match input.provider {
            Provider::Off => true,
            Provider::Ntfy => parsed.is_some(),
            Provider::Pushover => stored(&user, PUSHOVER_USER)? && stored(&app, PUSHOVER_TOKEN)?,
        };
        if !complete {
            return Err(PushError::Incomplete);
        }
        if input.provider == Provider::Ntfy
            && !token_stored
            && parsed
                .as_ref()
                .is_some_and(|url| topic(url).len() < MIN_OPEN_TOPIC_CHARS)
        {
            return Err(PushError::WeakTopic);
        }
        for (change, account) in [
            (&token, NTFY_TOKEN),
            (&user, PUSHOVER_USER),
            (&app, PUSHOVER_TOKEN),
        ] {
            match change {
                Change::Keep => {}
                Change::Remove => self.store.delete(account)?,
                Change::Set(value) => self.store.set(account, value)?,
            }
        }
        let config = PushConfig {
            provider: input.provider,
            ntfy_url: address,
            ntfy_token_origin: token_origin,
            include_title: input.include_title,
            events: input.events,
        };
        save_config(&self.config_dir, &config).map_err(|e| PushError::Storage(e.to_string()))?;
        *lock(&self.config) = config;
        self.view()
    }

    /// The push for a session's new state, or `None` when none is due: no provider, the event
    /// is not chosen, this session had a push less than [`MIN_GAP`] ago, or [`MAX_PER_MINUTE`]
    /// went out in the last minute. The message is `template` with the session's program and
    /// number, or its name when the user allowed that. Blocking.
    pub fn for_session(
        &self,
        id: SessionId,
        event: PushEvent,
        template: &str,
        names: &SessionNames,
        now: Instant,
    ) -> Result<Option<PushRequest>, PushError> {
        let config = lock(&self.config).clone();
        if config.provider == Provider::Off || !config.events.allows(event) {
            return Ok(None);
        }
        let name = if config.include_title {
            &names.title
        } else {
            &names.program
        };
        let text = fill(template, name)?;
        if !lock(&self.debounce).allow(id, now) || !lock(&self.rate).allow(now) {
            return Ok(None);
        }
        self.request(&config, &text).map(Some)
    }

    /// A test push with the saved settings, whatever the events say; at most one every
    /// [`TEST_GAP`] and within [`MAX_PER_MINUTE`]. Blocking.
    pub fn for_test(&self, text: &str, now: Instant) -> Result<PushRequest, PushError> {
        let config = lock(&self.config).clone();
        if config.provider == Provider::Off {
            return Err(PushError::Incomplete);
        }
        {
            let mut last = lock(&self.last_test);
            if last.is_some_and(|at| now.saturating_duration_since(at) < TEST_GAP) {
                return Err(PushError::RateLimited);
            }
            if !lock(&self.rate).allow(now) {
                return Err(PushError::RateLimited);
            }
            *last = Some(now);
        }
        self.request(&config, text)
    }

    fn request(&self, config: &PushConfig, text: &str) -> Result<PushRequest, PushError> {
        match config.provider {
            Provider::Off => Err(PushError::Incomplete),
            Provider::Ntfy => {
                let url = ntfy_url(&config.ntfy_url)?;
                let token = self.store.get(NTFY_TOKEN)?;
                // Also when push.json was changed by hand: the token stays with its server.
                if token.is_some() && origin(&url) != config.ntfy_token_origin {
                    return Err(PushError::TokenOrigin);
                }
                Ok(ntfy_request(url, token, text))
            }
            Provider::Pushover => {
                let user = self
                    .store
                    .get(PUSHOVER_USER)?
                    .ok_or(PushError::Incomplete)?;
                let token = self
                    .store
                    .get(PUSHOVER_TOKEN)?
                    .ok_or(PushError::Incomplete)?;
                let endpoint = Url::parse(PUSHOVER_ENDPOINT).map_err(|_| PushError::InvalidUrl)?;
                Ok(pushover_request(endpoint, &user, &token, text))
            }
        }
    }

    /// The shared client, built on first use.
    pub fn client(&self) -> Result<reqwest::Client, PushError> {
        let mut slot = lock(&self.client);
        if let Some(client) = slot.as_ref() {
            return Ok(client.clone());
        }
        let client = client()?;
        *slot = Some(client.clone());
        Ok(client)
    }
}

enum Change<'a> {
    Keep,
    Remove,
    Set(&'a str),
}

fn secret_change<'a>(
    value: Option<&'a str>,
    check: fn(&str) -> Result<(), PushError>,
) -> Result<Change<'a>, PushError> {
    match value.map(str::trim) {
        None => Ok(Change::Keep),
        Some("") => Ok(Change::Remove),
        Some(value) => check(value).map(|()| Change::Set(value)),
    }
}

/// `push.json`, read leniently: a missing or broken file means push is off.
pub fn load_config(config_dir: &Path) -> PushConfig {
    std::fs::read(config_dir.join(CONFIG_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save_config(config_dir: &Path, config: &PushConfig) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    let text = serde_json::to_vec_pretty(config).map_err(std::io::Error::other)?;
    crate::persist::write_atomic(&config_dir.join(CONFIG_FILE), &text)
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    use super::*;
    use crate::test_support::TempDir;

    /// In-memory keychain; Debug never shows values.
    #[derive(Default)]
    struct MemoryStore {
        values: Mutex<HashMap<String, Zeroizing<String>>>,
    }

    impl SecretStore for MemoryStore {
        fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, SecretError> {
            Ok(lock(&self.values).get(account).cloned())
        }
        fn exists(&self, account: &str) -> Result<bool, SecretError> {
            Ok(lock(&self.values).contains_key(account))
        }
        fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
            lock(&self.values).insert(account.to_owned(), Zeroizing::new(value.to_owned()));
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<(), SecretError> {
            lock(&self.values).remove(account);
            Ok(())
        }
    }

    /// A long enough topic for ntfy without a token.
    const OPEN: &str = "https://ntfy.sh/hangar-AbCdEfGhIjKl";

    fn input(provider: Provider) -> PushInput {
        PushInput {
            provider,
            ntfy_url: String::new(),
            include_title: false,
            events: Events::default(),
            ntfy_token: None,
            pushover_user: None,
            pushover_token: None,
        }
    }

    fn ntfy(url: &str, token: Option<&str>) -> PushInput {
        let mut input = input(Provider::Ntfy);
        input.ntfy_url = url.to_owned();
        input.ntfy_token = token.map(str::to_owned);
        input
    }

    fn pushover(user: Option<&str>, app: Option<&str>) -> PushInput {
        let mut input = input(Provider::Pushover);
        input.pushover_user = user.map(str::to_owned);
        input.pushover_token = app.map(str::to_owned);
        input
    }

    fn names() -> SessionNames {
        SessionNames {
            program: "Claude Code (2)".to_owned(),
            title: "fix the login bug".to_owned(),
        }
    }

    fn service(dir: &TempDir) -> (PushService, Arc<MemoryStore>) {
        let store = Arc::new(MemoryStore::default());
        (PushService::new(dir.path(), store.clone()), store)
    }

    fn body(request: &PushRequest) -> &str {
        std::str::from_utf8(&request.body).unwrap()
    }

    #[test]
    fn ntfy_urls_must_be_https_with_a_topic() {
        assert!(ntfy_url("https://ntfy.sh/hangar_Alerts-42").is_ok());
        assert!(ntfy_url("  https://push.example.org/ntfy/topic  ").is_ok());
        for bad in [
            "",
            "ntfy.sh/topic",
            "http://ntfy.sh/topic",
            "https://ntfy.sh",
            "https://ntfy.sh/",
            "https://ntfy.sh/topic/",
            "https://ntfy.sh/to pic",
            "https://ntfy.sh/topic?title=x",
            "https://ntfy.sh/topic#x",
            "https://user:pass@ntfy.sh/topic",
            "https://ntfy.sh/t%C3%B6pic",
            "file:///etc/topic",
        ] {
            assert!(ntfy_url(bad).is_err(), "{bad} accepted");
        }
        let long = format!("https://ntfy.sh/{}", "a".repeat(65));
        assert!(ntfy_url(&long).is_err());
    }

    #[test]
    fn tokens_and_keys_are_checked() {
        assert!(check_token("tk_abc123").is_ok());
        assert!(check_token("tk abc").is_err());
        assert!(check_token("").is_err());
        assert!(check_key("azGDORePK8gMaC0QOYAMyEEuzJnyUi").is_ok());
        assert!(check_key("abc-123").is_err());
    }

    #[test]
    fn text_has_no_control_characters_and_a_limit() {
        assert_eq!(
            clean_text("  Build\u{1b}[31m is\nwaiting  "),
            "Build [31m is waiting"
        );
        assert_eq!(clean_text("\n\t"), "Hangar");
        assert_eq!(clean_text(&"x".repeat(500)).chars().count(), MAX_TEXT_CHARS);
    }

    #[test]
    fn templates_take_exactly_one_name() {
        assert_eq!(
            fill("{name} wartet auf dich", "Claude Code (3)").unwrap(),
            "Claude Code (3) wartet auf dich"
        );
        for bad in [
            "waiting",
            "{name} {name}",
            "{name}\nsecret",
            &format!("{{name}}{}", "x".repeat(200)),
        ] {
            assert!(
                matches!(fill(bad, "x"), Err(PushError::InvalidTemplate)),
                "{bad}"
            );
        }
    }

    #[test]
    fn sessions_are_named_by_program_and_number() {
        let sessions = vec![
            (7, "claude".to_owned(), "fix login".to_owned()),
            (3, "claude".to_owned(), "Claude Code".to_owned()),
            (5, "pwsh".to_owned(), "PowerShell".to_owned()),
        ];
        let programs = HashMap::from([("claude".to_owned(), "Claude Code".to_owned())]);
        let named = session_names(&sessions, &programs, 7).unwrap();
        assert_eq!(named.program, "Claude Code (2)");
        assert_eq!(named.title, "fix login");
        assert_eq!(
            session_names(&sessions, &programs, 5).unwrap().program,
            "pwsh (1)"
        );
        assert!(session_names(&sessions, &programs, 9).is_none());
    }

    #[test]
    fn ntfy_request_carries_text_headers_and_token() {
        let url = ntfy_url("https://ntfy.sh/topic").unwrap();
        let request = ntfy_request(
            url.clone(),
            Some(Zeroizing::new("tk_x".to_owned())),
            "api waits for you",
        );
        assert_eq!(request.url, url);
        assert_eq!(request.content_type, "text/plain; charset=utf-8");
        assert_eq!(request.bearer.as_deref().map(String::as_str), Some("tk_x"));
        assert_eq!(request.headers, vec![("Title", "Hangar")]);
        assert_eq!(body(&request), "api waits for you");
    }

    #[test]
    fn pushover_request_is_one_form() {
        let endpoint = Url::parse(PUSHOVER_ENDPOINT).unwrap();
        let request = pushover_request(endpoint, "user1", "app1", "web & api: done");
        assert_eq!(request.url.as_str(), PUSHOVER_ENDPOINT);
        assert_eq!(request.content_type, "application/x-www-form-urlencoded");
        assert!(request.bearer.is_none());
        assert_eq!(
            body(&request),
            "token=app1&user=user1&title=Hangar&message=web+%26+api%3A+done"
        );
    }

    #[test]
    fn one_push_per_session_every_thirty_seconds() {
        let mut debounce = Debounce::default();
        let start = Instant::now();
        assert!(debounce.allow(1, start));
        assert!(!debounce.allow(1, start + Duration::from_secs(29)));
        assert!(
            debounce.allow(2, start + Duration::from_secs(29)),
            "per session"
        );
        assert!(debounce.allow(1, start + MIN_GAP));
        assert!(!debounce.allow(1, start + MIN_GAP + Duration::from_secs(1)));
    }

    #[test]
    fn ten_pushes_a_minute_in_all() {
        let mut rate = RateLimit::default();
        let start = Instant::now();
        for second in 0..10 {
            assert!(rate.allow(start + Duration::from_secs(second)));
        }
        assert!(!rate.allow(start + Duration::from_secs(59)));
        assert!(
            rate.allow(start + Duration::from_secs(60)),
            "the first one aged out"
        );
        assert!(!rate.allow(start + Duration::from_secs(60)));
    }

    #[test]
    fn input_debug_hides_secrets() {
        let input = ntfy(OPEN, Some("tk_secret"));
        let shown = format!("{input:?}");
        assert!(!shown.contains("tk_secret"), "{shown}");
        assert!(shown.contains("<redacted>"), "{shown}");
    }

    #[test]
    fn off_by_default_and_nothing_is_due() {
        let dir = TempDir::new("push-off");
        let (push, _) = service(&dir);
        let view = push.view().unwrap();
        assert_eq!(view.provider, Provider::Off);
        assert!(!view.include_title);
        assert!(!view.ntfy_token && !view.pushover_user && !view.pushover_token);
        let due = push
            .for_session(1, PushEvent::Waiting, "{name}", &names(), Instant::now())
            .unwrap();
        assert!(due.is_none());
        assert!(matches!(
            push.for_test("x", Instant::now()),
            Err(PushError::Incomplete)
        ));
    }

    #[test]
    fn secrets_go_to_the_keychain_only() {
        let dir = TempDir::new("push-secrets");
        let (push, store) = service(&dir);
        let view = push
            .save(ntfy("https://ntfy.sh/topic", Some("tk_secret")))
            .unwrap();
        assert!(view.ntfy_token);
        let file = std::fs::read_to_string(dir.path().join(CONFIG_FILE)).unwrap();
        assert!(!file.contains("tk_secret"), "{file}");
        assert!(file.contains("https://ntfy.sh/topic"));
        assert_eq!(
            store
                .get(NTFY_TOKEN)
                .unwrap()
                .as_deref()
                .map(String::as_str),
            Some("tk_secret")
        );
        let shown = serde_json::to_string(&view).unwrap();
        assert!(!shown.contains("tk_secret"), "{shown}");
        // None keeps it (same server, so a short topic is fine with the token).
        let view = push.save(ntfy("https://ntfy.sh/other", None)).unwrap();
        assert!(view.ntfy_token);
        // An empty text removes it; then the short topic is too weak.
        assert!(matches!(
            push.save(ntfy("https://ntfy.sh/topic", Some(""))),
            Err(PushError::WeakTopic)
        ));
        let view = push.save(ntfy(OPEN, Some(""))).unwrap();
        assert!(!view.ntfy_token);
        // The settings come back after a restart.
        let (again, _) = service(&dir);
        assert_eq!(again.view().unwrap().provider, Provider::Ntfy);
    }

    #[test]
    fn a_kept_token_stays_with_its_server() {
        let dir = TempDir::new("push-origin");
        let (push, _) = service(&dir);
        push.save(ntfy("https://ntfy.sh/topic", Some("tk_secret")))
            .unwrap();
        for other in [
            "https://evil.example/topic",
            "https://ntfy.sh:8443/topic",
            "https://push.ntfy.sh/topic",
        ] {
            assert!(
                matches!(push.save(ntfy(other, None)), Err(PushError::TokenOrigin)),
                "{other}"
            );
        }
        // Entered again for the new server, it is accepted and bound to that one.
        push.save(ntfy("https://push.example.org/topic", Some("tk_new")))
            .unwrap();
        let request = push.for_test("x", Instant::now()).unwrap();
        assert_eq!(request.url.host_str(), Some("push.example.org"));
        // A hand-edited push.json with another server does not get the token either.
        let mut config = load_config(dir.path());
        config.ntfy_url = "https://evil.example/topic".to_owned();
        save_config(dir.path(), &config).unwrap();
        let edited = PushService::new(dir.path(), push.store.clone());
        assert!(matches!(
            edited.for_test("x", Instant::now()),
            Err(PushError::TokenOrigin)
        ));
    }

    #[test]
    fn a_provider_must_be_complete_and_nothing_is_written_otherwise() {
        let dir = TempDir::new("push-incomplete");
        let (push, store) = service(&dir);
        assert!(matches!(
            push.save(input(Provider::Ntfy)),
            Err(PushError::Incomplete)
        ));
        assert!(matches!(
            push.save(pushover(Some("user1"), None)),
            Err(PushError::Incomplete)
        ));
        assert!(
            !store.exists(PUSHOVER_USER).unwrap(),
            "checked before writing"
        );
        assert!(matches!(
            push.save(ntfy("http://ntfy.sh/topic", None)),
            Err(PushError::InvalidUrl)
        ));
        assert!(matches!(
            push.save(ntfy("https://ntfy.sh/short", None)),
            Err(PushError::WeakTopic)
        ));
        assert!(matches!(
            push.save(pushover(Some("bad key"), Some("app1"))),
            Err(PushError::InvalidKey)
        ));
        assert!(!dir.path().join(CONFIG_FILE).exists());
    }

    #[test]
    fn session_pushes_follow_events_and_the_debounce() {
        let dir = TempDir::new("push-events");
        let (push, _) = service(&dir);
        let mut settings = pushover(Some("user1"), Some("app1"));
        settings.events = Events {
            waiting: true,
            done: false,
            failed: true,
        };
        push.save(settings).unwrap();
        let start = Instant::now();
        let due = |id, event, at| {
            push.for_session(id, event, "{name}", &names(), at)
                .unwrap()
                .is_some()
        };
        assert!(!due(1, PushEvent::Done, start), "done is not chosen");
        assert!(due(1, PushEvent::Waiting, start));
        assert!(!due(1, PushEvent::Failed, start + Duration::from_secs(5)));
        assert!(due(1, PushEvent::Failed, start + MIN_GAP));
        assert!(due(2, PushEvent::Waiting, start));
    }

    #[test]
    fn the_message_names_the_program_unless_the_name_is_allowed() {
        let dir = TempDir::new("push-names");
        let (push, _) = service(&dir);
        push.save(ntfy(OPEN, None)).unwrap();
        let start = Instant::now();
        let request = push
            .for_session(1, PushEvent::Waiting, "{name} waits", &names(), start)
            .unwrap()
            .unwrap();
        assert_eq!(body(&request), "Claude Code (2) waits");
        let mut settings = ntfy(OPEN, None);
        settings.include_title = true;
        assert!(push.save(settings).unwrap().include_title);
        let request = push
            .for_session(2, PushEvent::Waiting, "{name} waits", &names(), start)
            .unwrap()
            .unwrap();
        assert_eq!(body(&request), "fix the login bug waits");
    }

    #[test]
    fn tests_are_limited_too() {
        let dir = TempDir::new("push-tests");
        let (push, _) = service(&dir);
        push.save(ntfy(OPEN, None)).unwrap();
        let start = Instant::now();
        assert!(push.for_test("x", start).is_ok());
        assert!(matches!(
            push.for_test("x", start + Duration::from_secs(4)),
            Err(PushError::RateLimited)
        ));
        assert!(push.for_test("x", start + TEST_GAP).is_ok());
        // Ten in the minute, tests included: the session pushes after that wait.
        for id in 0..8 {
            assert!(push
                .for_session(id, PushEvent::Waiting, "{name}", &names(), start + TEST_GAP)
                .unwrap()
                .is_some());
        }
        assert!(push
            .for_session(9, PushEvent::Waiting, "{name}", &names(), start + TEST_GAP)
            .unwrap()
            .is_none());
    }

    #[test]
    fn errors_reach_the_window_as_kinds() {
        let text = serde_json::to_string(&PushError::Rejected(500)).unwrap();
        assert_eq!(text, "\"rejected\"");
        assert_eq!(
            serde_json::to_string(&PushError::TokenOrigin).unwrap(),
            "\"tokenOrigin\""
        );
        assert_eq!(answer(StatusCode::OK).ok(), Some(()));
        assert!(matches!(
            answer(StatusCode::FORBIDDEN),
            Err(PushError::Unauthorized)
        ));
        assert!(matches!(
            answer(StatusCode::TOO_MANY_REQUESTS),
            Err(PushError::Rejected(429))
        ));
    }

    /// One request on 127.0.0.1: its head and body, answered with `status`.
    fn serve_once(listener: TcpListener, status: u16) -> std::thread::JoinHandle<(String, String)> {
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                if line == "\r\n" {
                    break;
                }
                head.push_str(&line);
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let mut stream = stream;
            write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            (head, String::from_utf8(body).unwrap())
        })
    }

    #[test]
    fn delivers_to_a_local_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = Url::parse(&format!("http://{}/topic", listener.local_addr().unwrap())).unwrap();
        let server = serve_once(listener, 200);
        let client = build_client(false).unwrap();
        let request = ntfy_request(url, Some(Zeroizing::new("tk_x".to_owned())), "api: waiting");
        tauri::async_runtime::block_on(deliver(&client, request)).unwrap();
        let (head, body) = server.join().unwrap();
        assert!(head.starts_with("POST /topic HTTP/1.1"), "{head}");
        assert!(head.contains("authorization: Bearer tk_x"), "{head}");
        assert!(head.contains("title: Hangar"), "{head}");
        assert!(
            !head.contains("cache:"),
            "the server cache stays on: {head}"
        );
        assert_eq!(body, "api: waiting");
    }

    #[test]
    fn a_refusal_is_reported() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = Url::parse(&format!("http://{}/topic", listener.local_addr().unwrap())).unwrap();
        let server = serve_once(listener, 401);
        let client = build_client(false).unwrap();
        let result = tauri::async_runtime::block_on(deliver(&client, ntfy_request(url, None, "x")));
        assert!(matches!(result, Err(PushError::Unauthorized)));
        server.join().unwrap();
    }

    #[test]
    fn the_real_client_refuses_plain_http() {
        // Bound but never accepted: the client must refuse before it connects.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = Url::parse(&format!("http://{}/topic", listener.local_addr().unwrap())).unwrap();
        let result = tauri::async_runtime::block_on(deliver(
            &client().unwrap(),
            ntfy_request(url, None, "x"),
        ));
        assert!(matches!(result, Err(PushError::Network)));
    }
}

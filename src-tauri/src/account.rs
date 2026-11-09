//! Hangar account, pure part (team service step F, part 1; contract:
//! docs/wiki/pages/components/hangar-account-api.md).
//!
//! The app signs in to a Hangar account through the OAuth device flow (RFC 8628), keeps the
//! refresh token in the OS keychain and lists the account's projects. This module holds what can
//! be decided without I/O: the server address and its origin checks, the wire types, problem
//! documents (RFC 9457), the polling rules of the device flow and the state the window sees.
//! HTTP lives in `account_api.rs`, tokens and the flow in `account_service.rs`.
//!
//! No token ever appears in `Debug`, `Display`, an error, a log line or an event.

use std::fmt;
use std::path::Path;

use reqwest::Url;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroizing;

/// The official service. Self-hosted servers are set in Settings ([`BaseUrl::parse`]).
pub const OFFICIAL_BASE: &str = "https://hangar.feoren.com/api/v1";
pub const CLIENT_ID: &str = "hangar-desktop";
pub const SCOPE: &str = "team";
/// Event to the main window with every new [`AccountView`].
pub const EVENT: &str = "account-changed";
/// Keychain service of the refresh token; the account name is the server address. Separate from
/// the profile secrets (`secrets::SERVICE`), so no profile variable can share its entry.
pub const KEYCHAIN_SERVICE: &str = "hangar-account";
/// The chosen server address, in the config folder.
pub const CONFIG_FILE: &str = "account.json";

const MAX_BASE_CHARS: usize = 512;
/// Tokens go into an `Authorization` header and the refresh token into the keychain, whose
/// Windows store holds at most 1280 UTF-16 units (`secrets::MAX_VALUE_UNITS`).
const MAX_TOKEN_CHARS: usize = 1024;
const MAX_TEXT_CHARS: usize = 300;
const MAX_USER_CODE_CHARS: usize = 32;

// Device flow timing, in seconds (RFC 8628 3.2 and 3.5).
pub const DEFAULT_INTERVAL: u64 = 5;
pub const MIN_INTERVAL: u64 = 1;
/// A server asking for more than this is still polled once a minute or so.
pub const MAX_INTERVAL: u64 = 120;
/// `slow_down` adds this to the interval for all further requests.
pub const SLOW_DOWN_STEP: u64 = 5;
/// The longest code lifetime Hangar accepts; the official service gives 600 s.
pub const MAX_EXPIRES_IN: u64 = 1800;
/// After a network error the next request waits longer, up to this.
const MAX_BACKOFF: u64 = 60;

// ---------------------------------------------------------------------------------------------
// Server address
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BaseUrlError {
    #[error("The server address is not a valid URL")]
    Invalid,
    #[error("The server address must use https (http only for 127.0.0.1 or localhost)")]
    Insecure,
    #[error("The server address must not contain a user name, password, query or fragment")]
    Extra,
    #[error("The server address is too long")]
    TooLong,
}

/// The API base of a Hangar server, for example `https://hangar.feoren.com/api/v1`: https, or
/// plain http only on the loopback for development. Kept without a trailing slash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseUrl {
    url: Url,
}

impl BaseUrl {
    pub fn official() -> Self {
        // Invariant: OFFICIAL_BASE is a valid https base (test `official_base_is_valid`).
        Self::parse(OFFICIAL_BASE).expect("OFFICIAL_BASE is a valid base URL")
    }

    pub fn parse(text: &str) -> Result<Self, BaseUrlError> {
        let text = text.trim();
        if text.chars().count() > MAX_BASE_CHARS {
            return Err(BaseUrlError::TooLong);
        }
        let mut url = Url::parse(text).map_err(|_| BaseUrlError::Invalid)?;
        if url.host_str().is_none_or(str::is_empty) {
            return Err(BaseUrlError::Invalid);
        }
        match url.scheme() {
            "https" => {}
            "http" if is_loopback(&url) => {}
            "http" => return Err(BaseUrlError::Insecure),
            _ => return Err(BaseUrlError::Invalid),
        }
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(BaseUrlError::Extra);
        }
        let path = url.path().trim_end_matches('/').to_owned();
        url.set_path(&path);
        Ok(Self { url })
    }

    /// The address as the user sees it, without a trailing slash.
    pub fn as_str(&self) -> &str {
        self.url.as_str().trim_end_matches('/')
    }

    pub fn is_official(&self) -> bool {
        self.as_str() == OFFICIAL_BASE
    }

    pub fn is_https(&self) -> bool {
        self.url.scheme() == "https"
    }

    /// `<base>/<path>`, for example `oauth/token`.
    pub fn endpoint(&self, path: &str) -> Url {
        let mut url = self.url.clone();
        let base = self.url.path().trim_end_matches('/');
        url.set_path(&format!("{base}/{}", path.trim_start_matches('/')));
        url
    }

    /// The account page that lists and revokes connected devices.
    pub fn devices_page(&self) -> Url {
        let mut url = self.url.clone();
        url.set_path("/account/devices");
        url
    }

    /// `text` as a URL when it is an http(s) page on exactly this server's origin (scheme, host
    /// and port) without credentials; anything else is refused, so the server cannot make
    /// Hangar open another site.
    pub fn same_origin(&self, text: &str) -> Option<Url> {
        if text.chars().count() > MAX_BASE_CHARS * 2 {
            return None;
        }
        let url = Url::parse(text).ok()?;
        let fine = matches!(url.scheme(), "https" | "http")
            && url.origin() == self.url.origin()
            && url.username().is_empty()
            && url.password().is_none();
        fine.then_some(url)
    }
}

impl Default for BaseUrl {
    fn default() -> Self {
        Self::official()
    }
}

/// `localhost`, `127.0.0.1` or `[::1]` (the url crate lower-cases hosts and writes IPv6 in
/// brackets).
fn is_loopback(url: &Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

/// The stored server address; the official one when the file is missing or broken.
pub fn load_base(config_dir: &Path) -> BaseUrl {
    let Ok(text) = std::fs::read_to_string(config_dir.join(CONFIG_FILE)) else {
        return BaseUrl::official();
    };
    serde_json::from_str::<AccountConfig>(&text)
        .ok()
        .and_then(|config| BaseUrl::parse(&config.base_url).ok())
        .unwrap_or_default()
}

pub fn save_base(config_dir: &Path, base: &BaseUrl) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    let config = AccountConfig {
        base_url: base.as_str().to_owned(),
    };
    let text = serde_json::to_vec_pretty(&config).map_err(std::io::Error::other)?;
    crate::persist::write_atomic(&config_dir.join(CONFIG_FILE), &text)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountConfig {
    base_url: String,
}

// ---------------------------------------------------------------------------------------------
// Problems and OAuth errors
// ---------------------------------------------------------------------------------------------

/// The fields of an RFC 9457 problem document Hangar reads, plus the OAuth error fields
/// (RFC 6749 5.2) the token endpoint adds. Read leniently: a field of the wrong type is missing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Problem {
    pub kind: Option<String>,
    pub title: Option<String>,
    pub status: Option<u16>,
    pub detail: Option<String>,
    pub code: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

impl Problem {
    /// `None` when the body is no JSON object.
    pub fn parse(body: &[u8]) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_slice(body).ok()?;
        let object = value.as_object()?;
        let text = |key: &str| object.get(key).and_then(|v| v.as_str()).map(clean_text);
        Some(Self {
            kind: text("type"),
            title: text("title"),
            status: object
                .get("status")
                .and_then(serde_json::Value::as_u64)
                .and_then(|status| u16::try_from(status).ok()),
            detail: text("detail"),
            code: text("code"),
            error: text("error"),
            error_description: text("error_description"),
        })
    }

    /// The most specific readable text: detail, then the OAuth description, then the title.
    pub fn message(&self) -> Option<String> {
        self.detail
            .clone()
            .or_else(|| self.error_description.clone())
            .or_else(|| self.title.clone())
            .filter(|text| !text.is_empty())
    }
}

/// Server text for the window: control characters and bidirectional marks dropped (they could
/// reorder what the person reads), at most [`MAX_TEXT_CHARS`].
pub fn clean_text(text: &str) -> String {
    text.chars()
        .filter(|&c| {
            !c.is_control()
                && !matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(MAX_TEXT_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// OAuth error codes of the token endpoint (RFC 6749 5.2, RFC 8628 3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthCode {
    AuthorizationPending,
    SlowDown,
    AccessDenied,
    ExpiredToken,
    InvalidGrant,
    InvalidClient,
    InvalidScope,
    InvalidRequest,
    UnsupportedGrantType,
    Other,
}

impl OAuthCode {
    pub fn parse(code: &str) -> Self {
        match code {
            "authorization_pending" => Self::AuthorizationPending,
            "slow_down" => Self::SlowDown,
            "access_denied" => Self::AccessDenied,
            "expired_token" => Self::ExpiredToken,
            "invalid_grant" => Self::InvalidGrant,
            "invalid_client" => Self::InvalidClient,
            "invalid_scope" => Self::InvalidScope,
            "invalid_request" => Self::InvalidRequest,
            "unsupported_grant_type" => Self::UnsupportedGrantType,
            _ => Self::Other,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------------------------

/// Answer of `POST /oauth/device`, checked. The device code is the polling credential and
/// never leaves the backend.
pub struct DeviceAuthorization {
    pub device_code: Zeroizing<String>,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: Option<u64>,
}

impl fmt::Debug for DeviceAuthorization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceAuthorization")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("expires_in", &self.expires_in)
            .field("interval", &self.interval)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct RawDeviceAuthorization {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    expires_in: u64,
    interval: Option<u64>,
}

impl DeviceAuthorization {
    pub fn parse(body: &[u8]) -> Option<Self> {
        let raw: RawDeviceAuthorization = serde_json::from_slice(body).ok()?;
        let device_code = Zeroizing::new(raw.device_code);
        let fine = is_token_text(&device_code)
            && is_user_code(&raw.user_code)
            && raw.expires_in > 0
            && !raw.verification_uri.is_empty();
        fine.then(|| Self {
            device_code,
            user_code: raw.user_code,
            verification_uri: raw.verification_uri,
            verification_uri_complete: raw.verification_uri_complete,
            expires_in: raw.expires_in.min(MAX_EXPIRES_IN),
            interval: raw.interval,
        })
    }
}

/// `XXXX-XXXX` on the official service; generally short, upper-case letters, digits and `-`.
pub fn is_user_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= MAX_USER_CODE_CHARS
        && code
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
}

/// A token as it may go into a header: visible ASCII without spaces, bounded.
fn is_token_text(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAX_TOKEN_CHARS
        && token.bytes().all(|b| b.is_ascii_graphic())
}

/// A successful token answer (device code or refresh grant), checked.
pub struct TokenSet {
    pub access: Zeroizing<String>,
    /// Missing only when a server does not rotate; the old one stays then.
    pub refresh: Option<Zeroizing<String>>,
    /// Seconds; the official service gives 900.
    pub expires_in: u64,
}

impl fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenSet")
            .field("expires_in", &self.expires_in)
            .field("has_refresh", &self.refresh.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct RawTokenSet {
    access_token: String,
    token_type: String,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
}

impl TokenSet {
    pub fn parse(body: &[u8]) -> Option<Self> {
        let raw: RawTokenSet = serde_json::from_slice(body).ok()?;
        let access = Zeroizing::new(raw.access_token);
        let refresh = raw.refresh_token.map(Zeroizing::new);
        let fine = raw.token_type.eq_ignore_ascii_case("bearer")
            && is_token_text(&access)
            && refresh.as_deref().is_none_or(|r| is_token_text(r));
        fine.then(|| Self {
            access,
            refresh,
            // RFC 6749 leaves it optional; without it, assume the official lifetime.
            expires_in: raw.expires_in.unwrap_or(900),
        })
    }
}

/// `GET /me`: the signed-in person and the seats of their own account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct Me {
    #[serde(deserialize_with = "opaque_id")]
    pub id: String,
    pub email: String,
    #[serde(default, deserialize_with = "optional_cleaned")]
    pub name: Option<String>,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub seats_account_total: Option<u32>,
    #[serde(default)]
    pub seats_account_used: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Owner,
    Admin,
    Member,
    /// A role a newer server knows; shown without a name and with no rights assumed.
    #[serde(other)]
    Other,
}

/// One project of `GET /projects`; a project is a team (glossary). Seats count per account of
/// the project's owner (ADR 0005).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct Project {
    #[serde(deserialize_with = "opaque_id")]
    pub id: String,
    /// Chosen by any owner or admin, so cleaned like server text ([`clean_text`]).
    #[serde(deserialize_with = "cleaned")]
    pub name: String,
    pub role: Role,
    #[serde(default, deserialize_with = "optional_id")]
    pub owner_id: Option<String>,
    #[serde(default, deserialize_with = "optional_cleaned")]
    pub plan: Option<String>,
    #[serde(default)]
    pub seats_account_total: Option<u32>,
    #[serde(default)]
    pub seats_account_used: Option<u32>,
    /// Only owners and admins get it.
    #[serde(default)]
    pub pending_invitations: Option<u32>,
    /// Never sent to the window; it opens through [`AccountPage::Invite`] after the origin check.
    #[serde(default, skip_serializing)]
    pub invite_url: Option<String>,
    /// Set by the backend: an owner or admin with an invite page on the server's own origin.
    #[serde(default, skip_deserializing)]
    pub can_invite: bool,
}

#[derive(Deserialize)]
struct RawProjects {
    projects: Vec<Project>,
}

impl Project {
    /// The list, with `can_invite` decided against `base`.
    pub fn parse_list(body: &[u8], base: &BaseUrl) -> Option<Vec<Self>> {
        let raw: RawProjects = serde_json::from_slice(body).ok()?;
        Some(
            raw.projects
                .into_iter()
                .map(|mut project| {
                    project.can_invite = matches!(project.role, Role::Owner | Role::Admin)
                        && project
                            .invite_url
                            .as_deref()
                            .and_then(|url| base.same_origin(url))
                            .is_some();
                    project
                })
                .collect(),
        )
    }
}

/// `POST /relay-ticket`: a short-lived ticket for the relay (step D). Not used by the window yet.
pub struct RelayTicket {
    pub ticket: Zeroizing<String>,
    pub expires_at: u64,
    pub relay_url: String,
}

impl fmt::Debug for RelayTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelayTicket")
            .field("expires_at", &self.expires_at)
            .field("relay_url", &self.relay_url)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct RawRelayTicket {
    ticket: String,
    expires_at: u64,
    relay_url: String,
}

impl RelayTicket {
    pub fn parse(body: &[u8]) -> Option<Self> {
        let raw: RawRelayTicket = serde_json::from_slice(body).ok()?;
        let ticket = Zeroizing::new(raw.ticket);
        is_token_text(&ticket).then(|| Self {
            ticket,
            expires_at: raw.expires_at,
            relay_url: raw.relay_url,
        })
    }
}

/// Ids are opaque strings (decimal numbers as text); a number is accepted too.
fn opaque_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(text) if !text.is_empty() => Ok(text),
        serde_json::Value::Number(number) => Ok(number.to_string()),
        _ => Err(serde::de::Error::custom("id must be a string")),
    }
}

/// A name the window shows: without control characters and bidirectional marks.
fn cleaned<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    String::deserialize(deserializer).map(|text| clean_text(&text))
}

fn optional_cleaned<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer).map(|text| text.map(|text| clean_text(&text)))
}

fn optional_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(text) => Ok(Some(text)),
        serde_json::Value::Number(number) => Ok(Some(number.to_string())),
        _ => Err(serde::de::Error::custom("id must be a string")),
    }
}

// ---------------------------------------------------------------------------------------------
// Device flow polling
// ---------------------------------------------------------------------------------------------

/// What one token request of the device flow returned, as far as the polling rules care.
pub enum PollResponse {
    Tokens(TokenSet),
    OAuth(OAuthCode, Option<String>),
    /// Network error, timeout, 5xx or 429: try again later.
    Transient,
    /// Anything else the server should not have sent.
    Malformed,
}

#[derive(Debug)]
pub enum PollStep {
    /// Ask again after this many seconds.
    Wait(u64),
    SignedIn(TokenSet),
    Failed(FailReason),
}

/// Polling rules of the device flow (RFC 8628 3.4 and 3.5): wait `interval` seconds between
/// requests, add 5 s for good on `slow_down`, back off after network errors, and stop once the
/// code expires. Time is counted by the caller and passed in, so the rules are testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevicePoll {
    interval: u64,
    expires_in: u64,
    /// Network errors in a row.
    failures: u32,
}

impl DevicePoll {
    pub fn new(interval: Option<u64>, expires_in: u64) -> Self {
        Self {
            interval: interval
                .unwrap_or(DEFAULT_INTERVAL)
                .clamp(MIN_INTERVAL, MAX_INTERVAL),
            expires_in: expires_in.min(MAX_EXPIRES_IN),
            failures: 0,
        }
    }

    pub fn interval(&self) -> u64 {
        self.interval
    }

    /// Seconds to wait before the next request, `None` when the code expires before then.
    pub fn next_wait(&self, elapsed: u64) -> Option<u64> {
        let backoff = if self.failures == 0 {
            0
        } else {
            (self.interval << self.failures.min(4)).min(MAX_BACKOFF)
        };
        let wait = self.interval.max(backoff);
        (elapsed.saturating_add(wait) < self.expires_in).then_some(wait)
    }

    pub fn on_response(&mut self, response: PollResponse) -> PollStep {
        let failed = |kind, detail| PollStep::Failed(FailReason { kind, detail });
        match response {
            PollResponse::Tokens(tokens) => PollStep::SignedIn(tokens),
            PollResponse::OAuth(OAuthCode::AuthorizationPending, _) => {
                self.failures = 0;
                PollStep::Wait(self.interval)
            }
            PollResponse::OAuth(OAuthCode::SlowDown, _) => {
                self.failures = 0;
                self.interval = (self.interval + SLOW_DOWN_STEP).min(MAX_INTERVAL);
                PollStep::Wait(self.interval)
            }
            PollResponse::OAuth(OAuthCode::AccessDenied, _) => failed(FailKind::Denied, None),
            PollResponse::OAuth(OAuthCode::ExpiredToken, _) => failed(FailKind::Expired, None),
            PollResponse::OAuth(_, detail) => failed(FailKind::Refused, detail),
            PollResponse::Transient => {
                self.failures = self.failures.saturating_add(1);
                PollStep::Wait(self.next_wait(0).unwrap_or(self.interval))
            }
            PollResponse::Malformed => failed(FailKind::BadResponse, None),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// What the window sees
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FailKind {
    /// The person declined in the browser.
    Denied,
    /// The code expired before it was confirmed.
    Expired,
    /// The server refused the flow (`invalid_grant`, `invalid_client`, `invalid_scope` ...).
    Refused,
    Network,
    Server,
    /// The server named a page on another origin; Hangar did not open it.
    ForeignPage,
    /// The refresh token could not be stored.
    Keychain,
    BadResponse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailReason {
    pub kind: FailKind,
    /// The server's own text, cleaned ([`clean_text`]); never a token.
    pub detail: Option<String>,
}

/// Why the account is signed out, when the window should say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IdleNotice {
    /// The person signed out on this device.
    SignedOut,
    /// The server no longer accepts the refresh token (revoked device, reuse, expiry).
    SessionEnded,
}

/// The sign-in state machine. Transitions (`account_service.rs`):
/// `Idle -> WaitingForUser` (sign in), `WaitingForUser -> SignedIn | Failed | Idle` (confirmed,
/// declined or expired, cancelled), `Failed -> Idle | WaitingForUser`, `SignedIn -> Idle` (sign
/// out, refresh refused). A token in the keychain at start means `SignedIn`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AccountState {
    Idle {
        notice: Option<IdleNotice>,
    },
    WaitingForUser {
        user_code: String,
        /// The page without the code, to type it in by hand.
        verification_uri: String,
        verification_uri_complete: String,
        /// Unix ms.
        expires_at: u64,
        /// Seconds between two polls.
        interval: u64,
    },
    SignedIn {
        /// `None` until `/me` answered.
        user: Option<Me>,
    },
    Failed {
        reason: FailReason,
    },
}

/// Payload of `account-changed` and answer of the account commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub base_url: String,
    pub official: bool,
    /// Where connected devices are listed and revoked.
    pub devices_url: String,
    pub state: AccountState,
}

impl AccountView {
    pub fn new(base: &BaseUrl, state: AccountState) -> Self {
        Self {
            base_url: base.as_str().to_owned(),
            official: base.is_official(),
            devices_url: base.devices_page().to_string(),
            state,
        }
    }
}

/// Pages the window may open. It never passes a URL itself.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AccountPage {
    /// The device flow's confirmation page ("Open browser again").
    Verification,
    /// The account's connected devices.
    Devices,
    /// A project's invite page, from the last project list.
    Invite { project_id: String },
}

/// Errors of the account commands; the window shows a text per `kind`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AccountError {
    #[error("Not signed in")]
    NotSignedIn,
    #[error("The sign-in ended; sign in again")]
    SessionEnded,
    #[error("The server cannot be reached: {0}")]
    Network(String),
    #[error("The server reported an error")]
    Server(Option<String>),
    #[error("The server sent an unexpected answer")]
    BadResponse,
    #[error("{0}")]
    BaseUrl(BaseUrlError),
    #[error("The page is not on the server's own address")]
    ForeignPage,
    #[error("The keychain failed: {0}")]
    Keychain(String),
    #[error("The browser could not be opened: {0}")]
    Open(String),
    #[error("Unknown project")]
    UnknownProject,
    #[error("No sign-in is waiting")]
    NothingToOpen,
    #[error("Settings could not be saved: {0}")]
    Config(String),
}

impl AccountError {
    fn kind(&self) -> &'static str {
        match self {
            Self::NotSignedIn => "notSignedIn",
            Self::SessionEnded => "sessionEnded",
            Self::Network(_) => "network",
            Self::Server(_) => "server",
            Self::BadResponse => "badResponse",
            Self::BaseUrl(_) => "baseUrl",
            Self::ForeignPage => "foreignPage",
            Self::Keychain(_) => "keychain",
            Self::Open(_) => "open",
            Self::UnknownProject => "unknownProject",
            Self::NothingToOpen => "nothingToOpen",
            Self::Config(_) => "config",
        }
    }

    fn detail(&self) -> Option<String> {
        match self {
            Self::Server(detail) => detail.clone(),
            Self::Network(text) | Self::Keychain(text) | Self::Open(text) | Self::Config(text) => {
                Some(text.clone())
            }
            _ => None,
        }
    }
}

/// `{ kind, detail }`, so the window can show a translated text per kind.
impl Serialize for AccountError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct as _;
        let mut state = serializer.serialize_struct("AccountError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("detail", &self.detail())?;
        state.end()
    }
}

/// Name and platform the device flow sends, shown on the account's device list.
pub fn device_name() -> String {
    let name = ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|key| std::env::var(key).ok())
        .map(|name| clean_text(&name))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Hangar".to_owned());
    name.chars().take(100).collect()
}

pub fn platform() -> &'static str {
    std::env::consts::OS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> BaseUrl {
        BaseUrl::parse("https://example.test/api/v1").unwrap()
    }

    #[test]
    fn official_base_is_valid() {
        let official = BaseUrl::official();
        assert_eq!(official.as_str(), OFFICIAL_BASE);
        assert!(official.is_official());
        assert!(official.is_https());
        assert_eq!(
            official.devices_page().as_str(),
            "https://hangar.feoren.com/account/devices"
        );
    }

    #[test]
    fn base_urls_need_https_except_on_loopback() {
        for good in [
            "https://team.example.test/api/v1",
            "https://team.example.test:8443/api/v1/",
            "  https://example.test  ",
            "http://127.0.0.1:8080/api/v1",
            "http://localhost:3000/api/v1",
            "http://[::1]:3000/api/v1",
        ] {
            assert!(BaseUrl::parse(good).is_ok(), "{good}");
        }
        assert_eq!(
            BaseUrl::parse("http://example.test/api/v1"),
            Err(BaseUrlError::Insecure)
        );
        assert_eq!(
            BaseUrl::parse("http://localhost.example.test/api"),
            Err(BaseUrlError::Insecure)
        );
        for invalid in ["", "example.test", "ftp://example.test", "file:///C:/x"] {
            assert_eq!(
                BaseUrl::parse(invalid),
                Err(BaseUrlError::Invalid),
                "{invalid}"
            );
        }
        for extra in [
            "https://user:pw@example.test/api",
            "https://example.test/api?x=1",
            "https://example.test/api#x",
        ] {
            assert_eq!(BaseUrl::parse(extra), Err(BaseUrlError::Extra), "{extra}");
        }
        let long = format!("https://example.test/{}", "a".repeat(MAX_BASE_CHARS));
        assert_eq!(BaseUrl::parse(&long), Err(BaseUrlError::TooLong));
    }

    #[test]
    fn endpoints_and_pages() {
        let base = BaseUrl::parse("https://example.test/api/v1/").unwrap();
        assert_eq!(base.as_str(), "https://example.test/api/v1");
        assert_eq!(
            base.endpoint("oauth/token").as_str(),
            "https://example.test/api/v1/oauth/token"
        );
        assert_eq!(
            base.devices_page().as_str(),
            "https://example.test/account/devices"
        );
        let root = BaseUrl::parse("https://example.test").unwrap();
        assert_eq!(root.endpoint("/me").as_str(), "https://example.test/me");
    }

    #[test]
    fn only_pages_on_the_same_origin_open() {
        let base = base();
        assert!(base
            .same_origin("https://example.test/device?user_code=BCDF-GHJK")
            .is_some());
        assert!(base
            .same_origin("https://example.test/projects/317#invite")
            .is_some());
        for foreign in [
            "https://evil.test/device",
            "http://example.test/device",
            "https://example.test:444/device",
            "https://example.test.evil.test/device",
            "https://user@example.test/device",
            "javascript:alert(1)",
            "file:///C:/Windows",
            "not a url",
        ] {
            assert!(base.same_origin(foreign).is_none(), "{foreign}");
        }
    }

    #[test]
    fn config_round_trip_and_fallback() {
        let temp = crate::test_support::TempDir::new("account-config");
        let dir = temp.path();
        assert_eq!(load_base(dir), BaseUrl::official());
        let custom = BaseUrl::parse("https://team.example.test/api/v1").unwrap();
        save_base(dir, &custom).unwrap();
        assert_eq!(load_base(dir), custom);
        std::fs::write(
            dir.join(CONFIG_FILE),
            "{\"baseUrl\":\"http://example.test\"}",
        )
        .unwrap();
        assert_eq!(load_base(dir), BaseUrl::official());
        std::fs::write(dir.join(CONFIG_FILE), "broken").unwrap();
        assert_eq!(load_base(dir), BaseUrl::official());
    }

    #[test]
    fn problems_are_read_leniently() {
        let body = br#"{"type":"urn:hangar:api:problem:not_found","title":"Not found",
            "status":404,"detail":"No such\nproject","code":"not_found"}"#;
        let problem = Problem::parse(body).unwrap();
        assert_eq!(problem.status, Some(404));
        assert_eq!(problem.code.as_deref(), Some("not_found"));
        assert_eq!(problem.message().as_deref(), Some("No suchproject"));
        let oauth =
            Problem::parse(br#"{"error":"slow_down","error_description":"Too fast","status":"x"}"#)
                .unwrap();
        assert_eq!(oauth.status, None);
        assert_eq!(
            OAuthCode::parse(oauth.error.as_deref().unwrap()),
            OAuthCode::SlowDown
        );
        assert_eq!(oauth.message().as_deref(), Some("Too fast"));
        let bidi = Problem::parse("{\"detail\":\"a\u{202e}b\u{2066}c\"}".as_bytes()).unwrap();
        assert_eq!(bidi.message().as_deref(), Some("abc"));
        assert!(Problem::parse(b"<html>").is_none());
        assert!(Problem::parse(b"[]").is_none());
        let long = format!(r#"{{"detail":"{}"}}"#, "x".repeat(1000));
        let problem = Problem::parse(long.as_bytes()).unwrap();
        assert_eq!(problem.message().unwrap().chars().count(), MAX_TEXT_CHARS);
    }

    #[test]
    fn oauth_codes() {
        for (text, code) in [
            ("authorization_pending", OAuthCode::AuthorizationPending),
            ("slow_down", OAuthCode::SlowDown),
            ("access_denied", OAuthCode::AccessDenied),
            ("expired_token", OAuthCode::ExpiredToken),
            ("invalid_grant", OAuthCode::InvalidGrant),
            ("invalid_client", OAuthCode::InvalidClient),
            ("invalid_scope", OAuthCode::InvalidScope),
            ("something_new", OAuthCode::Other),
        ] {
            assert_eq!(OAuthCode::parse(text), code);
        }
    }

    const ACCESS: &str = "hgat_secret_access_value";
    const REFRESH: &str = "hgrt_secret_refresh_value";

    #[test]
    fn token_sets_are_checked_and_never_shown() {
        let body = format!(
            r#"{{"access_token":"{ACCESS}","token_type":"Bearer","expires_in":900,
                "refresh_token":"{REFRESH}","scope":"team"}}"#
        );
        let tokens = TokenSet::parse(body.as_bytes()).unwrap();
        assert_eq!(tokens.access.as_str(), ACCESS);
        assert_eq!(tokens.refresh.as_deref().map(String::as_str), Some(REFRESH));
        assert_eq!(tokens.expires_in, 900);
        let debug = format!("{tokens:?}");
        assert!(
            !debug.contains(ACCESS) && !debug.contains(REFRESH),
            "{debug}"
        );
        for bad in [
            r#"{"access_token":"a","token_type":"mac"}"#,
            r#"{"access_token":"","token_type":"Bearer"}"#,
            r#"{"access_token":"a b","token_type":"Bearer"}"#,
            r#"{"access_token":"a","token_type":"Bearer","refresh_token":"x\ny"}"#,
            r#"{"token_type":"Bearer"}"#,
        ] {
            assert!(TokenSet::parse(bad.as_bytes()).is_none(), "{bad}");
        }
    }

    #[test]
    fn device_authorizations_are_checked() {
        let body = br#"{"device_code":"dc_secret","user_code":"BCDF-GHJK",
            "verification_uri":"https://example.test/device",
            "verification_uri_complete":"https://example.test/device?user_code=BCDF-GHJK",
            "expires_in":600,"interval":5}"#;
        let flow = DeviceAuthorization::parse(body).unwrap();
        assert_eq!(flow.user_code, "BCDF-GHJK");
        assert_eq!(flow.interval, Some(5));
        assert!(!format!("{flow:?}").contains("dc_secret"));
        let long =
            br#"{"device_code":"d","user_code":"BCDF","verification_uri":"x","expires_in":99999}"#;
        assert_eq!(
            DeviceAuthorization::parse(long).unwrap().expires_in,
            MAX_EXPIRES_IN
        );
        for bad in [
            r#"{"device_code":"d","user_code":"bcdf-ghjk","verification_uri":"x","expires_in":600}"#,
            r#"{"device_code":"d","user_code":"<b>","verification_uri":"x","expires_in":600}"#,
            r#"{"device_code":"d","user_code":"BCDF","verification_uri":"x","expires_in":0}"#,
            r#"{"device_code":"","user_code":"BCDF","verification_uri":"x","expires_in":600}"#,
        ] {
            assert!(
                DeviceAuthorization::parse(bad.as_bytes()).is_none(),
                "{bad}"
            );
        }
    }

    #[test]
    fn projects_decide_who_may_invite() {
        let body = br#"{"projects":[
            {"id":"317","name":"Core","role":"owner","owner_id":"1","plan":"free",
             "seats_account_total":3,"seats_account_used":2,"pending_invitations":1,
             "invite_url":"https://example.test/projects/317#invite"},
            {"id":318,"name":"Docs","role":"member","owner_id":"9",
             "invite_url":"https://example.test/projects/318#invite"},
            {"id":"319","name":"Elsewhere","role":"admin",
             "invite_url":"https://evil.test/projects/319#invite"},
            {"id":"320","name":"Future","role":"auditor"}]}"#;
        let projects = Project::parse_list(body, &base()).unwrap();
        assert_eq!(projects.len(), 4);
        assert!(projects[0].can_invite);
        assert_eq!(projects[0].seats_account_used, Some(2));
        assert_eq!(projects[1].id, "318");
        assert!(!projects[1].can_invite, "members do not invite");
        assert!(!projects[2].can_invite, "foreign invite pages never open");
        assert_eq!(projects[3].role, Role::Other);
        let json = serde_json::to_value(&projects[0]).unwrap();
        assert_eq!(json["seatsAccountTotal"], 3);
        assert_eq!(json["canInvite"], true);
        assert!(json.get("inviteUrl").is_none() && json.get("invite_url").is_none());
        assert!(Project::parse_list(br#"{"projects":[{"id":true}]}"#, &base()).is_none());
        // Names are typed by people; marks that reorder text never reach the window.
        let spoof =
            "{\"projects\":[{\"id\":\"1\",\"name\":\"Core\u{202e}txt\",\"role\":\"member\",\
                     \"plan\":\"te\u{200f}am\"}]}";
        let projects = Project::parse_list(spoof.as_bytes(), &base()).unwrap();
        assert_eq!(projects[0].name, "Coretxt");
        assert_eq!(projects[0].plan.as_deref(), Some("team"));
    }

    #[test]
    fn me_mirrors_in_camel_case() {
        let me: Me = serde_json::from_slice(
            br#"{"id":"7","email":"ada@example.test","name":"Ada","locale":"de",
                "seats_account_total":3,"seats_account_used":1}"#,
        )
        .unwrap();
        let json = serde_json::to_value(&me).unwrap();
        assert_eq!(json["seatsAccountUsed"], 1);
        assert_eq!(json["email"], "ada@example.test");
        let me: Me = serde_json::from_str(
            "{\"id\":\"7\",\"email\":\"a@example.test\",\"name\":\"A\u{2066}da\"}",
        )
        .unwrap();
        assert_eq!(me.name.as_deref(), Some("Ada"));
    }

    #[test]
    fn relay_tickets_hide_the_ticket() {
        let ticket = RelayTicket::parse(
            br#"{"ticket":"eyJ2IjoxfQ.c2ln","expires_at":1794225660,"relay_url":"wss://relay.example.test"}"#,
        )
        .unwrap();
        assert_eq!(ticket.expires_at, 1_794_225_660);
        assert!(!format!("{ticket:?}").contains("eyJ2"));
    }

    #[test]
    fn polling_honors_interval_and_slow_down() {
        let mut poll = DevicePoll::new(Some(5), 600);
        assert_eq!(poll.next_wait(0), Some(5));
        assert!(matches!(
            poll.on_response(PollResponse::OAuth(OAuthCode::AuthorizationPending, None)),
            PollStep::Wait(5)
        ));
        assert!(matches!(
            poll.on_response(PollResponse::OAuth(OAuthCode::SlowDown, None)),
            PollStep::Wait(10)
        ));
        // The slower interval stays for all further requests.
        assert!(matches!(
            poll.on_response(PollResponse::OAuth(OAuthCode::AuthorizationPending, None)),
            PollStep::Wait(10)
        ));
        assert_eq!(poll.next_wait(100), Some(10));
        for _ in 0..100 {
            poll.on_response(PollResponse::OAuth(OAuthCode::SlowDown, None));
        }
        assert_eq!(poll.interval(), MAX_INTERVAL);
        assert_eq!(DevicePoll::new(None, 600).interval(), DEFAULT_INTERVAL);
        assert_eq!(DevicePoll::new(Some(0), 600).interval(), MIN_INTERVAL);
    }

    #[test]
    fn polling_stops_at_expiry() {
        let poll = DevicePoll::new(Some(5), 600);
        assert_eq!(poll.next_wait(594), Some(5));
        assert_eq!(poll.next_wait(595), None);
        assert_eq!(poll.next_wait(u64::MAX), None);
        let mut poll = DevicePoll::new(Some(5), 600);
        let step = poll.on_response(PollResponse::OAuth(OAuthCode::ExpiredToken, None));
        assert!(matches!(
            step,
            PollStep::Failed(FailReason {
                kind: FailKind::Expired,
                ..
            })
        ));
    }

    #[test]
    fn polling_ends_on_refusal_and_backs_off_on_errors() {
        let mut poll = DevicePoll::new(Some(5), 600);
        for (code, kind) in [
            (OAuthCode::AccessDenied, FailKind::Denied),
            (OAuthCode::InvalidGrant, FailKind::Refused),
            (OAuthCode::InvalidClient, FailKind::Refused),
            (OAuthCode::InvalidScope, FailKind::Refused),
        ] {
            match poll.on_response(PollResponse::OAuth(code, Some("why".into()))) {
                PollStep::Failed(reason) => assert_eq!(reason.kind, kind),
                other => panic!("{other:?}"),
            }
        }
        assert!(matches!(
            poll.on_response(PollResponse::Transient),
            PollStep::Wait(10)
        ));
        assert!(matches!(
            poll.on_response(PollResponse::Transient),
            PollStep::Wait(20)
        ));
        for _ in 0..10 {
            poll.on_response(PollResponse::Transient);
        }
        assert_eq!(poll.next_wait(0), Some(MAX_BACKOFF));
        // An answer from the server resets the backoff.
        poll.on_response(PollResponse::OAuth(OAuthCode::AuthorizationPending, None));
        assert_eq!(poll.next_wait(0), Some(5));
        assert!(matches!(
            poll.on_response(PollResponse::Malformed),
            PollStep::Failed(FailReason {
                kind: FailKind::BadResponse,
                ..
            })
        ));
    }

    #[test]
    fn views_and_errors_serialize_for_the_window() {
        let view = AccountView::new(
            &base(),
            AccountState::WaitingForUser {
                user_code: "BCDF-GHJK".into(),
                verification_uri: "https://example.test/device".into(),
                verification_uri_complete: "https://example.test/device?user_code=BCDF-GHJK".into(),
                expires_at: 1,
                interval: 5,
            },
        );
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["state"]["kind"], "waitingForUser");
        assert_eq!(json["state"]["userCode"], "BCDF-GHJK");
        assert_eq!(json["devicesUrl"], "https://example.test/account/devices");
        assert_eq!(json["official"], false);
        let idle = serde_json::to_value(AccountState::Idle {
            notice: Some(IdleNotice::SessionEnded),
        })
        .unwrap();
        assert_eq!(
            idle,
            serde_json::json!({"kind":"idle","notice":"sessionEnded"})
        );
        let error = serde_json::to_value(AccountError::Server(Some("Busy".into()))).unwrap();
        assert_eq!(error, serde_json::json!({"kind":"server","detail":"Busy"}));
        let page: AccountPage =
            serde_json::from_str(r#"{"kind":"invite","projectId":"317"}"#).unwrap();
        assert_eq!(
            page,
            AccountPage::Invite {
                project_id: "317".into()
            }
        );
    }

    #[test]
    fn device_name_is_bounded() {
        let name = device_name();
        assert!(!name.is_empty() && name.chars().count() <= 100);
        assert!(!platform().is_empty());
    }
}

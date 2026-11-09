//! HTTP client of the Hangar account API (`account.rs` holds the types and rules).
//!
//! Every request has a total time limit, every answer a size limit, redirects are not followed
//! (a bearer token never travels to another address), and plain http is allowed only when the
//! configured base is a loopback address (which is also never reached through a proxy). Request
//! bodies are JSON (the API accepts flat JSON objects), except the RFC 7009 revocation, which is a
//! form. Errors never carry a token: request URLs hold none, and bodies are not logged.

use std::fmt::Write as _;
use std::time::Duration;

use reqwest::{Method, StatusCode};
use zeroize::Zeroizing;

use crate::account::{
    self, BaseUrl, DeviceAuthorization, Me, OAuthCode, PollResponse, Problem, Project, RelayTicket,
    TokenSet,
};

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// Revoking on sign-out is best effort; the person should not wait long for it.
pub const REVOKE_TIMEOUT: Duration = Duration::from_secs(5);
/// Far above any real answer (a project list of a few hundred entries).
pub const RESPONSE_LIMIT: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    #[error("network error: {0}")]
    Network(String),
    #[error("the server took too long")]
    Timeout,
    #[error("the answer is too large")]
    TooLarge,
    /// 401 on an endpoint with a bearer token.
    #[error("the access token was refused")]
    Unauthorized,
    /// An OAuth error of the token endpoint.
    #[error("OAuth error")]
    OAuth(OAuthCode, Option<String>),
    #[error("HTTP {status}")]
    Status {
        status: u16,
        /// Boxed: the error travels through every `Result` of this module.
        problem: Option<Box<Problem>>,
    },
    #[error("unexpected answer")]
    Malformed,
}

impl ApiError {
    /// Worth asking again later: network trouble, rate limits, server errors.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Network(_) | Self::Timeout => true,
            Self::Status { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }
}

impl From<ApiError> for account::AccountError {
    fn from(error: ApiError) -> Self {
        match error {
            ApiError::Network(text) => Self::Network(text),
            ApiError::Timeout => Self::Network("timeout".to_owned()),
            ApiError::Unauthorized => Self::SessionEnded,
            ApiError::OAuth(_, detail) => Self::Server(detail),
            ApiError::Status { problem, .. } => Self::Server(problem.and_then(|p| p.message())),
            ApiError::TooLarge | ApiError::Malformed => Self::BadResponse,
        }
    }
}

/// One server's API. Cheap to share behind an `Arc`; replaced when the base changes.
#[derive(Debug)]
pub struct AccountApi {
    client: reqwest::Client,
    base: BaseUrl,
}

enum Body {
    None,
    Json(serde_json::Value),
    /// `application/x-www-form-urlencoded`, already encoded; may hold a token.
    Form(Zeroizing<String>),
}

struct Answer {
    status: StatusCode,
    /// Token answers carry tokens; wiped when dropped.
    body: Zeroizing<Vec<u8>>,
}

impl AccountApi {
    pub fn new(base: BaseUrl) -> Result<Self, ApiError> {
        crate::updates::ensure_crypto_provider();
        let mut builder = reqwest::Client::builder()
            .user_agent(format!("Hangar/{}", env!("CARGO_PKG_VERSION")))
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(REQUEST_TIMEOUT)
            .https_only(base.is_https())
            .redirect(reqwest::redirect::Policy::none());
        if base.is_loopback() {
            // A development server on this machine: a system proxy would see the plain-http
            // tokens, or fail to reach it at all.
            builder = builder.no_proxy();
        }
        let client = builder
            .build()
            .map_err(|error| ApiError::Network(error.without_url().to_string()))?;
        Ok(Self { client, base })
    }

    pub fn base(&self) -> &BaseUrl {
        &self.base
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        bearer: Option<&str>,
        body: Body,
    ) -> Result<Answer, ApiError> {
        let mut request = self
            .client
            .request(method, self.base.endpoint(path))
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(token) = bearer {
            request = request.bearer_auth(token);
        }
        match body {
            Body::None => {}
            Body::Json(body) => {
                let bytes = serde_json::to_vec(&body).map_err(|_| ApiError::Malformed)?;
                request = request
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(bytes);
            }
            Body::Form(form) => {
                request = request
                    .header(
                        reqwest::header::CONTENT_TYPE,
                        "application/x-www-form-urlencoded",
                    )
                    .timeout(REVOKE_TIMEOUT)
                    .body(form.as_bytes().to_vec());
            }
        }
        let mut response = request.send().await.map_err(map_reqwest)?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > RESPONSE_LIMIT as u64)
        {
            return Err(ApiError::TooLarge);
        }
        let mut body = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(map_reqwest)? {
            if body.len() + chunk.len() > RESPONSE_LIMIT {
                return Err(ApiError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Answer { status, body })
    }

    /// `POST /oauth/device` (RFC 8628 3.1).
    pub async fn device_authorization(
        &self,
        device_name: &str,
        platform: &str,
    ) -> Result<DeviceAuthorization, ApiError> {
        let body = serde_json::json!({
            "client_id": account::CLIENT_ID,
            "scope": account::SCOPE,
            "device_name": device_name,
            "platform": platform,
        });
        let answer = self
            .send(Method::POST, "oauth/device", None, Body::Json(body))
            .await?;
        if !answer.status.is_success() {
            return Err(oauth_error(&answer));
        }
        DeviceAuthorization::parse(&answer.body).ok_or(ApiError::Malformed)
    }

    /// One poll of `POST /oauth/token` with the device code, sorted for the polling rules.
    pub async fn poll_token(&self, device_code: &str) -> PollResponse {
        let body = serde_json::json!({
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
            "device_code": device_code,
            "client_id": account::CLIENT_ID,
        });
        let answer = match self
            .send(Method::POST, "oauth/token", None, Body::Json(body))
            .await
        {
            Ok(answer) => answer,
            Err(error) if error.is_transient() => return PollResponse::Transient,
            Err(_) => return PollResponse::Malformed,
        };
        if answer.status.is_success() {
            return TokenSet::parse(&answer.body)
                .map_or(PollResponse::Malformed, PollResponse::Tokens);
        }
        match oauth_error(&answer) {
            ApiError::OAuth(code, detail) => PollResponse::OAuth(code, detail),
            error if error.is_transient() => PollResponse::Transient,
            _ => PollResponse::Malformed,
        }
    }

    /// `POST /oauth/token` with `grant_type=refresh_token`. The server rotates the refresh token
    /// and treats a second use of the old one as theft, so callers serialize this
    /// (`account_service.rs`).
    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenSet, ApiError> {
        let body = serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": account::CLIENT_ID,
        });
        let answer = self
            .send(Method::POST, "oauth/token", None, Body::Json(body))
            .await?;
        if !answer.status.is_success() {
            return Err(oauth_error(&answer));
        }
        TokenSet::parse(&answer.body).ok_or(ApiError::Malformed)
    }

    /// `POST /oauth/revoke` (RFC 7009) with the refresh token, which removes the whole device grant
    /// on the server. A form, as the RFC prescribes; at most [`REVOKE_TIMEOUT`]. The server answers
    /// 200 even for a token it no longer knows.
    pub async fn revoke(&self, refresh_token: &str) -> Result<(), ApiError> {
        let mut form = Zeroizing::new(String::new());
        for (key, value) in [
            ("token", refresh_token),
            ("token_type_hint", "refresh_token"),
            ("client_id", account::CLIENT_ID),
        ] {
            if !form.is_empty() {
                form.push('&');
            }
            form.push_str(key);
            form.push('=');
            form_encode(&mut form, value);
        }
        let answer = self
            .send(Method::POST, "oauth/revoke", None, Body::Form(form))
            .await?;
        if answer.status.is_success() {
            Ok(())
        } else {
            Err(oauth_error(&answer))
        }
    }

    async fn get(&self, path: &str, access: &str) -> Result<Zeroizing<Vec<u8>>, ApiError> {
        let answer = self
            .send(Method::GET, path, Some(access), Body::None)
            .await?;
        checked(answer)
    }

    pub async fn me(&self, access: &str) -> Result<Me, ApiError> {
        let body = self.get("me", access).await?;
        serde_json::from_slice(&body).map_err(|_| ApiError::Malformed)
    }

    pub async fn projects(&self, access: &str) -> Result<Vec<Project>, ApiError> {
        let body = self.get("projects", access).await?;
        Project::parse_list(&body, &self.base).ok_or(ApiError::Malformed)
    }

    /// `POST /relay-ticket` for a project the person is a member of (404 otherwise).
    pub async fn relay_ticket(
        &self,
        access: &str,
        project_id: &str,
    ) -> Result<RelayTicket, ApiError> {
        let body = serde_json::json!({ "project_id": project_id });
        let answer = self
            .send(Method::POST, "relay-ticket", Some(access), Body::Json(body))
            .await?;
        let body = checked(answer)?;
        RelayTicket::parse(&body).ok_or(ApiError::Malformed)
    }
}

fn checked(answer: Answer) -> Result<Zeroizing<Vec<u8>>, ApiError> {
    if answer.status == StatusCode::UNAUTHORIZED {
        return Err(ApiError::Unauthorized);
    }
    if !answer.status.is_success() {
        return Err(ApiError::Status {
            status: answer.status.as_u16(),
            problem: Problem::parse(&answer.body).map(Box::new),
        });
    }
    Ok(answer.body)
}

/// The OAuth error of a failed token or device request; a problem without `error` stays a
/// status error.
fn oauth_error(answer: &Answer) -> ApiError {
    let problem = Problem::parse(&answer.body);
    match problem.as_ref().and_then(|p| p.error.as_deref()) {
        Some(code) if answer.status.is_client_error() => ApiError::OAuth(
            OAuthCode::parse(code),
            problem.as_ref().and_then(Problem::message),
        ),
        _ => ApiError::Status {
            status: answer.status.as_u16(),
            problem: problem.map(Box::new),
        },
    }
}

/// Percent-encodes `value` for a form body: unreserved characters stay, every other byte becomes
/// `%XX` (WHATWG `application/x-www-form-urlencoded`, with `%20` for space, which it accepts).
fn form_encode(out: &mut String, value: &str) {
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'*') {
            out.push(char::from(byte));
        } else {
            // Writing to a String cannot fail.
            let _ = write!(out, "%{byte:02X}");
        }
    }
}

fn map_reqwest(error: reqwest::Error) -> ApiError {
    if error.is_timeout() {
        ApiError::Timeout
    } else {
        ApiError::Network(error.without_url().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_values_are_percent_encoded() {
        let mut out = String::new();
        form_encode(&mut out, "hgrt_a-b.c*d~e+f=g&h%i j/\u{e4}");
        assert_eq!(out, "hgrt_a-b.c*d%7Ee%2Bf%3Dg%26h%25i%20j%2F%C3%A4");
    }
}

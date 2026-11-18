//! Connection test of a self-hosted server (Settings > Account): three steps in a row, each with
//! an explicit result the window turns into one line and one sentence on what to do.
//!
//! 1. The account API: `GET <base>/keys`, the server's public ticket key, answers as a Hangar
//!    server does (a JSON object with `keys`), not with a web page.
//! 2. The relay: `GET <relay>/health` answers 200 with `{"status":"ok", "version": ...}`. A web
//!    page or a 404 there means the reverse proxy does not pass the relay's paths on.
//! 3. The live channel: a WebSocket upgrade on `<relay>/v1/live` gets 101. A 400 or 426 means the
//!    proxy drops the `Upgrade` and `Connection` headers.
//!
//! The relay address is the one a ticket names when the person is signed in (what the team
//! connection uses), otherwise the one hangar-server derives from its own address (`PUBLIC_URL`
//! plus `/v1/live`). No step sends a token or a ticket, and no result or log line holds one: the
//! upgrade ends right after the handshake, before `hello`.
//!
//! Only https and wss, or plain http and ws when the server itself is a loopback one
//! ([`account::relay_url`]); every request has a time limit and a size limit and follows no
//! redirect.

use std::time::Duration;

use reqwest::Url;
use serde::Serialize;

use crate::account::{self, BaseUrl};
use crate::team_relay::{self, RelayError};

/// Per step; a server that answers at all does so well within this.
pub const STEP_TIMEOUT: Duration = Duration::from_secs(8);
/// `/keys` and `/health` are a few hundred bytes; an HTML error page is a few kilobytes.
const BODY_LIMIT: usize = 64 * 1024;
const MAX_VERSION_CHARS: usize = 32;

/// Step 1: the account API at the server address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ApiCheck {
    Ok,
    /// No connection (name, refused, TLS).
    Unreachable,
    Timeout,
    /// A redirect: Hangar follows none, so the address must be the final one.
    Redirected {
        status: u16,
    },
    /// A Hangar server (JSON) without the account API at this path: the address misses `/api/v1`
    /// or the server does not keep its own accounts.
    WrongPath,
    /// 502 or 504: the proxy answers, the server behind it does not.
    BadGateway {
        status: u16,
    },
    ServerError {
        status: u16,
    },
    /// Anything else, a web page above all: no Hangar server answers at this address.
    NotHangar {
        status: u16,
    },
}

/// Step 2: the relay's health check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RelayCheck {
    Ok {
        /// The relay's version as it reports it, cleaned; `None` when it names none.
        version: Option<String>,
    },
    Unreachable,
    Timeout,
    /// A web page, a 404 or a redirect at `/health`: the proxy does not pass `/` on to the relay.
    NoProxy {
        status: u16,
    },
    BadGateway {
        status: u16,
    },
    /// 503 with `{"status":"error"}`: the relay runs, its database does not answer.
    Unhealthy,
    Unexpected {
        status: u16,
    },
}

/// Step 3: the WebSocket upgrade of the live channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LiveCheck {
    Ok,
    /// Not tried: the relay did not answer its health check.
    Skipped,
    Unreachable,
    Timeout,
    /// 400 or 426: the proxy does not pass `Upgrade` and `Connection` on.
    MissingUpgrade {
        status: u16,
    },
    NoProxy {
        status: u16,
    },
    BadGateway {
        status: u16,
    },
    /// 503: the relay takes no more connections right now.
    Busy,
    /// Another status, or no status at all (`None`: the handshake broke off).
    Unexpected {
        status: Option<u16>,
    },
}

/// Where the relay address came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RelaySource {
    /// A ticket of the signed-in account named it.
    Ticket,
    /// Derived from the server address, as hangar-server does.
    Derived,
}

/// The whole test, for the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCheck {
    /// The server address the test ran against: the window drops a result that arrives after
    /// the address changed.
    pub base_url: String,
    pub api: ApiCheck,
    pub relay: RelayCheck,
    pub live: LiveCheck,
    /// `host` or `host:port` of the relay; never a path or a ticket.
    pub relay_host: String,
    pub relay_source: RelaySource,
}

/// One HTTP answer, as far as the checks care.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub status: u16,
    /// The body as JSON, `None` when it is none (an HTML page, empty, too large).
    pub json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Unreachable,
    Timeout,
}

pub fn classify_api(answer: Result<Answer, Failure>) -> ApiCheck {
    let answer = match answer {
        Ok(answer) => answer,
        Err(Failure::Unreachable) => return ApiCheck::Unreachable,
        Err(Failure::Timeout) => return ApiCheck::Timeout,
    };
    let status = answer.status;
    let keys = answer
        .json
        .as_ref()
        .and_then(|json| json.get("keys"))
        .is_some_and(serde_json::Value::is_array);
    match status {
        200 if keys => ApiCheck::Ok,
        300..=399 => ApiCheck::Redirected { status },
        404 if answer
            .json
            .as_ref()
            .is_some_and(serde_json::Value::is_object) =>
        {
            ApiCheck::WrongPath
        }
        502 | 504 => ApiCheck::BadGateway { status },
        500..=599 => ApiCheck::ServerError { status },
        _ => ApiCheck::NotHangar { status },
    }
}

pub fn classify_health(answer: Result<Answer, Failure>) -> RelayCheck {
    let answer = match answer {
        Ok(answer) => answer,
        Err(Failure::Unreachable) => return RelayCheck::Unreachable,
        Err(Failure::Timeout) => return RelayCheck::Timeout,
    };
    let status = answer.status;
    let field = |key: &str| {
        answer
            .json
            .as_ref()
            .and_then(|json| json.get(key))
            .and_then(serde_json::Value::as_str)
    };
    match status {
        200 if field("status") == Some("ok") => RelayCheck::Ok {
            version: field("version")
                .map(|version| {
                    account::clean_text(version)
                        .chars()
                        .take(MAX_VERSION_CHARS)
                        .collect::<String>()
                })
                .filter(|version| !version.is_empty()),
        },
        // 200 with anything else is a page of whatever the proxy serves at `/`.
        200 | 300..=399 | 404 => RelayCheck::NoProxy { status },
        503 if field("status") == Some("error") => RelayCheck::Unhealthy,
        502..=504 => RelayCheck::BadGateway { status },
        _ => RelayCheck::Unexpected { status },
    }
}

pub fn classify_live(result: Result<(), RelayError>) -> LiveCheck {
    match result {
        Ok(()) => LiveCheck::Ok,
        Err(RelayError::Timeout) => LiveCheck::Timeout,
        Err(RelayError::Address | RelayError::Connect(_) | RelayError::Tls(_)) => {
            LiveCheck::Unreachable
        }
        Err(RelayError::Status(status)) => match status {
            400 | 426 => LiveCheck::MissingUpgrade { status },
            // A page instead of the upgrade: the request never reached the relay.
            200 | 300..=399 | 404 => LiveCheck::NoProxy { status },
            503 => LiveCheck::Busy,
            502 | 504 => LiveCheck::BadGateway { status },
            _ => LiveCheck::Unexpected {
                status: Some(status),
            },
        },
        Err(RelayError::Handshake(_)) => LiveCheck::Unexpected { status: None },
    }
}

/// The relay's live channel as hangar-server names it for `base`: its address with `/api/v1`
/// replaced by `/v1/live`, `wss` for https and `ws` for a loopback http server.
pub fn derived_relay(base: &BaseUrl) -> Option<Url> {
    let url = Url::parse(base.as_str()).ok()?;
    let scheme = if base.is_https() { "wss" } else { "ws" };
    let prefix = url.path().trim_end_matches('/');
    let prefix = prefix.strip_suffix("/api/v1").unwrap_or(prefix);
    let host = url.host_str()?;
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    account::relay_url(&format!("{scheme}://{host}{port}{prefix}/v1/live"), base)
}

/// `<relay>/health` for a live channel address: the same origin and path prefix, http(s).
pub fn health_url(live: &Url) -> Option<Url> {
    let scheme = match live.scheme() {
        "wss" => "https",
        "ws" => "http",
        _ => return None,
    };
    let mut url = live.clone();
    url.set_scheme(scheme).ok()?;
    let path = live.path().trim_end_matches('/');
    let prefix = path.strip_suffix("/v1/live").unwrap_or("");
    url.set_path(&format!("{prefix}/health"));
    url.set_query(None);
    url.set_fragment(None);
    Some(url)
}

/// `host` or `host:port` of `url`.
pub fn host_of(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

/// Step 1 against `base`, within `limit`.
pub async fn check_api(base: &BaseUrl, limit: Duration) -> ApiCheck {
    let answer = match client(base, limit) {
        Some(client) => get(&client, base.endpoint("keys")).await,
        None => Err(Failure::Unreachable),
    };
    classify_api(answer)
}

/// Steps 2 and 3 against `live`, the relay of a ticket `base` issued or the derived one; the
/// upgrade only after a healthy relay.
pub async fn check_relay(base: &BaseUrl, live: &Url, limit: Duration) -> (RelayCheck, LiveCheck) {
    let answer = match (client(base, limit), health_url(live)) {
        (Some(client), Some(url)) => get(&client, url).await,
        _ => Err(Failure::Unreachable),
    };
    let relay = classify_health(answer);
    if !matches!(relay, RelayCheck::Ok { .. }) {
        return (relay, LiveCheck::Skipped);
    }
    // The socket is dropped right after the handshake: nothing but the upgrade travels.
    let upgrade = team_relay::connect(live, base, limit).await.map(drop);
    (relay, classify_live(upgrade))
}

/// The whole test for `base`; `ticket_relay` is the relay a ticket named, when there is one.
pub async fn run(base: &BaseUrl, ticket_relay: Option<Url>, limit: Duration) -> ServerCheck {
    let api = check_api(base, limit).await;
    finish(base, api, ticket_relay, limit).await
}

/// Steps 2 and 3 after step 1 gave `api`.
pub async fn finish(
    base: &BaseUrl,
    api: ApiCheck,
    ticket_relay: Option<Url>,
    limit: Duration,
) -> ServerCheck {
    let (live, relay_source) = match ticket_relay {
        Some(url) => (Some(url), RelaySource::Ticket),
        None => (derived_relay(base), RelaySource::Derived),
    };
    let Some(live) = live else {
        return ServerCheck {
            base_url: base.as_str().to_owned(),
            api,
            relay: RelayCheck::Unreachable,
            live: LiveCheck::Skipped,
            relay_host: String::new(),
            relay_source,
        };
    };
    let (relay, live_check) = check_relay(base, &live, limit).await;
    ServerCheck {
        base_url: base.as_str().to_owned(),
        api,
        relay,
        live: live_check,
        relay_host: host_of(&live),
        relay_source,
    }
}

/// Like the account API's client: no redirects, https only unless the server is a loopback one,
/// which is never reached through a proxy.
fn client(base: &BaseUrl, limit: Duration) -> Option<reqwest::Client> {
    crate::updates::ensure_crypto_provider();
    let mut builder = reqwest::Client::builder()
        .user_agent(format!("Hangar/{}", env!("CARGO_PKG_VERSION")))
        .timeout(limit)
        .connect_timeout(limit)
        .https_only(base.is_https())
        .redirect(reqwest::redirect::Policy::none());
    if base.is_loopback() {
        builder = builder.no_proxy();
    }
    builder.build().ok()
}

async fn get(client: &reqwest::Client, url: Url) -> Result<Answer, Failure> {
    let mut response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(failure)?;
    let status = response.status();
    let mut body = Vec::new();
    let mut fits = response
        .content_length()
        .is_none_or(|length| length <= BODY_LIMIT as u64);
    while fits {
        match response.chunk().await.map_err(failure)? {
            Some(chunk) if body.len() + chunk.len() <= BODY_LIMIT => body.extend_from_slice(&chunk),
            Some(_) => fits = false,
            None => break,
        }
    }
    let json = if fits {
        serde_json::from_slice(&body).ok()
    } else {
        None
    };
    Ok(Answer {
        status: status.as_u16(),
        json,
    })
}

fn failure(error: reqwest::Error) -> Failure {
    if error.is_timeout() {
        Failure::Timeout
    } else {
        Failure::Unreachable
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    use serde_json::json;

    use super::*;

    fn answer(status: u16, json: Option<serde_json::Value>) -> Result<Answer, Failure> {
        Ok(Answer { status, json })
    }

    #[test]
    fn the_account_api_answers_as_a_hangar_server() {
        let keys = json!({ "keys": [{ "kty": "OKP" }] });
        assert_eq!(classify_api(answer(200, Some(keys))), ApiCheck::Ok);
        // A web page at the address (no JSON), or JSON of something else.
        assert_eq!(
            classify_api(answer(200, None)),
            ApiCheck::NotHangar { status: 200 }
        );
        assert_eq!(
            classify_api(answer(200, Some(json!({ "keys": "x" })))),
            ApiCheck::NotHangar { status: 200 }
        );
        assert_eq!(
            classify_api(answer(404, None)),
            ApiCheck::NotHangar { status: 404 }
        );
        // hangar-server's own 404 (problem or `{"error":"not_found"}`): the path is wrong.
        assert_eq!(
            classify_api(answer(404, Some(json!({ "error": "not_found" })))),
            ApiCheck::WrongPath
        );
        assert_eq!(
            classify_api(answer(301, None)),
            ApiCheck::Redirected { status: 301 }
        );
        assert_eq!(
            classify_api(answer(502, None)),
            ApiCheck::BadGateway { status: 502 }
        );
        assert_eq!(
            classify_api(answer(500, None)),
            ApiCheck::ServerError { status: 500 }
        );
        assert_eq!(classify_api(Err(Failure::Timeout)), ApiCheck::Timeout);
        assert_eq!(
            classify_api(Err(Failure::Unreachable)),
            ApiCheck::Unreachable
        );
    }

    #[test]
    fn the_health_check_tells_a_missing_proxy_rule_from_a_healthy_relay() {
        assert_eq!(
            classify_health(answer(
                200,
                Some(json!({ "status": "ok", "version": "0.4.1" }))
            )),
            RelayCheck::Ok {
                version: Some("0.4.1".to_owned())
            }
        );
        assert_eq!(
            classify_health(answer(200, Some(json!({ "status": "ok" })))),
            RelayCheck::Ok { version: None }
        );
        // The case of the report: nginx's 404 page, no proxy rule for `/`.
        assert_eq!(
            classify_health(answer(404, None)),
            RelayCheck::NoProxy { status: 404 }
        );
        assert_eq!(
            classify_health(answer(200, None)),
            RelayCheck::NoProxy { status: 200 }
        );
        assert_eq!(
            classify_health(answer(503, Some(json!({ "status": "error" })))),
            RelayCheck::Unhealthy
        );
        assert_eq!(
            classify_health(answer(503, None)),
            RelayCheck::BadGateway { status: 503 }
        );
        assert_eq!(
            classify_health(answer(401, None)),
            RelayCheck::Unexpected { status: 401 }
        );
    }

    #[test]
    fn a_reported_version_is_cleaned_and_short() {
        let long = format!("1.0\u{202e}{}", "9".repeat(100));
        let RelayCheck::Ok {
            version: Some(version),
        } = classify_health(answer(
            200,
            Some(json!({ "status": "ok", "version": long })),
        ))
        else {
            panic!("not ok");
        };
        assert!(!version.contains('\u{202e}'));
        assert_eq!(version.chars().count(), MAX_VERSION_CHARS);
    }

    #[test]
    fn the_upgrade_tells_missing_headers_from_a_missing_rule() {
        assert_eq!(classify_live(Ok(())), LiveCheck::Ok);
        assert_eq!(
            classify_live(Err(RelayError::Status(400))),
            LiveCheck::MissingUpgrade { status: 400 }
        );
        assert_eq!(
            classify_live(Err(RelayError::Status(426))),
            LiveCheck::MissingUpgrade { status: 426 }
        );
        assert_eq!(
            classify_live(Err(RelayError::Status(404))),
            LiveCheck::NoProxy { status: 404 }
        );
        assert_eq!(classify_live(Err(RelayError::Status(503))), LiveCheck::Busy);
        assert_eq!(classify_live(Err(RelayError::Timeout)), LiveCheck::Timeout);
        assert_eq!(
            classify_live(Err(RelayError::Connect("refused".to_owned()))),
            LiveCheck::Unreachable
        );
    }

    #[test]
    fn the_relay_is_derived_like_hangar_server_does() {
        let base = BaseUrl::parse("https://team.example.test/api/v1").unwrap();
        let live = derived_relay(&base).unwrap();
        assert_eq!(live.as_str(), "wss://team.example.test/v1/live");
        assert_eq!(
            health_url(&live).unwrap().as_str(),
            "https://team.example.test/health"
        );
        assert_eq!(host_of(&live), "team.example.test");
        let prefixed = BaseUrl::parse("https://example.test:8443/hangar/api/v1").unwrap();
        let live = derived_relay(&prefixed).unwrap();
        assert_eq!(live.as_str(), "wss://example.test:8443/hangar/v1/live");
        assert_eq!(
            health_url(&live).unwrap().as_str(),
            "https://example.test:8443/hangar/health"
        );
        assert_eq!(host_of(&live), "example.test:8443");
        // Plain ws only for a loopback server.
        let local = BaseUrl::parse("http://127.0.0.1:8080/api/v1").unwrap();
        assert_eq!(
            derived_relay(&local).unwrap().as_str(),
            "ws://127.0.0.1:8080/v1/live"
        );
    }

    /// A one-shot server on 127.0.0.1: reads one request, then writes `reply` (or nothing).
    fn serve(replies: Vec<Option<String>>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for reply in replies {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                read_request(&mut stream);
                match reply {
                    Some(reply) => {
                        let _ = stream.write_all(reply.as_bytes());
                    }
                    // Never answers: the client's time limit ends the wait.
                    None => thread::sleep(Duration::from_secs(3)),
                }
            }
        });
        port
    }

    fn read_request(stream: &mut TcpStream) -> String {
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            match stream.read(&mut byte) {
                Ok(1) => request.push(byte[0]),
                _ => break,
            }
        }
        String::from_utf8_lossy(&request).into_owned()
    }

    fn http(status: &str, kind: &str, body: &str) -> Option<String> {
        Some(format!(
            "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ))
    }

    fn local(port: u16) -> BaseUrl {
        BaseUrl::parse(&format!("http://127.0.0.1:{port}/api/v1")).unwrap()
    }

    fn block<T>(future: impl std::future::Future<Output = T>) -> T {
        tauri::async_runtime::block_on(future)
    }

    const LIMIT: Duration = Duration::from_millis(500);

    #[test]
    fn a_healthy_server_passes_all_three_steps() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for _ in 0..3 {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut peek = [0u8; 64];
                let read = stream.peek(&mut peek).unwrap_or(0);
                let line = String::from_utf8_lossy(&peek[..read]).into_owned();
                if line.starts_with("GET /v1/live") {
                    let _ = tokio_tungstenite::tungstenite::accept(stream);
                    continue;
                }
                read_request(&mut stream);
                let reply = if line.starts_with("GET /api/v1/keys") {
                    http("200 OK", "application/json", r#"{"keys":[{"kty":"OKP"}]}"#)
                } else {
                    http(
                        "200 OK",
                        "application/json",
                        r#"{"status":"ok","version":"0.4.1"}"#,
                    )
                };
                let _ = stream.write_all(reply.unwrap_or_default().as_bytes());
            }
        });
        let check = block(run(&local(port), None, LIMIT));
        assert_eq!(
            check,
            ServerCheck {
                base_url: format!("http://127.0.0.1:{port}/api/v1"),
                api: ApiCheck::Ok,
                relay: RelayCheck::Ok {
                    version: Some("0.4.1".to_owned())
                },
                live: LiveCheck::Ok,
                relay_host: format!("127.0.0.1:{port}"),
                relay_source: RelaySource::Derived,
            }
        );
    }

    #[test]
    fn an_html_404_at_health_means_the_proxy_rule_is_missing() {
        let page = "<html><body><h1>404 Not Found</h1></body></html>";
        let port = serve(vec![
            http("200 OK", "application/json", r#"{"keys":[]}"#),
            http("404 Not Found", "text/html", page),
        ]);
        let check = block(run(&local(port), None, LIMIT));
        assert_eq!(check.api, ApiCheck::Ok);
        assert_eq!(check.relay, RelayCheck::NoProxy { status: 404 });
        assert_eq!(check.live, LiveCheck::Skipped);
    }

    #[test]
    fn an_html_page_at_the_api_is_not_a_hangar_server() {
        let port = serve(vec![http(
            "404 Not Found",
            "text/html",
            "<html>Not Found</html>",
        )]);
        assert_eq!(
            block(check_api(&local(port), LIMIT)),
            ApiCheck::NotHangar { status: 404 }
        );
    }

    #[test]
    fn a_400_on_the_upgrade_means_the_upgrade_headers_are_missing() {
        let port = serve(vec![
            http("200 OK", "application/json", r#"{"status":"ok"}"#),
            http(
                "400 Bad Request",
                "text/plain",
                "Connection header did not include 'upgrade'",
            ),
        ]);
        let base = local(port);
        let live = derived_relay(&base).unwrap();
        assert_eq!(
            block(check_relay(&base, &live, LIMIT)),
            (
                RelayCheck::Ok { version: None },
                LiveCheck::MissingUpgrade { status: 400 }
            )
        );
    }

    #[test]
    fn a_server_that_never_answers_runs_into_the_time_limit() {
        let port = serve(vec![None, None]);
        let check = block(run(&local(port), None, Duration::from_millis(300)));
        assert_eq!(check.api, ApiCheck::Timeout);
        assert_eq!(check.relay, RelayCheck::Timeout);
        assert_eq!(check.live, LiveCheck::Skipped);
    }

    #[test]
    fn a_closed_port_is_unreachable() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        // Windows retries a refused connection for about two seconds before it gives up.
        let check = block(run(&local(port), None, Duration::from_secs(10)));
        assert_eq!(check.api, ApiCheck::Unreachable);
        assert_eq!(check.relay, RelayCheck::Unreachable);
    }
}

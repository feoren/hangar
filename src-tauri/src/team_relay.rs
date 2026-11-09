//! The WebSocket to the team relay: TCP from tokio, TLS from rustls with the operating system's
//! certificate verification (the same stack reqwest uses for the account), the handshake from
//! tokio-tungstenite. The address is the ticket's `relay_url`, already checked by
//! `account::relay_url` (`wss`, or `ws` only on the loopback); the check is repeated here right
//! before the ticket travels, and plain `ws` to the name `localhost` only in debug builds (a name
//! can resolve elsewhere, a loopback address cannot). No proxy support yet.

use std::sync::Arc;
use std::time::Duration;

use hangar_team_protocol::limits::MAX_FRAME_BYTES;
use reqwest::Url;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::WebSocketStream;

/// A byte stream the WebSocket runs over: plain TCP on the loopback, TLS otherwise.
pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

pub type Socket = WebSocketStream<Box<dyn Io>>;

#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    #[error("the relay address is not usable")]
    Address,
    #[error("the relay did not answer in time")]
    Timeout,
    #[error("the relay could not be reached: {0}")]
    Connect(String),
    #[error("TLS failed: {0}")]
    Tls(String),
    #[error("the WebSocket handshake failed: {0}")]
    Handshake(String),
}

/// Opens the WebSocket to `url` within `limit`. Messages and frames over the protocol's 1 MiB
/// are refused by the reader before they are decoded.
pub async fn connect(url: &Url, limit: Duration) -> Result<Socket, RelayError> {
    tokio::time::timeout(limit, open(url))
        .await
        .map_err(|_| RelayError::Timeout)?
}

async fn open(url: &Url) -> Result<Socket, RelayError> {
    let host = url
        .host_str()
        .map(|h| h.trim_start_matches('[').trim_end_matches(']').to_owned())
        .filter(|h| !h.is_empty())
        .ok_or(RelayError::Address)?;
    let port = url.port_or_known_default().ok_or(RelayError::Address)?;
    if url.scheme() == "ws" && !plain_allowed(&host, cfg!(debug_assertions)) {
        return Err(RelayError::Address);
    }
    let tcp = TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|error| RelayError::Connect(error.to_string()))?;
    let _ = tcp.set_nodelay(true);
    let stream: Box<dyn Io> = match url.scheme() {
        "ws" => Box::new(tcp),
        "wss" => {
            crate::updates::ensure_crypto_provider();
            use rustls_platform_verifier::ConfigVerifierExt as _;
            let config = tokio_rustls::rustls::ClientConfig::with_platform_verifier()
                .map_err(|error| RelayError::Tls(error.to_string()))?;
            let name = tokio_rustls::rustls::pki_types::ServerName::try_from(host)
                .map_err(|_| RelayError::Address)?;
            let tls = tokio_rustls::TlsConnector::from(Arc::new(config))
                .connect(name, tcp)
                .await
                .map_err(|error| RelayError::Tls(error.to_string()))?;
            Box::new(tls)
        }
        _ => return Err(RelayError::Address),
    };
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES));
    let (socket, _) =
        tokio_tungstenite::client_async_with_config(url.as_str(), stream, Some(config))
            .await
            .map_err(|error| RelayError::Handshake(error.to_string()))?;
    Ok(socket)
}

/// Whether plain `ws` may go to `host`: a loopback address, or `localhost` in a debug build.
fn plain_allowed(host: &str, debug: bool) -> bool {
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => debug && host.eq_ignore_ascii_case("localhost"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_websockets_only_reach_the_loopback() {
        assert!(plain_allowed("127.0.0.1", false));
        assert!(plain_allowed("::1", false));
        assert!(plain_allowed("localhost", true));
        assert!(!plain_allowed("localhost", false));
        assert!(!plain_allowed("10.0.0.1", true));
        assert!(!plain_allowed("relay.example.test", true));
    }

    #[test]
    fn a_plain_websocket_elsewhere_is_refused_before_connecting() {
        let url = Url::parse("ws://192.0.2.1:9/").unwrap();
        let refused = tauri::async_runtime::block_on(connect(&url, Duration::from_secs(5)));
        assert!(matches!(refused, Err(RelayError::Address)));
    }
}

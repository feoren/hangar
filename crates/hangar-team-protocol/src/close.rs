//! How the relay ends a connection: WebSocket close codes and the `hello` deadline. A close
//! ends every team on the connection, so the relay uses it only where the connection itself
//! is no longer acceptable; a refused intent is an `err` frame instead.
//!
//! Codes 4000 to 4999 are reserved for applications (RFC 6455, section 7.4.2); the protocol's
//! own codes mirror the HTTP status they resemble (401, 403).

/// Seconds after the WebSocket opens within which the relay must have accepted a `hello`;
/// otherwise it closes with [`CLOSE_HELLO_TIMEOUT`]. Keeps sockets without a valid sign-in from
/// holding resources.
pub const HELLO_TIMEOUT_SECS: u64 = 15;

/// No valid `hello` within [`HELLO_TIMEOUT_SECS`]. The standard "policy violation" code
/// (RFC 6455, section 7.4.1). The client reconnects with a fresh ticket.
pub const CLOSE_HELLO_TIMEOUT: u16 = 1008;

/// The member's role changed since this connection's ticket was issued, and the relay will not
/// keep acting on the old role. The client fetches a fresh ticket from the website and
/// reconnects; nothing was removed. On a live connection the app normally avoids this with
/// `ticket.renew`, which applies a new role in place.
pub const CLOSE_ROLE_CHANGED: u16 = 4401;

/// The connection's member may no longer use the team: the member was removed, or a
/// `ticket.renew` came without a seat (`seat_ok` false), for another subject than the
/// connection's member, for another team than the frame's (`prj`; answered with `err forbidden`
/// before the close), or did not verify. The client does not reconnect with the same ticket; it
/// asks the website for a new one, which tells whether the member still has a seat.
pub const CLOSE_REMOVED: u16 = 4403;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_codes_are_distinct_and_in_their_ranges() {
        // 1008 is a standard code; the others lie in the range RFC 6455 leaves to applications.
        assert_eq!(CLOSE_HELLO_TIMEOUT, 1008);
        for code in [CLOSE_ROLE_CHANGED, CLOSE_REMOVED] {
            assert!((4000..=4999).contains(&code), "{code}");
        }
        assert_ne!(CLOSE_ROLE_CHANGED, CLOSE_REMOVED);
        assert_eq!((CLOSE_ROLE_CHANGED, CLOSE_REMOVED), (4401, 4403));
        assert_eq!(HELLO_TIMEOUT_SECS, 15);
    }
}

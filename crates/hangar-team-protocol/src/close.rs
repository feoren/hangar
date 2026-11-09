//! How the relay ends a connection: WebSocket close codes and the `hello` deadline. A close
//! ends every team on the connection, so the relay uses it only where the connection itself
//! is no longer acceptable; a refused intent is an `err` frame instead.
//!
//! Codes 4000 to 4999 are reserved for applications (RFC 6455, section 7.4.2); the protocol's
//! own codes mirror the HTTP status they resemble (401, 403, 404).

/// Seconds after the WebSocket opens within which the relay must have accepted a `hello`;
/// otherwise it closes with [`CLOSE_HELLO_TIMEOUT`]. Keeps sockets without a valid sign-in from
/// holding resources.
pub const HELLO_TIMEOUT_SECS: u64 = 15;

/// No valid `hello` within [`HELLO_TIMEOUT_SECS`]. The standard "policy violation" code
/// (RFC 6455, section 7.4.1). The client reconnects with a fresh ticket.
pub const CLOSE_HELLO_TIMEOUT: u16 = 1008;

/// The member's role changed and the relay does not know the new one, so it will not keep
/// acting on the old role. The client fetches a fresh ticket from the website and reconnects;
/// nothing was removed.
///
/// Rare by design: a role change the relay learns together with the new role (from
/// `ticket.renew`, or from the website's revocation feed entry `role_changed` with `role`)
/// applies silently on the live connection, rights are checked per intent from then on, and
/// `member.role_changed` reports it to the team. The relay closes with this code only when a
/// role change is reported without the new role.
pub const CLOSE_ROLE_CHANGED: u16 = 4401;

/// The connection's member may no longer use the team: the member was removed, the
/// connection's device was removed (`device.remove`), or a `ticket.renew` came without a seat
/// (`seat_ok` false), for another subject than the connection's member, for another team than
/// the frame's (`prj`; answered with `err forbidden` before the close), or did not verify. The
/// client does not reconnect with the same ticket; it asks the website for a new one, which
/// tells whether the member still has a seat.
pub const CLOSE_REMOVED: u16 = 4403;

/// The team no longer exists: its project was deleted on the website (the revocation feed's
/// `project_deleted`, or the self hosted relay's own deletion). Every connection of the team is
/// closed with it. The client drops the team's local state and does not reconnect to it; the
/// website issues no more tickets for the project.
pub const CLOSE_TEAM_DELETED: u16 = 4404;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_codes_are_distinct_and_in_their_ranges() {
        // 1008 is a standard code; the others lie in the range RFC 6455 leaves to applications.
        assert_eq!(CLOSE_HELLO_TIMEOUT, 1008);
        let own = [CLOSE_ROLE_CHANGED, CLOSE_REMOVED, CLOSE_TEAM_DELETED];
        for code in own {
            assert!((4000..=4999).contains(&code), "{code}");
        }
        let mut unique = own.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), own.len());
        assert_eq!(own, [4401, 4403, 4404]);
        assert_eq!(HELLO_TIMEOUT_SECS, 15);
    }
}

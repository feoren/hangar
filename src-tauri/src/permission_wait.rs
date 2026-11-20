//! Which sessions wait for a permission or input dialog of their agent (ADR 0016), as the notice
//! hooks report it. Pure: `gate_service.rs` feeds it from the notice routes and the window reads
//! the set; nothing here talks to the network or the window.
//!
//! The signal is set when a dialog appears and cleared when the agent goes on, the turn ends, the
//! person sends a prompt or the process ends. Where none of those arrives (what follows a "No" in
//! the dialog is not measured), [`SIGNAL_TTL`] is the fallback: a stale signal only ever keeps a
//! message waiting for a click, never sends one.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::session::SessionId;

/// How long a signal holds without a new notice. A dialog left open longer falls back to the
/// window's screen check (`src/promptGuard.ts`).
pub const SIGNAL_TTL: Duration = Duration::from_secs(10 * 60);

/// Sessions whose agent shows a dialog, with the time of the newest notice.
#[derive(Debug, Default)]
pub struct PermissionWaits {
    since: BTreeMap<SessionId, Instant>,
}

impl PermissionWaits {
    /// A dialog appeared in `session`; `true` when the set changed.
    pub fn set(&mut self, session: SessionId, now: Instant) -> bool {
        self.since.insert(session, now).is_none()
    }

    /// No dialog waits in `session` any more; `true` when the set changed.
    pub fn clear(&mut self, session: SessionId) -> bool {
        self.since.remove(&session).is_some()
    }

    /// Drops signals older than [`SIGNAL_TTL`] at `now`; `true` when any went.
    pub fn expire(&mut self, now: Instant) -> bool {
        let before = self.since.len();
        self.since
            .retain(|_, at| now.saturating_duration_since(*at) < SIGNAL_TTL);
        self.since.len() != before
    }

    /// The waiting sessions at `now`, in order, without expired ones.
    pub fn sessions(&self, now: Instant) -> Vec<SessionId> {
        self.since
            .iter()
            .filter(|(_, at)| now.saturating_duration_since(**at) < SIGNAL_TTL)
            .map(|(session, _)| *session)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_clear_and_expire() {
        let start = Instant::now();
        let mut waits = PermissionWaits::default();
        assert!(waits.set(3, start));
        assert!(waits.set(1, start));
        // Another notice of the same dialog refreshes the time but changes nothing to show.
        assert!(!waits.set(3, start + Duration::from_secs(60)));
        assert_eq!(waits.sessions(start), [1, 3]);
        assert!(waits.clear(1));
        assert!(!waits.clear(1));
        assert_eq!(waits.sessions(start), [3]);
        // Session 3's newest notice came a minute later, so it holds a minute longer.
        let late = start + SIGNAL_TTL + Duration::from_secs(30);
        assert_eq!(waits.sessions(late), [3]);
        assert!(!waits.expire(late));
        let later = start + SIGNAL_TTL + Duration::from_secs(60);
        assert!(waits.sessions(later).is_empty());
        assert!(waits.expire(later));
        assert!(!waits.expire(later));
    }
}

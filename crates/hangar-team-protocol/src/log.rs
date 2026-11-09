//! Sequencing of a team's event log: monotonic sequence numbers, a replay window for
//! reconnecting clients, the decision when a snapshot (resync) is needed, the client's side of
//! it, and op id deduplication for retried intents.

use std::collections::{HashMap, VecDeque};

use crate::ids::{OpId, Seq, StreamId, TeamId};
use crate::wire::{Cursor, Holder, ResyncReason};

/// The server's log of one team, keeping the newest `capacity` events for replay. Older events
/// live on only in the state (reservations, members) a snapshot sends.
#[derive(Debug, Clone)]
pub struct EventLog<T> {
    stream: StreamId,
    head: Seq,
    window: VecDeque<(Seq, T)>,
    capacity: usize,
}

/// What a reconnecting client gets.
#[derive(Debug, PartialEq, Eq)]
pub enum Replay<'a, T> {
    /// The events after the cursor, oldest first (empty when the client is up to date).
    Events(Vec<(Seq, &'a T)>),
    Resync(ResyncReason),
}

impl<T> EventLog<T> {
    /// A new, empty log: the first event gets sequence number 1.
    pub fn new(stream: StreamId, capacity: usize) -> Self {
        Self::resume(stream, Seq(0), capacity)
    }

    /// A log whose events up to `head` are in storage only (after a server restart): clients
    /// behind `head` need a snapshot, clients at `head` go on.
    pub fn resume(stream: StreamId, head: Seq, capacity: usize) -> Self {
        Self {
            stream,
            head,
            window: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    /// The incarnation of this log.
    pub fn stream(&self) -> &StreamId {
        &self.stream
    }

    /// The newest sequence number (0 while empty).
    pub fn head(&self) -> Seq {
        self.head
    }

    /// Appends an event and returns its sequence number.
    pub fn push(&mut self, event: T) -> Seq {
        self.head = self.head.next();
        if self.window.len() == self.capacity {
            self.window.pop_front();
        }
        self.window.push_back((self.head, event));
        self.head
    }

    /// What a client with `cursor` (from `hello.lastSeq`) gets.
    pub fn replay(&self, cursor: Option<&Cursor>) -> Replay<'_, T> {
        let Some(cursor) = cursor else {
            return Replay::Resync(ResyncReason::NoCursor);
        };
        if cursor.stream != self.stream {
            return Replay::Resync(ResyncReason::OtherStream);
        }
        if cursor.seq > self.head {
            return Replay::Resync(ResyncReason::Ahead);
        }
        if cursor.seq == self.head {
            return Replay::Events(Vec::new());
        }
        // The first event the client misses must still be in the window.
        let oldest = self.window.front().map(|(seq, _)| *seq);
        if oldest.is_none_or(|oldest| cursor.seq.next() < oldest) {
            return Replay::Resync(ResyncReason::TooOld);
        }
        Replay::Events(
            self.window
                .iter()
                .filter(|(seq, _)| *seq > cursor.seq)
                .map(|(seq, event)| (*seq, event))
                .collect(),
        )
    }
}

/// What the client does with an event's `seq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observed {
    /// The next event: apply it; the cursor moved.
    Next,
    /// Seen before (a replay overlapping live events): ignore it.
    Duplicate,
    /// Events are missing: reconnect with the current cursor, which replays or resyncs.
    Gap { expected: Seq },
}

/// The client's cursor in one team's log.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientCursor {
    cursor: Option<Cursor>,
}

impl ClientCursor {
    /// `None` until the first `welcome`; then sent as `hello.lastSeq`.
    pub fn cursor(&self) -> Option<&Cursor> {
        self.cursor.as_ref()
    }

    /// A `welcome`: after a resync the snapshot is complete at `head`; after a replay of
    /// another stream the old cursor is worthless. Within the same stream the cursor stays
    /// where it is: replayed events follow and are observed one by one.
    pub fn welcome(&mut self, stream: &StreamId, head: Seq, after_resync: bool) {
        let same_stream = self.cursor.as_ref().is_some_and(|c| c.stream == *stream);
        if after_resync || !same_stream {
            self.cursor = Some(Cursor {
                stream: stream.clone(),
                seq: head,
            });
        }
    }

    /// `resync_required`: the client drops its team state; the cursor is set by the `welcome`
    /// that ends the snapshot.
    pub fn resync(&mut self) {
        self.cursor = None;
    }

    /// An event with `seq` arrived.
    pub fn observe(&mut self, seq: Seq) -> Observed {
        let Some(cursor) = &mut self.cursor else {
            // Events before any welcome: the server broke the order; start over.
            return Observed::Gap { expected: Seq(1) };
        };
        let expected = cursor.seq.next();
        if seq < expected {
            Observed::Duplicate
        } else if seq == expected {
            cursor.seq = seq;
            Observed::Next
        } else {
            Observed::Gap { expected }
        }
    }
}

/// Who sent an op: a retry is only a retry from the same device of the same member in the same
/// team. Op ids are chosen by clients, so two devices may pick the same one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OpKey {
    pub holder: Holder,
    pub team: TeamId,
    pub op: OpId,
}

/// What [`RecentOps::check`] found.
#[derive(Debug, PartialEq, Eq)]
pub enum Retry<'a, R> {
    /// Not seen (or forgotten): apply the intent.
    New,
    /// The same intent again: send this answer, apply nothing.
    Same(&'a R),
    /// The op id was used before with another body: refuse with `badRequest`, apply nothing.
    Mismatch,
}

/// The results of recent intents, so a retry with the same op id gets the same answer instead of
/// being applied twice. Each entry keeps the digest of the body ([`ClientMessage::digest`]), so
/// an op id reused for a different intent is refused rather than answered with an old result.
/// Bounded: the oldest are forgotten first. Only intents whose
/// [`ClientMessage::deduplicated`] is true go through it; the others are applied and answered
/// every time.
///
/// [`ClientMessage::digest`]: crate::wire::ClientMessage::digest
/// [`ClientMessage::deduplicated`]: crate::wire::ClientMessage::deduplicated
#[derive(Debug, Clone)]
pub struct RecentOps<R> {
    capacity: usize,
    order: VecDeque<OpKey>,
    results: HashMap<OpKey, ([u8; 32], R)>,
}

impl<R> RecentOps<R> {
    /// Remembers the answers to the newest `capacity` ops (at least one).
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            order: VecDeque::new(),
            results: HashMap::new(),
        }
    }

    /// Whether `key` with a body of `digest` was answered before.
    pub fn check(&self, key: &OpKey, digest: &[u8; 32]) -> Retry<'_, R> {
        match self.results.get(key) {
            None => Retry::New,
            Some((seen, result)) if seen == digest => Retry::Same(result),
            Some(_) => Retry::Mismatch,
        }
    }

    /// Remembers the answer to `key`; a known op keeps its first answer.
    pub fn remember(&mut self, key: OpKey, digest: [u8; 32], result: R) {
        if self.results.contains_key(&key) {
            return;
        }
        if self.order.len() == self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.results.remove(&oldest);
            }
        }
        self.order.push_back(key.clone());
        self.results.insert(key, (digest, result));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(name: &str) -> StreamId {
        StreamId::new(name).unwrap()
    }

    fn cursor(name: &str, seq: u64) -> Cursor {
        Cursor {
            stream: stream(name),
            seq: Seq(seq),
        }
    }

    fn seqs<T>(replay: Replay<'_, T>) -> Vec<u64> {
        match replay {
            Replay::Events(events) => events.into_iter().map(|(seq, _)| seq.0).collect(),
            Replay::Resync(reason) => panic!("resync: {reason:?}"),
        }
    }

    #[test]
    fn sequence_numbers_rise_from_one() {
        let mut log = EventLog::new(stream("s"), 3);
        assert_eq!(log.head(), Seq(0));
        assert_eq!(log.push("a"), Seq(1));
        assert_eq!(log.push("b"), Seq(2));
        assert_eq!(log.head(), Seq(2));
        assert_eq!(log.stream(), &stream("s"));
    }

    #[test]
    fn replay_and_resync_decisions() {
        let mut log = EventLog::new(stream("s"), 3);
        // An empty log: a client at 0 is up to date.
        assert_eq!(seqs(log.replay(Some(&cursor("s", 0)))), Vec::<u64>::new());
        for event in ["a", "b", "c", "d", "e"] {
            log.push(event);
        }
        // Window holds 3, 4, 5.
        assert_eq!(seqs(log.replay(Some(&cursor("s", 5)))), Vec::<u64>::new());
        assert_eq!(seqs(log.replay(Some(&cursor("s", 3)))), vec![4, 5]);
        // At the window's edge: the first missing event (3) is still there.
        assert_eq!(seqs(log.replay(Some(&cursor("s", 2)))), vec![3, 4, 5]);
        // One further back: event 2 is gone.
        assert_eq!(
            log.replay(Some(&cursor("s", 1))),
            Replay::Resync(ResyncReason::TooOld)
        );
        assert_eq!(log.replay(None), Replay::Resync(ResyncReason::NoCursor));
        assert_eq!(
            log.replay(Some(&cursor("other", 5))),
            Replay::Resync(ResyncReason::OtherStream)
        );
        assert_eq!(
            log.replay(Some(&cursor("s", 6))),
            Replay::Resync(ResyncReason::Ahead)
        );
    }

    #[test]
    fn a_resumed_log_replays_nothing_from_before() {
        let mut log = EventLog::<&str>::resume(stream("s"), Seq(10), 4);
        assert_eq!(seqs(log.replay(Some(&cursor("s", 10)))), Vec::<u64>::new());
        assert_eq!(
            log.replay(Some(&cursor("s", 9))),
            Replay::Resync(ResyncReason::TooOld)
        );
        assert_eq!(log.push("x"), Seq(11));
        assert_eq!(seqs(log.replay(Some(&cursor("s", 10)))), vec![11]);
    }

    #[test]
    fn client_cursor_follows_and_detects_gaps() {
        let mut client = ClientCursor::default();
        assert_eq!(client.observe(Seq(1)), Observed::Gap { expected: Seq(1) });
        client.welcome(&stream("s"), Seq(7), true);
        assert_eq!(client.cursor(), Some(&cursor("s", 7)));
        assert_eq!(client.observe(Seq(8)), Observed::Next);
        assert_eq!(client.observe(Seq(8)), Observed::Duplicate);
        assert_eq!(client.observe(Seq(5)), Observed::Duplicate);
        assert_eq!(client.observe(Seq(10)), Observed::Gap { expected: Seq(9) });
        assert_eq!(client.cursor(), Some(&cursor("s", 8)));
        // A normal welcome in the same stream keeps the cursor: the replay follows.
        client.welcome(&stream("s"), Seq(10), false);
        assert_eq!(client.cursor(), Some(&cursor("s", 8)));
        assert_eq!(client.observe(Seq(9)), Observed::Next);
        // Another stream moves the cursor to its head.
        client.welcome(&stream("t"), Seq(2), false);
        assert_eq!(client.cursor(), Some(&cursor("t", 2)));
        client.resync();
        assert_eq!(client.cursor(), None);
    }

    #[test]
    fn retried_ops_get_their_first_answer() {
        use crate::ids::{DeviceId, MemberId};
        let mut ops = RecentOps::new(2);
        let holder = |member: &str| Holder {
            member: MemberId::new(member).unwrap(),
            device: DeviceId::new("d").unwrap(),
        };
        let key = |member: &str, op: &str| OpKey {
            holder: holder(member),
            team: TeamId::new("t").unwrap(),
            op: OpId::new(op).unwrap(),
        };
        let body = [1u8; 32];
        let other = [2u8; 32];
        assert_eq!(ops.check(&key("m", "a"), &body), Retry::New);
        ops.remember(key("m", "a"), body, "ack 1");
        ops.remember(key("m", "a"), body, "ack 2");
        assert_eq!(ops.check(&key("m", "a"), &body), Retry::Same(&"ack 1"));
        // The same op id with another body is not a retry.
        assert_eq!(ops.check(&key("m", "a"), &other), Retry::Mismatch);
        // Another member's op with the same id is a different op.
        assert_eq!(ops.check(&key("n", "a"), &body), Retry::New);
        let mut in_other_team = key("m", "a");
        in_other_team.team = TeamId::new("u").unwrap();
        assert_eq!(ops.check(&in_other_team, &body), Retry::New);
        ops.remember(key("m", "b"), body, "ack b");
        ops.remember(key("m", "c"), body, "ack c");
        // The oldest is forgotten first.
        assert_eq!(ops.check(&key("m", "a"), &body), Retry::New);
        assert_eq!(ops.check(&key("m", "b"), &body), Retry::Same(&"ack b"));
        assert_eq!(ops.check(&key("m", "c"), &body), Retry::Same(&"ack c"));
    }
}

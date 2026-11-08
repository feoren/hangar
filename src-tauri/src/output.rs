//! Building blocks for the output stream: batching, UTF-8 at chunk boundaries, backpressure.

use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::lock;

/// Collects bytes and releases them in batches: at most `max_delay` after the first byte,
/// or as soon as `max_bytes` is reached. Fewer, larger messages keep the UI responsive.
#[derive(Debug)]
pub struct Batcher {
    pending: Vec<u8>,
    first_at: Option<Instant>,
    max_delay: Duration,
    max_bytes: usize,
}

impl Batcher {
    pub fn new(max_delay: Duration, max_bytes: usize) -> Self {
        Self {
            pending: Vec::new(),
            first_at: None,
            max_delay,
            max_bytes,
        }
    }

    pub fn push(&mut self, data: &[u8], now: Instant) {
        if data.is_empty() {
            return;
        }
        if self.pending.is_empty() {
            self.first_at = Some(now);
        }
        self.pending.extend_from_slice(data);
    }

    pub fn ready(&self, now: Instant) -> bool {
        self.pending.len() >= self.max_bytes
            || self.deadline().is_some_and(|deadline| now >= deadline)
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.first_at.map(|first| first + self.max_delay)
    }

    pub fn take(&mut self) -> Vec<u8> {
        self.first_at = None;
        std::mem::take(&mut self.pending)
    }
}

/// Decodes a byte stream as UTF-8, even when a character is split across two reads.
/// Incomplete sequences at the end are held back until the next call; truly invalid
/// bytes become U+FFFD.
#[derive(Debug, Default)]
pub struct Utf8Carry {
    carry: Vec<u8>,
}

impl Utf8Carry {
    pub fn decode(&mut self, bytes: &[u8]) -> String {
        let mut buffer = std::mem::take(&mut self.carry);
        buffer.extend_from_slice(bytes);
        let split = incomplete_tail_start(&buffer);
        self.carry = buffer.split_off(split);
        String::from_utf8_lossy(&buffer).into_owned()
    }

    pub fn finish(&mut self) -> String {
        let rest = std::mem::take(&mut self.carry);
        String::from_utf8_lossy(&rest).into_owned()
    }
}

/// Index from which `bytes` ends with a started but incomplete UTF-8 sequence,
/// otherwise `bytes.len()`.
fn incomplete_tail_start(bytes: &[u8]) -> usize {
    let len = bytes.len();
    for back in 1..=len.min(3) {
        let index = len - back;
        let byte = bytes[index];
        if byte & 0xC0 == 0x80 {
            continue; // continuation byte, keep looking for the start byte
        }
        let needed = match byte {
            0xF0..=0xFF => 4,
            0xE0..=0xEF => 3,
            0xC0..=0xDF => 2,
            _ => 1,
        };
        return if needed > back { index } else { len };
    }
    len
}

/// Backpressure towards the frontend: the backend counts bytes sent but not yet acknowledged and
/// pauses output while more than `high` bytes are in flight. xterm.js acknowledges as soon as it
/// has processed a chunk.
#[derive(Debug)]
pub struct Flow {
    state: Mutex<FlowState>,
    signal: Condvar,
    high: usize,
    stall: Duration,
}

#[derive(Debug, Default)]
struct FlowState {
    unacked: usize,
    closed: bool,
}

impl Flow {
    pub fn new(high: usize, stall: Duration) -> Self {
        Self {
            state: Mutex::default(),
            signal: Condvar::new(),
            high,
            stall,
        }
    }

    pub fn add(&self, bytes: usize) {
        lock(&self.state).unacked += bytes;
    }

    pub fn ack(&self, bytes: usize) {
        let mut state = lock(&self.state);
        state.unacked = state.unacked.saturating_sub(bytes);
        self.signal.notify_all();
    }

    pub fn close(&self) {
        lock(&self.state).closed = true;
        self.signal.notify_all();
    }

    pub fn unacked(&self) -> usize {
        lock(&self.state).unacked
    }

    /// Waits until there is room again. Returns `false` if the stream was closed.
    /// If acknowledgements stay away longer than `stall` (for example after a frontend reload), the
    /// backlog counts as lost; otherwise the session would stall forever.
    pub fn wait_below_high(&self) -> bool {
        let deadline = Instant::now() + self.stall;
        let mut state = lock(&self.state);
        while state.unacked > self.high && !state.closed {
            let now = Instant::now();
            if now >= deadline {
                state.unacked = 0;
                break;
            }
            let (guard, _) = self
                .signal
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(PoisonError::into_inner);
            state = guard;
        }
        !state.closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn batcher_releases_after_time() {
        let t0 = Instant::now();
        let mut b = Batcher::new(Duration::from_millis(10), 1000);
        assert!(!b.ready(t0));
        assert_eq!(b.deadline(), None);
        b.push(b"ab", t0);
        b.push(b"cd", t0 + Duration::from_millis(5));
        assert!(!b.ready(t0 + Duration::from_millis(9)));
        assert_eq!(b.deadline(), Some(t0 + Duration::from_millis(10)));
        assert!(b.ready(t0 + Duration::from_millis(10)));
        assert_eq!(b.take(), b"abcd");
        assert_eq!(b.deadline(), None);
        assert!(!b.ready(t0 + Duration::from_secs(1)));
    }

    #[test]
    fn batcher_releases_after_size() {
        let t0 = Instant::now();
        let mut b = Batcher::new(Duration::from_secs(10), 4);
        b.push(b"abc", t0);
        assert!(!b.ready(t0));
        b.push(b"d", t0);
        assert!(b.ready(t0));
    }

    #[test]
    fn batcher_ignores_empty_data() {
        let mut b = Batcher::new(Duration::from_millis(1), 10);
        b.push(b"", Instant::now());
        assert_eq!(b.deadline(), None);
    }

    #[test]
    fn utf8_across_chunk_boundary() {
        let text = "aé€😀z";
        let bytes = text.as_bytes();
        // Every possible split point must yield the same text.
        for split in 0..=bytes.len() {
            let mut carry = Utf8Carry::default();
            let mut out = carry.decode(&bytes[..split]);
            out += &carry.decode(&bytes[split..]);
            out += &carry.finish();
            assert_eq!(out, text, "split at {split}");
        }
    }

    #[test]
    fn utf8_byte_by_byte() {
        let text = "Café äöü 😀";
        let mut carry = Utf8Carry::default();
        let out: String = text
            .as_bytes()
            .iter()
            .map(|b| carry.decode(&[*b]))
            .collect();
        assert_eq!(out + &carry.finish(), text);
    }

    #[test]
    fn utf8_invalid_bytes_are_replaced() {
        let mut carry = Utf8Carry::default();
        assert_eq!(carry.decode(b"a\xFFb"), "a\u{FFFD}b");
        // Truncated sequence at the end of the stream
        assert_eq!(carry.decode(b"x\xE2\x82"), "x");
        assert_eq!(carry.finish(), "\u{FFFD}");
    }

    #[test]
    fn flow_waits_for_ack() {
        let flow = Arc::new(Flow::new(10, Duration::from_secs(5)));
        flow.add(11);
        let waiter = {
            let flow = Arc::clone(&flow);
            std::thread::spawn(move || flow.wait_below_high())
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(!waiter.is_finished());
        flow.ack(5);
        assert!(waiter.join().unwrap());
        assert_eq!(flow.unacked(), 6);
    }

    #[test]
    fn flow_close_ends_waiting() {
        let flow = Arc::new(Flow::new(0, Duration::from_secs(5)));
        flow.add(1);
        let waiter = {
            let flow = Arc::clone(&flow);
            std::thread::spawn(move || flow.wait_below_high())
        };
        std::thread::sleep(Duration::from_millis(20));
        flow.close();
        assert!(!waiter.join().unwrap());
    }

    #[test]
    fn flow_stall_without_ack_resolves() {
        let flow = Flow::new(0, Duration::from_millis(30));
        flow.add(100);
        assert!(flow.wait_below_high());
        assert_eq!(flow.unacked(), 0);
    }

    #[test]
    fn flow_ack_does_not_underflow() {
        let flow = Flow::new(0, Duration::from_millis(1));
        flow.ack(10);
        assert_eq!(flow.unacked(), 0);
    }
}

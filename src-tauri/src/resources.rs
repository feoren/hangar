//! CPU and memory of each session's whole process tree, for the sidebar (contract:
//! docs/wiki/pages/syntheses/session-resources-and-phone-push.md).
//!
//! The window asks every few seconds while it is visible and the setting is on
//! (`session_resources`); nothing samples in between. Each answer turns the CPU time the tree has
//! used since the previous answer into a share of the whole machine, like the task manager. A
//! session is an outlier when its tree keeps at least [`BUSY_CORES`] processors busy for
//! [`CPU_SUSTAIN`] (a whole core for a long time, whatever the machine's size) or holds more than
//! [`MEMORY_LIMIT`]. Answers are cached for [`CACHE_FOR`], so several callers do not multiply the
//! system calls.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::lock;
use crate::proctree::TreeUsage;
use crate::session::SessionId;

/// Processors' worth of CPU time (core equivalents) from which a session counts as busy. Not a
/// share of the machine: on 16 processors a runaway single-threaded loop is only 6 %.
pub const BUSY_CORES: f64 = 0.9;
/// How long a session must stay busy before it is an outlier.
pub const CPU_SUSTAIN: Duration = Duration::from_secs(30);
/// Resident memory above which a session is an outlier at once.
pub const MEMORY_LIMIT: u64 = 4 * 1024 * 1024 * 1024;
/// A second answer within this time repeats the first one without reading again.
pub const CACHE_FOR: Duration = Duration::from_secs(1);

/// What the window shows for one session.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResources {
    pub id: SessionId,
    /// Share of the whole machine in percent (0 to 100), one decimal; `None` on the first
    /// answer, which has nothing to compare with.
    pub cpu: Option<f64>,
    /// Resident memory in bytes.
    pub memory: u64,
    pub outlier: bool,
}

#[derive(Debug, Clone, Copy)]
struct Track {
    cpu: Duration,
    at: Instant,
    /// Since when the session has kept [`BUSY_CORES`] busy without a break.
    busy_since: Option<Instant>,
}

/// The previous reading per session; sessions that are gone are forgotten on the next answer.
#[derive(Debug)]
pub struct ResourceMonitor {
    tracks: Mutex<HashMap<SessionId, Track>>,
    /// The last answer and when it was taken ([`CACHE_FOR`]).
    last: Mutex<Option<(Instant, Vec<SessionResources>)>>,
    /// Logical processors; the CPU time of all of them together is 100 %.
    cpus: u32,
}

impl Default for ResourceMonitor {
    fn default() -> Self {
        let cpus = std::thread::available_parallelism()
            .map_or(1, |count| u32::try_from(count.get()).unwrap_or(u32::MAX));
        Self::new(cpus)
    }
}

impl ResourceMonitor {
    pub fn new(cpus: u32) -> Self {
        Self {
            tracks: Mutex::default(),
            last: Mutex::default(),
            cpus: cpus.max(1),
        }
    }

    /// The answer for `now`: the cached one when it is younger than [`CACHE_FOR`], otherwise
    /// `read` runs (outside every lock of this monitor) and its readings are turned into a new one.
    pub fn sample(
        &self,
        now: Instant,
        read: impl FnOnce() -> Vec<(SessionId, TreeUsage)>,
    ) -> Vec<SessionResources> {
        if let Some((at, answer)) = lock(&self.last).as_ref() {
            if now.saturating_duration_since(*at) < CACHE_FOR {
                return answer.clone();
            }
        }
        let answer = self.update(now, read());
        *lock(&self.last) = Some((now, answer.clone()));
        answer
    }

    /// Turns the readings taken `now` into what the window shows, in the same order.
    pub fn update(
        &self,
        now: Instant,
        readings: Vec<(SessionId, TreeUsage)>,
    ) -> Vec<SessionResources> {
        let mut tracks = lock(&self.tracks);
        let mut next = HashMap::with_capacity(readings.len());
        let answer = readings
            .into_iter()
            .map(|(id, usage)| {
                let (track, shown) = step(tracks.get(&id).copied(), usage, now, self.cpus);
                next.insert(id, track);
                SessionResources {
                    id,
                    cpu: shown.cpu,
                    memory: usage.memory,
                    outlier: shown.outlier,
                }
            })
            .collect();
        *tracks = next;
        answer
    }
}

struct Shown {
    cpu: Option<f64>,
    outlier: bool,
}

/// One session's new track and what it shows, from its previous track and the new reading.
fn step(previous: Option<Track>, usage: TreeUsage, now: Instant, cpus: u32) -> (Track, Shown) {
    let fresh = Track {
        cpu: usage.cpu,
        at: now,
        busy_since: None,
    };
    let memory_high = usage.memory > MEMORY_LIMIT;
    let Some(previous) = previous else {
        let shown = Shown {
            cpu: None,
            outlier: memory_high,
        };
        return (fresh, shown);
    };
    let wall = now.saturating_duration_since(previous.at);
    if wall.is_zero() {
        // Two answers at the same instant: nothing new to compare.
        return (
            previous,
            Shown {
                cpu: None,
                outlier: memory_high,
            },
        );
    }
    let used = usage.cpu.saturating_sub(previous.cpu);
    let cpu = share(used, wall, cpus);
    // The use is the average since the previous answer, also after a pause (window hidden):
    // a busy stretch starts at the previous reading.
    let cores = used.as_secs_f64() / wall.as_secs_f64();
    let busy_since = (cores >= BUSY_CORES).then(|| previous.busy_since.unwrap_or(previous.at));
    let cpu_high =
        busy_since.is_some_and(|since| now.saturating_duration_since(since) >= CPU_SUSTAIN);
    let track = Track {
        busy_since,
        ..fresh
    };
    let shown = Shown {
        cpu: Some(cpu),
        outlier: memory_high || cpu_high,
    };
    (track, shown)
}

/// CPU time used over `wall` as a share of all processors, in percent with one decimal.
fn share(used: Duration, wall: Duration, cpus: u32) -> f64 {
    let capacity = wall.as_secs_f64() * f64::from(cpus);
    let percent = (used.as_secs_f64() / capacity * 100.0).clamp(0.0, 100.0);
    (percent * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;

    fn usage(cpu_ms: u64, memory: u64) -> TreeUsage {
        TreeUsage {
            cpu: Duration::from_millis(cpu_ms),
            memory,
        }
    }

    fn at(start: Instant, secs: u64) -> Instant {
        start + Duration::from_secs(secs)
    }

    #[test]
    fn first_answer_has_memory_but_no_cpu() {
        let monitor = ResourceMonitor::new(4);
        let now = Instant::now();
        let answer = monitor.update(now, vec![(1, usage(5_000, 340 * MB))]);
        assert_eq!(
            answer,
            vec![SessionResources {
                id: 1,
                cpu: None,
                memory: 340 * MB,
                outlier: false,
            }]
        );
    }

    #[test]
    fn cpu_is_a_share_of_the_whole_machine() {
        let monitor = ResourceMonitor::new(4);
        let start = Instant::now();
        monitor.update(start, vec![(1, usage(0, MB))]);
        // 1 s of CPU time in 2 s on 4 processors: 12.5 %.
        let answer = monitor.update(at(start, 2), vec![(1, usage(1_000, MB))]);
        assert_eq!(answer[0].cpu, Some(12.5));
    }

    #[test]
    fn share_is_clamped_and_rounded() {
        assert_eq!(
            share(Duration::from_secs(10), Duration::from_secs(1), 2),
            100.0
        );
        assert_eq!(
            share(Duration::from_millis(1), Duration::from_secs(3), 1),
            0.0
        );
        assert_eq!(
            share(Duration::from_millis(333), Duration::from_secs(1), 1),
            33.3
        );
    }

    #[test]
    fn a_tree_whose_time_drops_counts_as_idle() {
        // A process that ended without being waited for takes its time along (Linux).
        let monitor = ResourceMonitor::new(1);
        let start = Instant::now();
        monitor.update(start, vec![(1, usage(9_000, MB))]);
        let answer = monitor.update(at(start, 2), vec![(1, usage(4_000, MB))]);
        assert_eq!(answer[0].cpu, Some(0.0));
    }

    #[test]
    fn busy_for_thirty_seconds_is_an_outlier() {
        let monitor = ResourceMonitor::new(1);
        let start = Instant::now();
        let mut cpu = 0;
        monitor.update(start, vec![(1, usage(cpu, MB))]);
        let mut outliers = Vec::new();
        for second in (2..=34).step_by(2) {
            cpu += 1_950; // 97.5 % of the one processor
            let answer = monitor.update(at(start, second), vec![(1, usage(cpu, MB))]);
            outliers.push((second, answer[0].outlier));
        }
        // Busy since the first reading at 0 s: an outlier from 30 s on.
        for (second, outlier) in outliers {
            assert_eq!(outlier, second >= 30, "at {second} s");
        }
    }

    #[test]
    fn a_quiet_moment_resets_the_busy_window() {
        let monitor = ResourceMonitor::new(1);
        let start = Instant::now();
        monitor.update(start, vec![(1, usage(0, MB))]);
        monitor.update(at(start, 20), vec![(1, usage(19_500, MB))]);
        // 2 s at 10 %: no longer busy.
        monitor.update(at(start, 22), vec![(1, usage(19_700, MB))]);
        let answer = monitor.update(at(start, 40), vec![(1, usage(37_200, MB))]);
        assert!(!answer[0].outlier, "busy again only since 22 s");
        let answer = monitor.update(at(start, 52), vec![(1, usage(48_900, MB))]);
        assert!(answer[0].outlier, "busy for 30 s since 22 s");
    }

    #[test]
    fn a_long_pause_counts_by_its_average() {
        let monitor = ResourceMonitor::new(1);
        let start = Instant::now();
        monitor.update(start, vec![(1, usage(0, MB))]);
        monitor.update(at(start, 2), vec![(1, usage(1_950, MB))]);
        // The window was hidden for a minute and the tree used nearly all of it.
        let answer = monitor.update(at(start, 62), vec![(1, usage(60_000, MB))]);
        assert!(answer[0].outlier, "60 s at 96.75 %");
        let answer = monitor.update(at(start, 122), vec![(1, usage(61_000, MB))]);
        assert!(!answer[0].outlier, "the next minute was quiet");
    }

    #[test]
    fn a_whole_core_is_busy_on_a_large_machine() {
        // 16 processors: one core is 6.25 % of the machine, still an outlier after 30 s.
        let monitor = ResourceMonitor::new(16);
        let start = Instant::now();
        monitor.update(start, vec![(1, usage(0, MB)), (2, usage(0, MB))]);
        let answer = monitor.update(
            at(start, 30),
            vec![(1, usage(30_000, MB)), (2, usage(15_000, MB))],
        );
        assert_eq!(answer[0].cpu, Some(6.3));
        assert!(answer[0].outlier, "one core for 30 s");
        assert!(!answer[1].outlier, "half a core is no outlier");
    }

    #[test]
    fn answers_are_cached_for_a_second() {
        let monitor = ResourceMonitor::new(1);
        let start = Instant::now();
        let reads = std::cell::Cell::new(0);
        let read = || {
            reads.set(reads.get() + 1);
            vec![(1, usage(0, MB))]
        };
        monitor.sample(start, read);
        monitor.sample(start + Duration::from_millis(900), read);
        assert_eq!(reads.get(), 1);
        monitor.sample(start + CACHE_FOR, read);
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn much_memory_is_an_outlier_at_once() {
        let monitor = ResourceMonitor::new(8);
        let answer = monitor.update(Instant::now(), vec![(1, usage(0, MEMORY_LIMIT + 1))]);
        assert!(answer[0].outlier);
        let answer = monitor.update(Instant::now(), vec![(2, usage(0, MEMORY_LIMIT))]);
        assert!(!answer[0].outlier, "at the limit is still fine");
    }

    #[test]
    fn closed_sessions_are_forgotten() {
        let monitor = ResourceMonitor::new(1);
        let start = Instant::now();
        monitor.update(start, vec![(1, usage(0, MB)), (2, usage(0, MB))]);
        monitor.update(at(start, 2), vec![(2, usage(100, MB))]);
        // Session 1 comes back (same id after a restart): a first reading again.
        let answer = monitor.update(at(start, 4), vec![(1, usage(500, MB))]);
        assert_eq!(answer[0].cpu, None);
        assert_eq!(lock(&monitor.tracks).len(), 1);
    }
}

//! Load of the whole machine (CPU, memory, GPU) for the sidebar's system bar (contract:
//! docs/wiki/pages/syntheses/session-resources-and-phone-push.md).
//!
//! Like the session resources (`resources.rs`), nothing samples on its own: the window asks
//! (`system_usage`) while it is visible and the setting is on, and an answer younger than
//! [`CACHE_FOR`] is repeated. CPU is the busy share of all processors since the previous answer,
//! memory the share of physical memory in use, GPU the busiest engine of any adapter, as the task
//! manager shows it, and the dedicated GPU memory in use. Memory and GPU memory also come as
//! bytes. A value the platform cannot give stays `None`, and the window hides it.
//!
//! - Windows: `GetSystemTimes`, `GlobalMemoryStatusEx`; the GPU from the shared PDH reading
//!   (`gpu.rs`).
//! - Linux: `/proc/stat` and `/proc/meminfo`; no GPU (no vendor-neutral source).
//! - Other systems: nothing yet.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::gpu::GpuSnapshot;
use crate::lock;

/// A second answer within this time repeats the first one without reading again.
pub const CACHE_FOR: Duration = Duration::from_secs(1);

/// What the system bar shows; `None` where unknown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemUsage {
    /// Busy share of all processors in percent (0 to 100, one decimal).
    pub cpu: Option<f64>,
    /// Share of physical memory in use in percent (0 to 100, one decimal).
    pub memory: Option<f64>,
    /// Physical memory in use and in total, in bytes.
    pub memory_used: Option<u64>,
    pub memory_total: Option<u64>,
    /// Busiest GPU engine in percent (0 to 100, one decimal).
    pub gpu: Option<f64>,
    /// Dedicated GPU memory in use on all adapters, in bytes.
    pub gpu_memory: Option<u64>,
}

/// Processor time of the whole machine so far, in any unit as long as both use the same one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuTimes {
    /// Time all processors spent idle.
    pub idle: u64,
    /// Time all processors existed (idle included).
    pub total: u64,
}

/// Busy share of the processors between two readings; `None` when no time passed or the
/// counters went backwards (a wrapped or reset counter says nothing).
pub fn cpu_percent(previous: CpuTimes, now: CpuTimes) -> Option<f64> {
    let total = now.total.checked_sub(previous.total)?;
    let idle = now.idle.checked_sub(previous.idle)?;
    if total == 0 {
        return None;
    }
    let busy = total.saturating_sub(idle);
    Some(percent(busy as f64 / total as f64))
}

/// Share of physical memory in use; `None` without a total.
pub fn memory_percent(total: u64, available: u64) -> Option<f64> {
    if total == 0 {
        return None;
    }
    let used = total.saturating_sub(available);
    Some(percent(used as f64 / total as f64))
}

/// A fraction as a percentage with one decimal, clamped to 0 to 100.
fn percent(fraction: f64) -> f64 {
    let value = (fraction * 100.0).clamp(0.0, 100.0);
    (value * 10.0).round() / 10.0
}

/// The aggregate `cpu` line of `/proc/stat`: idle is `idle` plus `iowait`, total the first eight
/// fields (`guest` and `guest_nice` are already part of `user` and `nice`).
pub fn parse_proc_stat(text: &str) -> Option<CpuTimes> {
    let line = text.lines().find(|line| line.starts_with("cpu "))?;
    let fields: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .take(8)
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if fields.len() < 4 {
        return None;
    }
    let idle = fields[3] + fields.get(4).copied().unwrap_or(0);
    Some(CpuTimes {
        idle,
        total: fields.iter().sum(),
    })
}

/// `MemTotal` and `MemAvailable` of `/proc/meminfo`, in kB.
pub fn parse_meminfo(text: &str) -> Option<(u64, u64)> {
    let field = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
            .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
    };
    Some((field("MemTotal")?, field("MemAvailable")?))
}

/// The previous CPU reading and the last answer.
#[derive(Default)]
struct State {
    cpu: Option<CpuTimes>,
    last: Option<(Instant, SystemUsage)>,
}

/// Managed state behind `system_usage`; one shared poll for every caller.
#[derive(Default)]
pub struct SystemMonitor {
    state: Mutex<State>,
}

impl SystemMonitor {
    /// The answer for `now`: the cached one when it is younger than [`CACHE_FOR`], otherwise a
    /// fresh reading, with the GPU from `gpu` (the shared reading of `gpu.rs`). The lock is
    /// held while reading, so a second caller waits and gets the cached answer instead of a
    /// second reading.
    pub fn sample(
        &self,
        now: Instant,
        gpu: impl FnOnce() -> Option<Arc<GpuSnapshot>>,
    ) -> SystemUsage {
        let mut state = lock(&self.state);
        if let Some((at, answer)) = state.last {
            if now.saturating_duration_since(at) < CACHE_FOR {
                return answer;
            }
        }
        let times = platform::cpu_times();
        let cpu = match (state.cpu, times) {
            (Some(previous), Some(now)) => cpu_percent(previous, now),
            _ => None,
        };
        state.cpu = times;
        let memory = platform::memory();
        let gpu = gpu();
        let answer = SystemUsage {
            cpu,
            memory: memory.and_then(|(total, available)| memory_percent(total, available)),
            memory_used: memory.map(|(total, available)| total.saturating_sub(available)),
            memory_total: memory.map(|(total, _)| total),
            gpu: gpu.as_ref().and_then(|snapshot| snapshot.load),
            gpu_memory: gpu.as_ref().and_then(|snapshot| snapshot.memory),
        };
        state.last = Some((now, answer));
        answer
    }
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetSystemTimes;

    use super::CpuTimes;

    fn ticks(time: FILETIME) -> u64 {
        (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
    }

    pub fn cpu_times() -> Option<CpuTimes> {
        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: The three pointers point at live FILETIME values the call fills in.
        let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
        if ok == 0 {
            return None;
        }
        // Kernel time includes the idle time.
        Some(CpuTimes {
            idle: ticks(idle),
            total: ticks(kernel).wrapping_add(ticks(user)),
        })
    }

    /// Total and available physical memory in bytes.
    pub fn memory() -> Option<(u64, u64)> {
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..MEMORYSTATUSEX::default()
        };
        // SAFETY: `status` is a live MEMORYSTATUSEX whose dwLength names its size, as required.
        let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
        (ok != 0).then_some((status.ullTotalPhys, status.ullAvailPhys))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{parse_meminfo, parse_proc_stat, CpuTimes};

    pub fn cpu_times() -> Option<CpuTimes> {
        parse_proc_stat(&std::fs::read_to_string("/proc/stat").ok()?)
    }

    /// Total and available physical memory in bytes (`/proc/meminfo` counts in kB).
    pub fn memory() -> Option<(u64, u64)> {
        let (total, available) = parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)?;
        Some((total.saturating_mul(1024), available.saturating_mul(1024)))
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::CpuTimes;

    pub fn cpu_times() -> Option<CpuTimes> {
        None
    }

    pub fn memory() -> Option<(u64, u64)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_is_the_busy_share_between_readings() {
        let before = CpuTimes {
            idle: 1_000,
            total: 2_000,
        };
        let after = CpuTimes {
            idle: 1_750,
            total: 3_000,
        };
        // 1000 ticks passed, 750 of them idle: 25 % busy.
        assert_eq!(cpu_percent(before, after), Some(25.0));
    }

    #[test]
    fn cpu_without_time_or_with_counters_going_back_says_nothing() {
        let at = CpuTimes {
            idle: 10,
            total: 20,
        };
        assert_eq!(cpu_percent(at, at), None);
        let back = CpuTimes { idle: 5, total: 30 };
        assert_eq!(cpu_percent(at, back), None);
    }

    #[test]
    fn cpu_is_rounded_and_clamped() {
        let before = CpuTimes { idle: 0, total: 0 };
        let third = CpuTimes { idle: 2, total: 3 };
        assert_eq!(cpu_percent(before, third), Some(33.3));
        // More idle than total time (rounding in the counters): 0 %, never negative.
        let odd = CpuTimes { idle: 10, total: 5 };
        assert_eq!(cpu_percent(before, odd), Some(0.0));
    }

    #[test]
    fn memory_is_the_share_in_use() {
        assert_eq!(memory_percent(16_000, 4_000), Some(75.0));
        assert_eq!(memory_percent(3, 2), Some(33.3));
        assert_eq!(memory_percent(8, 9), Some(0.0));
        assert_eq!(memory_percent(0, 0), None);
    }

    #[test]
    fn proc_stat_counts_iowait_as_idle() {
        let text = "cpu  100 5 50 800 40 3 2 0 7 0\ncpu0 50 2 25 400 20 1 1 0 3 0\nintr 1\n";
        let times = parse_proc_stat(text).expect("aggregate line");
        assert_eq!(times.idle, 840);
        // Guest time is part of user time already and not counted twice.
        assert_eq!(times.total, 100 + 5 + 50 + 800 + 40 + 3 + 2);
        assert_eq!(parse_proc_stat("cpu0 1 2 3 4\n"), None);
        assert_eq!(parse_proc_stat("cpu  1 x 3 4\n"), None);
    }

    #[test]
    fn meminfo_reads_total_and_available() {
        let text = "MemTotal:       16000000 kB\nMemFree:  1000 kB\nMemAvailable:    4000000 kB\n";
        assert_eq!(parse_meminfo(text), Some((16_000_000, 4_000_000)));
        assert_eq!(parse_meminfo("MemTotal: 5 kB\n"), None);
    }

    #[test]
    fn gpu_comes_from_the_shared_reading() {
        let monitor = SystemMonitor::default();
        let engine = "pid_1_luid_0x00000000_0x0000D1B5_phys_0_eng_0_engtype_3D";
        let adapter = "luid_0x00000000_0x0000D1B5_phys_0";
        let snapshot = GpuSnapshot::from_counters(
            [(engine, 30.04)],
            std::iter::empty(),
            Some([(adapter, 2048.0)]),
        );
        let answer = monitor.sample(Instant::now(), || Some(Arc::new(snapshot)));
        assert_eq!(answer.gpu, Some(30.0));
        assert_eq!(answer.gpu_memory, Some(2048));
        if let (Some(used), Some(total)) = (answer.memory_used, answer.memory_total) {
            assert!(used <= total && total > 0);
        }
        let without = SystemMonitor::default().sample(Instant::now(), || None);
        assert_eq!((without.gpu, without.gpu_memory), (None, None));
    }

    #[test]
    fn answers_are_cached_for_a_second() {
        let monitor = SystemMonitor::default();
        let start = Instant::now();
        let first = monitor.sample(start, || None);
        let again = monitor.sample(start + Duration::from_millis(900), || None);
        assert_eq!(first, again);
        assert!(lock(&monitor.state).last.is_some_and(|(at, _)| at == start));
        monitor.sample(start + CACHE_FOR, || None);
        assert!(lock(&monitor.state)
            .last
            .is_some_and(|(at, _)| at == start + CACHE_FOR));
    }

    /// The real reading of this machine; prints the values and the time a reading takes.
    #[test]
    fn reads_this_machine() {
        let monitor = SystemMonitor::default();
        let start = Instant::now();
        let first = monitor.sample(start, || None);
        std::thread::sleep(Duration::from_millis(1_100));
        let begun = Instant::now();
        let second = monitor.sample(start + Duration::from_millis(1_100), || None);
        let took = begun.elapsed();
        println!("first {first:?}, second {second:?}, reading took {took:?}");
        assert_eq!(first.cpu, None, "no previous reading yet");
        if cfg!(any(windows, target_os = "linux")) {
            assert!(second.cpu.is_some_and(|cpu| (0.0..=100.0).contains(&cpu)));
            assert!(second
                .memory
                .is_some_and(|memory| memory > 0.0 && memory <= 100.0));
        }
        if let Some(gpu) = second.gpu {
            assert!((0.0..=100.0).contains(&gpu));
        }
    }
}

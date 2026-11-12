//! Load of the whole machine (CPU, memory, GPU) for the sidebar's system bar (contract:
//! docs/wiki/pages/syntheses/session-resources-and-phone-push.md).
//!
//! Like the session resources (`resources.rs`), nothing samples on its own: the window asks
//! (`system_usage`) while it is visible and the setting is on, and an answer younger than
//! [`CACHE_FOR`] is repeated. CPU is the busy share of all processors since the previous answer,
//! memory the share of physical memory in use, GPU the busiest engine of any adapter, as the task
//! manager shows it. A value the platform cannot give stays `None`, and the window hides it.
//!
//! - Windows: `GetSystemTimes`, `GlobalMemoryStatusEx` and the performance counter
//!   `\GPU Engine(*)\Utilization Percentage` through one PDH query that stays open.
//! - Linux: `/proc/stat` and `/proc/meminfo`; no GPU (no vendor-neutral source).
//! - Other systems: nothing yet.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::lock;

/// A second answer within this time repeats the first one without reading again.
pub const CACHE_FOR: Duration = Duration::from_secs(1);

/// What the system bar shows, each in percent (0 to 100, one decimal); `None` where unknown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemUsage {
    pub cpu: Option<f64>,
    pub memory: Option<f64>,
    pub gpu: Option<f64>,
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

/// GPU load from the instances of `\GPU Engine(*)\Utilization Percentage`, named like
/// `pid_1234_luid_0x00000000_0x0000D1B5_phys_0_eng_3_engtype_3D`: the processes' shares are
/// summed per engine (adapter, physical GPU and engine index), and the busiest engine is the
/// load. This is how the task manager's GPU column reads the same counters, so a video decode
/// running on its own engine shows as GPU load even while 3D is idle. `None` without any engine
/// (no GPU, or no data yet).
pub fn gpu_percent<'a>(instances: impl IntoIterator<Item = (&'a str, f64)>) -> Option<f64> {
    let mut engines: HashMap<&str, f64> = HashMap::new();
    for (name, value) in instances {
        let Some(engine) = engine_key(name) else {
            continue;
        };
        // A broken value counts as idle; the engine still exists.
        let value = if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        };
        *engines.entry(engine).or_default() += value;
    }
    engines
        .into_values()
        .reduce(f64::max)
        .map(|busiest| percent(busiest / 100.0))
}

/// The engine part of an instance name: from `luid_` up to the engine type, without the process.
fn engine_key(name: &str) -> Option<&str> {
    let start = name.find("luid_")?;
    let rest = &name[start..];
    let end = rest.find("_engtype_").unwrap_or(rest.len());
    Some(&rest[..end])
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

/// The previous CPU reading, the open GPU query and the last answer.
#[derive(Default)]
struct State {
    cpu: Option<CpuTimes>,
    last: Option<(Instant, SystemUsage)>,
    platform: platform::Sampler,
}

/// Managed state behind `system_usage`; one shared poll for every caller.
#[derive(Default)]
pub struct SystemMonitor {
    state: Mutex<State>,
}

impl SystemMonitor {
    /// The answer for `now`: the cached one when it is younger than [`CACHE_FOR`], otherwise a
    /// fresh reading. The lock is held while reading, so a second caller waits and gets the
    /// cached answer instead of a second reading.
    pub fn sample(&self, now: Instant) -> SystemUsage {
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
        let answer = SystemUsage {
            cpu,
            memory: platform::memory()
                .and_then(|(total, available)| memory_percent(total, available)),
            gpu: state.platform.gpu(),
        };
        state.last = Some((now, answer));
        answer
    }
}

#[cfg(windows)]
mod platform {
    use std::ptr;

    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W,
        PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    };
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetSystemTimes;

    use super::{gpu_percent, CpuTimes};

    /// Counter path in English, so it resolves on every display language.
    const GPU_COUNTER: &str = "\\GPU Engine(*)\\Utilization Percentage";
    /// Bytes of counter values read at most; a machine with that many engine instances is
    /// summed partially rather than allocating without bound.
    const MAX_BUFFER: u32 = 4 * 1024 * 1024;

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

    pub fn memory() -> Option<(u64, u64)> {
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..MEMORYSTATUSEX::default()
        };
        // SAFETY: `status` is a live MEMORYSTATUSEX whose dwLength names its size, as required.
        let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
        (ok != 0).then_some((status.ullTotalPhys, status.ullAvailPhys))
    }

    /// The GPU query: opened on the first reading, kept open, given up for good when the
    /// counter does not exist (no GPU driver with WDDM 2.0, say).
    #[derive(Default)]
    pub enum Sampler {
        #[default]
        Untried,
        Open(GpuQuery),
        Unavailable,
    }

    impl Sampler {
        pub fn gpu(&mut self) -> Option<f64> {
            if matches!(self, Self::Untried) {
                // The first collection only sets the rate's baseline: nothing to show yet.
                *self = GpuQuery::open().map_or(Self::Unavailable, Self::Open);
                return None;
            }
            match self {
                Self::Open(query) => query.read(),
                _ => None,
            }
        }
    }

    pub struct GpuQuery {
        query: PDH_HQUERY,
        counter: PDH_HCOUNTER,
    }

    // SAFETY: PDH query and counter handles are process-wide and not bound to the thread that
    // opened them; the monitor's mutex makes sure only one thread uses them at a time.
    unsafe impl Send for GpuQuery {}

    impl GpuQuery {
        fn open() -> Option<Self> {
            let mut query: PDH_HQUERY = ptr::null_mut();
            // SAFETY: A null data source means live data; `query` receives the handle.
            if unsafe { PdhOpenQueryW(ptr::null(), 0, &mut query) } != 0 {
                return None;
            }
            let path: Vec<u16> = GPU_COUNTER.encode_utf16().chain(Some(0)).collect();
            let mut counter: PDH_HCOUNTER = ptr::null_mut();
            // SAFETY: `query` is open, `path` is NUL-terminated and outlives the call.
            let status = unsafe { PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) };
            let opened = Self { query, counter };
            if status != 0 {
                return None; // Drop closes the query.
            }
            // The counter is a rate: the first collection only sets the baseline.
            // SAFETY: `query` is open.
            unsafe { PdhCollectQueryData(opened.query) };
            Some(opened)
        }

        /// Collects once more and sums the instances; `None` while no instance has valid data.
        fn read(&mut self) -> Option<f64> {
            // SAFETY: `query` is open for as long as `self` lives.
            if unsafe { PdhCollectQueryData(self.query) } != 0 {
                return None;
            }
            let mut size: u32 = 0;
            let mut count: u32 = 0;
            // SAFETY: A zero size with a null buffer asks for the size needed.
            let status = unsafe {
                PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_DOUBLE,
                    &mut size,
                    &mut count,
                    ptr::null_mut(),
                )
            };
            if status != PDH_MORE_DATA || size == 0 || size > MAX_BUFFER {
                return None;
            }
            // u64 words: the items hold f64 values and pointers, so the buffer needs their
            // alignment; the names the items point at follow inside the same buffer.
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            let items = buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            // SAFETY: `buffer` holds at least `size` bytes, aligned for the item type.
            let status = unsafe {
                PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_DOUBLE,
                    &mut size,
                    &mut count,
                    items,
                )
            };
            if status != 0 {
                return None;
            }
            let fits = (size as usize) <= buffer.len() * 8
                && (count as usize) * std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>()
                    <= buffer.len() * 8;
            if !fits {
                return None;
            }
            // SAFETY: PDH wrote `count` items at the start of `buffer` (checked to fit above).
            let items = unsafe { std::slice::from_raw_parts(items, count as usize) };
            let values: Vec<(String, f64)> = items
                .iter()
                .filter(|item| {
                    item.FmtValue.CStatus == PDH_CSTATUS_VALID_DATA
                        || item.FmtValue.CStatus == PDH_CSTATUS_NEW_DATA
                })
                .filter_map(|item| {
                    // SAFETY: The item's name is a NUL-terminated string inside `buffer`.
                    let name = unsafe { wide(item.szName) }?;
                    // SAFETY: PDH_FMT_DOUBLE was asked for, so the union holds a double.
                    let value = unsafe { item.FmtValue.Anonymous.doubleValue };
                    Some((name, value))
                })
                .collect();
            gpu_percent(values.iter().map(|(name, value)| (name.as_str(), *value)))
        }
    }

    /// A NUL-terminated UTF-16 string; `None` for a null pointer.
    ///
    /// # Safety
    /// `text` is null or points at a NUL-terminated UTF-16 string.
    unsafe fn wide(text: *const u16) -> Option<String> {
        if text.is_null() {
            return None;
        }
        let mut length = 0;
        // SAFETY: The caller guarantees a terminating NUL, which ends the walk.
        while unsafe { *text.add(length) } != 0 {
            length += 1;
        }
        // SAFETY: `length` units before the NUL were just read.
        let units = unsafe { std::slice::from_raw_parts(text, length) };
        Some(String::from_utf16_lossy(units))
    }

    impl Drop for GpuQuery {
        fn drop(&mut self) {
            // SAFETY: The query was opened by PdhOpenQueryW and is closed exactly once; closing it
            // also frees its counter.
            unsafe { PdhCloseQuery(self.query) };
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{parse_meminfo, parse_proc_stat, CpuTimes};

    pub fn cpu_times() -> Option<CpuTimes> {
        parse_proc_stat(&std::fs::read_to_string("/proc/stat").ok()?)
    }

    /// In kB, the unit cancels out in the share.
    pub fn memory() -> Option<(u64, u64)> {
        parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
    }

    #[derive(Default)]
    pub struct Sampler;

    impl Sampler {
        pub fn gpu(&mut self) -> Option<f64> {
            None
        }
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

    #[derive(Default)]
    pub struct Sampler;

    impl Sampler {
        pub fn gpu(&mut self) -> Option<f64> {
            None
        }
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

    const LUID: &str = "luid_0x00000000_0x0000D1B5_phys_0";

    fn engine(pid: u32, eng: u32, kind: &str) -> String {
        format!("pid_{pid}_{LUID}_eng_{eng}_engtype_{kind}")
    }

    #[test]
    fn gpu_sums_processes_per_engine_and_takes_the_busiest() {
        let names = [
            engine(10, 0, "3D"),
            engine(20, 0, "3D"),
            engine(10, 3, "VideoDecode"),
            engine(30, 1, "Copy"),
        ];
        let values = [12.0, 30.5, 55.0, 2.0];
        let samples = names.iter().map(String::as_str).zip(values);
        // 3D: 12 + 30.5 = 42.5; video decode 55 on its own engine is the busiest.
        assert_eq!(gpu_percent(samples), Some(55.0));
    }

    #[test]
    fn gpu_keeps_adapters_apart_and_caps_at_full() {
        let first = format!("pid_1_{LUID}_eng_0_engtype_3D");
        let second = "pid_1_luid_0x00000000_0x0000AAAA_phys_0_eng_0_engtype_3D".to_owned();
        let samples = [(first.as_str(), 70.0), (second.as_str(), 60.0)];
        assert_eq!(
            gpu_percent(samples),
            Some(70.0),
            "not 130 over two adapters"
        );
        let busy = [(first.as_str(), 80.0), (first.as_str(), 40.0)];
        assert_eq!(gpu_percent(busy), Some(100.0));
    }

    #[test]
    fn gpu_without_engines_is_unknown() {
        assert_eq!(gpu_percent(std::iter::empty()), None);
        assert_eq!(
            gpu_percent([("_Total", 5.0)]),
            None,
            "no engine in the name"
        );
        let idle = engine(1, 0, "3D");
        assert_eq!(gpu_percent([(idle.as_str(), 0.0)]), Some(0.0));
        assert_eq!(gpu_percent([(idle.as_str(), f64::NAN)]), Some(0.0));
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
    fn answers_are_cached_for_a_second() {
        let monitor = SystemMonitor::default();
        let start = Instant::now();
        let first = monitor.sample(start);
        let again = monitor.sample(start + Duration::from_millis(900));
        assert_eq!(first, again);
        assert!(lock(&monitor.state).last.is_some_and(|(at, _)| at == start));
        monitor.sample(start + CACHE_FOR);
        assert!(lock(&monitor.state)
            .last
            .is_some_and(|(at, _)| at == start + CACHE_FOR));
    }

    /// The real reading of this machine; prints the values and the time a reading takes.
    #[test]
    fn reads_this_machine() {
        let monitor = SystemMonitor::default();
        let start = Instant::now();
        let first = monitor.sample(start);
        std::thread::sleep(Duration::from_millis(1_100));
        let begun = Instant::now();
        let second = monitor.sample(start + Duration::from_millis(1_100));
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

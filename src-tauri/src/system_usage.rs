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
//! - Linux: `/proc/stat` and `/proc/meminfo`; the GPU from the shared reading where amdgpu or
//!   NVIDIA's `nvidia-smi` give it (`gpu.rs`).
//! - macOS: `host_statistics` (CPU ticks), `hw.memsize` and `host_statistics64` (pages in use,
//!   counted like Activity Monitor's "Memory Used"); the GPU from the shared reading (`gpu.rs`).
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

/// macOS `host_cpu_load_info` ticks in the order user, system, idle, nice (`CPU_STATE_*`).
pub fn cpu_times_from_ticks(ticks: [u32; 4]) -> CpuTimes {
    CpuTimes {
        idle: u64::from(ticks[2]),
        total: ticks.iter().map(|tick| u64::from(*tick)).sum(),
    }
}

/// The page counts of macOS `vm_statistics64` that make up the memory in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacPages {
    pub internal: u64,
    pub purgeable: u64,
    pub wired: u64,
    pub compressed: u64,
}

/// Available memory in bytes as Activity Monitor's "Memory Used" leaves it: in use is app memory
/// (anonymous pages minus purgeable ones), wired and compressed pages; file cache counts as
/// available, as `MemAvailable` does on Linux.
pub fn mac_available_memory(total: u64, pages: MacPages, page_size: u64) -> u64 {
    let used = pages
        .internal
        .saturating_sub(pages.purgeable)
        .saturating_add(pages.wired)
        .saturating_add(pages.compressed)
        .saturating_mul(page_size);
    total.saturating_sub(used)
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

#[cfg(target_os = "macos")]
mod platform {
    use std::sync::OnceLock;

    use super::{cpu_times_from_ticks, mac_available_memory, CpuTimes, MacPages};

    /// The host port, asked for once: every `mach_host_self` call adds a reference to it.
    #[allow(deprecated)] // libc points to the mach2 crate; these few bindings are enough.
    fn host() -> libc::mach_port_t {
        static HOST: OnceLock<libc::mach_port_t> = OnceLock::new();
        // SAFETY: mach_host_self has no preconditions and returns a send right to the host.
        *HOST.get_or_init(|| unsafe { libc::mach_host_self() })
    }

    pub fn cpu_times() -> Option<CpuTimes> {
        let mut load = libc::host_cpu_load_info { cpu_ticks: [0; 4] };
        let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
        // SAFETY: `load` is a live host_cpu_load_info and `count` its size in integer_t words,
        // as the flavor HOST_CPU_LOAD_INFO wants.
        let status = unsafe {
            libc::host_statistics(
                host(),
                libc::HOST_CPU_LOAD_INFO,
                (&raw mut load).cast(),
                &mut count,
            )
        };
        (status == libc::KERN_SUCCESS).then(|| cpu_times_from_ticks(load.cpu_ticks))
    }

    /// Total and available physical memory in bytes.
    pub fn memory() -> Option<(u64, u64)> {
        let mut total: u64 = 0;
        let mut size = std::mem::size_of::<u64>();
        // SAFETY: `total` is a writable u64 and `size` its length; the name is NUL-terminated.
        let status = unsafe {
            libc::sysctlbyname(
                c"hw.memsize".as_ptr(),
                (&raw mut total).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if status != 0 || size != std::mem::size_of::<u64>() {
            return None;
        }
        // SAFETY: An all-zero vm_statistics64 is a valid value of the plain counter struct.
        let mut stats: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
        let mut count = libc::HOST_VM_INFO64_COUNT;
        // SAFETY: `stats` is a live vm_statistics64 and `count` its size in integer_t words; an
        // older kernel fills fewer words and says so in `count`.
        let status = unsafe {
            libc::host_statistics64(
                host(),
                libc::HOST_VM_INFO64,
                (&raw mut stats).cast(),
                &mut count,
            )
        };
        if status != libc::KERN_SUCCESS {
            return None;
        }
        // SAFETY: sysconf only reads system configuration.
        let page = u64::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).ok()?;
        let pages = MacPages {
            internal: u64::from(stats.internal_page_count),
            purgeable: u64::from(stats.purgeable_count),
            wired: u64::from(stats.wire_count),
            compressed: u64::from(stats.compressor_page_count),
        };
        Some((total, mac_available_memory(total, pages, page)))
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
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
    fn mac_ticks_count_idle_apart() {
        let times = cpu_times_from_ticks([300, 100, 550, 50]);
        assert_eq!(
            times,
            CpuTimes {
                idle: 550,
                total: 1000
            }
        );
        let later = cpu_times_from_ticks([400, 150, 600, 50]);
        // 200 ticks passed, 50 of them idle.
        assert_eq!(cpu_percent(times, later), Some(75.0));
        // A counter that wrapped says nothing rather than something wrong.
        let wrapped = cpu_times_from_ticks([0, 0, 600, 0]);
        assert_eq!(cpu_percent(times, wrapped), None);
    }

    #[test]
    fn mac_memory_in_use_leaves_the_file_cache_available() {
        const PAGE: u64 = 16 * 1024;
        let total = 1000 * PAGE;
        let pages = MacPages {
            internal: 400,
            purgeable: 50,
            wired: 100,
            compressed: 50,
        };
        assert_eq!(mac_available_memory(total, pages, PAGE), 500 * PAGE);
        assert_eq!(memory_percent(total, 500 * PAGE), Some(50.0));
        // Counts beyond the total never underflow.
        let huge = MacPages {
            internal: u64::MAX,
            purgeable: 0,
            wired: 1,
            compressed: 1,
        };
        assert_eq!(mac_available_memory(total, huge, PAGE), 0);
        let none = MacPages {
            internal: 10,
            purgeable: 20,
            wired: 0,
            compressed: 0,
        };
        assert_eq!(mac_available_memory(total, none, PAGE), total);
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

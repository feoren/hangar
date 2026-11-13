//! GPU load and GPU memory, of the whole machine for the system bar (`system_usage.rs`) and per
//! session process tree for the session rows (`resources.rs`) (contract:
//! docs/wiki/pages/syntheses/session-resources-and-phone-push.md).
//!
//! One reading serves both: [`GpuMonitor::snapshot`] collects every counter once and caches the
//! result for [`CACHE_FOR`], so the system bar and the session rows, which poll on their own
//! clocks, never collect twice within a second.
//!
//! - Windows: one PDH query that stays open, with `\GPU Engine(*)\Utilization Percentage`,
//!   `\GPU Process Memory(*)\Dedicated Usage` and `\GPU Adapter Memory(*)\Dedicated Usage`.
//! - Other systems: nothing (`None`), the window hides the GPU.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::lock;

/// A second snapshot within this time repeats the first one without collecting again.
pub const CACHE_FOR: Duration = Duration::from_secs(1);

/// One collection of the GPU counters, parsed for both readers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GpuSnapshot {
    /// Per process id: its engines (`luid_..._phys_n_eng_m`) and their utilization in percent.
    engines: HashMap<u32, Vec<(String, f64)>>,
    /// Per process id: dedicated GPU memory in bytes.
    process_memory: HashMap<u32, u64>,
    /// Busiest engine of the machine in percent; `None` without any engine.
    pub load: Option<f64>,
    /// Dedicated GPU memory in use on all adapters, in bytes; `None` without the counter.
    pub memory: Option<u64>,
}

/// What one process tree uses of the GPU.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TreeGpu {
    /// Busiest engine counting only the tree's processes, in percent with one decimal; `None`
    /// when none of them has an engine instance.
    pub load: Option<f64>,
    /// Dedicated GPU memory of the tree's processes in bytes; `None` when none of them has any.
    pub memory: Option<u64>,
}

impl GpuSnapshot {
    /// Parses the instances of the three counters. Engine instances are named like
    /// `pid_1234_luid_0x00000000_0x0000D1B5_phys_0_eng_3_engtype_3D`, process memory instances
    /// like `pid_1234_luid_0x00000000_0x0000D1B5_phys_0`; adapter memory instances carry no
    /// process and are summed as they are. A broken value counts as zero.
    pub fn from_counters<'a>(
        engines: impl IntoIterator<Item = (&'a str, f64)>,
        process_memory: impl IntoIterator<Item = (&'a str, f64)>,
        adapter_memory: Option<impl IntoIterator<Item = (&'a str, f64)>>,
    ) -> Self {
        let mut snapshot = Self::default();
        let mut machine: HashMap<String, f64> = HashMap::new();
        for (name, value) in engines {
            let Some(engine) = engine_key(name) else {
                continue;
            };
            let value = sane(value);
            *machine.entry(engine.to_owned()).or_default() += value;
            if let Some(pid) = instance_pid(name) {
                snapshot
                    .engines
                    .entry(pid)
                    .or_default()
                    .push((engine.to_owned(), value));
            }
        }
        snapshot.load = busiest(machine.into_values());
        for (name, value) in process_memory {
            if let Some(pid) = instance_pid(name) {
                let entry = snapshot.process_memory.entry(pid).or_default();
                *entry = entry.saturating_add(bytes(value));
            }
        }
        snapshot.memory = adapter_memory.map(|instances| {
            instances
                .into_iter()
                .fold(0u64, |sum, (_, value)| sum.saturating_add(bytes(value)))
        });
        snapshot
    }

    /// The tree of `processes`: their shares summed per engine (two processes on the same 3D
    /// engine share its load), the busiest engine as the load, like the machine's load; their
    /// dedicated memory summed.
    pub fn tree(&self, processes: &[u32]) -> TreeGpu {
        let mut engines: HashMap<&str, f64> = HashMap::new();
        let mut memory: Option<u64> = None;
        for pid in processes {
            for (engine, value) in self.engines.get(pid).into_iter().flatten() {
                *engines.entry(engine.as_str()).or_default() += value;
            }
            if let Some(&bytes) = self.process_memory.get(pid) {
                memory = Some(memory.unwrap_or(0).saturating_add(bytes));
            }
        }
        TreeGpu {
            load: busiest(engines.into_values()),
            memory,
        }
    }
}

/// The busiest engine as a percentage with one decimal, capped at 100; `None` without engines.
fn busiest(engines: impl IntoIterator<Item = f64>) -> Option<f64> {
    engines.into_iter().reduce(f64::max).map(percent)
}

fn sane(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn bytes(value: f64) -> u64 {
    // Saturating float to integer conversion: NaN becomes 0, huge values u64::MAX.
    sane(value).round() as u64
}

fn percent(value: f64) -> f64 {
    (value.clamp(0.0, 100.0) * 10.0).round() / 10.0
}

/// The engine part of an instance name: from `luid_` up to the engine type, without the process.
fn engine_key(name: &str) -> Option<&str> {
    let start = name.find("luid_")?;
    let rest = &name[start..];
    let end = rest.find("_engtype_").unwrap_or(rest.len());
    Some(&rest[..end])
}

/// The process id of an instance name starting with `pid_<n>_`.
fn instance_pid(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("pid_")?;
    let end = rest.find('_').unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[derive(Default)]
struct State {
    sampler: platform::Sampler,
    last: Option<(Instant, Option<Arc<GpuSnapshot>>)>,
}

/// Managed state: the open query and the last snapshot, shared by every reader.
#[derive(Default)]
pub struct GpuMonitor {
    state: Mutex<State>,
}

impl GpuMonitor {
    /// The snapshot for `now`: the cached one when it is younger than [`CACHE_FOR`], otherwise a
    /// fresh collection. `None` where the platform has no counters, and on the very first
    /// collection, which only sets the utilization's baseline. The lock is held while
    /// collecting, so a second caller waits and gets the cached snapshot.
    pub fn snapshot(&self, now: Instant) -> Option<Arc<GpuSnapshot>> {
        let mut state = lock(&self.state);
        if let Some((at, snapshot)) = &state.last {
            if now.saturating_duration_since(*at) < CACHE_FOR {
                return snapshot.clone();
            }
        }
        let snapshot = state.sampler.read().map(Arc::new);
        state.last = Some((now, snapshot.clone()));
        snapshot
    }
}

#[cfg(windows)]
mod platform {
    use std::ptr;

    use windows_sys::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W,
        PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    };

    use super::GpuSnapshot;

    /// Counter paths in English, so they resolve on every display language.
    const ENGINE_COUNTER: &str = "\\GPU Engine(*)\\Utilization Percentage";
    const PROCESS_MEMORY_COUNTER: &str = "\\GPU Process Memory(*)\\Dedicated Usage";
    const ADAPTER_MEMORY_COUNTER: &str = "\\GPU Adapter Memory(*)\\Dedicated Usage";
    /// Bytes of counter values read at most per counter; a machine with that many instances is
    /// summed partially rather than allocating without bound.
    const MAX_BUFFER: u32 = 4 * 1024 * 1024;

    /// The query: opened on the first reading, kept open, given up for good when the engine
    /// counter does not exist (no GPU driver with WDDM 2.0, say).
    #[derive(Default)]
    pub enum Sampler {
        #[default]
        Untried,
        Open(GpuQuery),
        Unavailable,
    }

    impl Sampler {
        pub fn read(&mut self) -> Option<GpuSnapshot> {
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
        engines: PDH_HCOUNTER,
        /// The memory counters are newer than the engine counter; without them only the load
        /// is known.
        process_memory: Option<PDH_HCOUNTER>,
        adapter_memory: Option<PDH_HCOUNTER>,
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
            let add = |path: &str| -> Option<PDH_HCOUNTER> {
                let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
                let mut counter: PDH_HCOUNTER = ptr::null_mut();
                // SAFETY: `query` is open, `path` is NUL-terminated and outlives the call.
                let status =
                    unsafe { PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) };
                (status == 0).then_some(counter)
            };
            let engines = add(ENGINE_COUNTER);
            let process_memory = add(PROCESS_MEMORY_COUNTER);
            let adapter_memory = add(ADAPTER_MEMORY_COUNTER);
            let Some(engines) = engines else {
                // SAFETY: The query was opened above and is closed exactly once, here.
                unsafe { PdhCloseQuery(query) };
                return None;
            };
            let opened = Self {
                query,
                engines,
                process_memory,
                adapter_memory,
            };
            // The utilization is a rate: the first collection only sets the baseline.
            // SAFETY: `query` is open.
            unsafe { PdhCollectQueryData(opened.query) };
            Some(opened)
        }

        /// Collects once more and parses all counters; `None` when the collection fails.
        fn read(&mut self) -> Option<GpuSnapshot> {
            // SAFETY: `query` is open for as long as `self` lives.
            if unsafe { PdhCollectQueryData(self.query) } != 0 {
                return None;
            }
            let engines = values(self.engines).unwrap_or_default();
            let process_memory = self.process_memory.and_then(values).unwrap_or_default();
            let adapter_memory = self.adapter_memory.and_then(values);
            Some(GpuSnapshot::from_counters(
                engines.iter().map(|(name, value)| (name.as_str(), *value)),
                process_memory
                    .iter()
                    .map(|(name, value)| (name.as_str(), *value)),
                adapter_memory
                    .as_ref()
                    .map(|items| items.iter().map(|(name, value)| (name.as_str(), *value))),
            ))
        }
    }

    /// The instances of one counter with valid data, as name and value.
    fn values(counter: PDH_HCOUNTER) -> Option<Vec<(String, f64)>> {
        let mut size: u32 = 0;
        let mut count: u32 = 0;
        // SAFETY: A zero size with a null buffer asks for the size needed.
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                counter,
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
            PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, items)
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
        Some(
            items
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
                .collect(),
        )
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
            // also frees its counters.
            unsafe { PdhCloseQuery(self.query) };
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::GpuSnapshot;

    #[derive(Default)]
    pub struct Sampler;

    impl Sampler {
        pub fn read(&mut self) -> Option<GpuSnapshot> {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LUID: &str = "luid_0x00000000_0x0000D1B5_phys_0";
    const MB: u64 = 1024 * 1024;

    fn engine(pid: u32, eng: u32, kind: &str) -> String {
        format!("pid_{pid}_{LUID}_eng_{eng}_engtype_{kind}")
    }

    fn memory(pid: u32) -> String {
        format!("pid_{pid}_{LUID}")
    }

    fn snapshot(engines: &[(String, f64)], process_memory: &[(String, f64)]) -> GpuSnapshot {
        GpuSnapshot::from_counters(
            engines.iter().map(|(name, value)| (name.as_str(), *value)),
            process_memory
                .iter()
                .map(|(name, value)| (name.as_str(), *value)),
            Some([("luid_0x00000000_0x0000D1B5_phys_0", 3.0 * MB as f64)]),
        )
    }

    #[test]
    fn machine_load_sums_processes_per_engine_and_takes_the_busiest() {
        let snapshot = snapshot(
            &[
                (engine(10, 0, "3D"), 12.0),
                (engine(20, 0, "3D"), 30.5),
                (engine(10, 3, "VideoDecode"), 55.0),
                (engine(30, 1, "Copy"), 2.0),
            ],
            &[],
        );
        // 3D: 12 + 30.5 = 42.5; video decode 55 on its own engine is the busiest.
        assert_eq!(snapshot.load, Some(55.0));
        assert_eq!(snapshot.memory, Some(3 * MB));
    }

    #[test]
    fn machine_load_keeps_adapters_apart_and_caps_at_full() {
        let first = format!("pid_1_{LUID}_eng_0_engtype_3D");
        let second = "pid_1_luid_0x00000000_0x0000AAAA_phys_0_eng_0_engtype_3D".to_owned();
        let apart = snapshot(&[(first.clone(), 70.0), (second, 60.0)], &[]);
        assert_eq!(apart.load, Some(70.0), "not 130 over two adapters");
        let full = snapshot(&[(first.clone(), 80.0), (engine(2, 0, "3D"), 40.0)], &[]);
        assert_eq!(full.load, Some(100.0));
    }

    #[test]
    fn without_engines_the_load_is_unknown() {
        assert_eq!(snapshot(&[], &[]).load, None);
        assert_eq!(
            snapshot(&[("_Total".to_owned(), 5.0)], &[]).load,
            None,
            "no engine in the name"
        );
        let idle = snapshot(&[(engine(1, 0, "3D"), f64::NAN)], &[]);
        assert_eq!(idle.load, Some(0.0));
    }

    #[test]
    fn a_tree_sums_its_processes_per_engine() {
        let snapshot = snapshot(
            &[
                (engine(10, 0, "3D"), 12.0),
                (engine(11, 0, "3D"), 20.25),
                (engine(11, 3, "VideoDecode"), 25.0),
                (engine(99, 0, "3D"), 50.0),
            ],
            &[
                (memory(10), 100.0 * MB as f64),
                (memory(11), 20.0 * MB as f64),
            ],
        );
        let tree = snapshot.tree(&[10, 11, 12]);
        // 3D of the tree: 12 + 20.25, rounded to one decimal; the other process's 50 % is not its.
        assert_eq!(tree.load, Some(32.3));
        assert_eq!(tree.memory, Some(120 * MB));
    }

    #[test]
    fn a_tree_without_gpu_instances_has_nothing() {
        let snapshot = snapshot(&[(engine(10, 0, "3D"), 12.0)], &[(memory(10), MB as f64)]);
        assert_eq!(snapshot.tree(&[42, 43]), TreeGpu::default());
        assert_eq!(snapshot.tree(&[]), TreeGpu::default());
    }

    #[test]
    fn a_tree_with_memory_only_has_no_load() {
        let snapshot = snapshot(&[], &[(memory(7), 64.0 * MB as f64)]);
        assert_eq!(
            snapshot.tree(&[7]),
            TreeGpu {
                load: None,
                memory: Some(64 * MB),
            }
        );
    }

    #[test]
    fn instance_names_give_the_process() {
        assert_eq!(instance_pid(&engine(1234, 0, "3D")), Some(1234));
        assert_eq!(instance_pid(&memory(5)), Some(5));
        assert_eq!(instance_pid("luid_0x0_phys_0"), None);
        assert_eq!(instance_pid("pid_x_luid"), None);
        assert_eq!(instance_pid("pid_"), None);
    }

    #[test]
    fn memory_values_are_whole_bytes_never_negative() {
        let snapshot = GpuSnapshot::from_counters(
            std::iter::empty(),
            [
                (memory(1).as_str(), -5.0),
                (memory(2).as_str(), f64::INFINITY),
            ],
            None::<std::iter::Empty<(&str, f64)>>,
        );
        assert_eq!(snapshot.tree(&[1]).memory, Some(0));
        assert_eq!(snapshot.tree(&[2]).memory, Some(0));
        assert_eq!(snapshot.memory, None, "no adapter counter");
    }

    #[test]
    fn snapshots_are_cached_for_a_second() {
        let monitor = GpuMonitor::default();
        let start = Instant::now();
        let first = monitor.snapshot(start);
        assert!(
            first.is_none(),
            "the first collection only sets the baseline"
        );
        monitor.snapshot(start + Duration::from_millis(900));
        assert!(lock(&monitor.state)
            .last
            .as_ref()
            .is_some_and(|(at, _)| *at == start));
        monitor.snapshot(start + CACHE_FOR);
        assert!(lock(&monitor.state)
            .last
            .as_ref()
            .is_some_and(|(at, _)| *at == start + CACHE_FOR));
    }

    /// The real counters of this machine; prints the values and the time a collection takes.
    #[test]
    fn reads_this_machine() {
        let monitor = GpuMonitor::default();
        let start = Instant::now();
        monitor.snapshot(start);
        std::thread::sleep(Duration::from_millis(1_100));
        let begun = Instant::now();
        let second = monitor.snapshot(start + Duration::from_millis(1_100));
        let took = begun.elapsed();
        println!(
            "load {:?}, memory {:?}, processes with engines {}, collection took {took:?}",
            second.as_ref().and_then(|s| s.load),
            second.as_ref().and_then(|s| s.memory),
            second.as_ref().map_or(0, |s| s.engines.len()),
        );
        if let Some(load) = second.as_ref().and_then(|s| s.load) {
            assert!((0.0..=100.0).contains(&load));
        }
    }
}

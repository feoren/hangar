//! GPU use on Linux as plain text parsing, so it is tested on every platform (evidence:
//! docs/wiki/pages/platforms/gpu-readings.md).
//!
//! - Per process: the DRM client statistics in `/proc/<pid>/fdinfo/<fd>` of every open DRM file
//!   (kernel documentation "DRM client usage stats"): busy time per engine and resident memory per
//!   region. The busy time is a counter, so a load needs two readings ([`History`]).
//! - Per machine: amdgpu's `gpu_busy_percent` and `mem_info_vram_used` in the card's device folder
//!   and NVIDIA's NVML (`nvml.rs`), combined by [`combine`].

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Busy time of one engine (or group of identical engines) since the client opened the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    /// `drm-engine-<name>: <n> ns`.
    Nanos(u64),
    /// `drm-cycles-<name>` with `drm-total-cycles-<name>` (xe): busy and elapsed GPU cycles.
    Cycles { busy: u64, total: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    pub name: String,
    pub busy: Busy,
    /// `drm-engine-capacity-<name>`: identical engines behind the name; 1 without the key.
    pub capacity: u32,
}

/// One DRM client, the statistics of one open DRM file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Client {
    pub driver: String,
    /// PCI slot of the device; `None` for a device without one.
    pub pdev: Option<String>,
    /// Unique per open file (per device with `drm-pdev`): duplicated and shared file descriptors
    /// carry the same id and must be counted once.
    pub id: Option<u64>,
    pub engines: Vec<Engine>,
    /// Resident memory in the device's own memory (regions `vram*` and `local*`), in bytes;
    /// `None` without such a region. System memory and GTT are not counted, like the dedicated
    /// memory on Windows.
    pub memory: Option<u64>,
}

impl Client {
    /// The device the engines belong to: two clients' shares of the same engine add up.
    fn device(&self) -> &str {
        self.pdev.as_deref().unwrap_or(&self.driver)
    }
}

/// A size in bytes with the optional unit the specification allows (`KiB`, `MiB`); `GiB` is
/// read too.
pub fn parse_size(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let number: u64 = parts.next()?.parse().ok()?;
    let factor = match parts.next() {
        None => 1,
        Some("KiB") => 1 << 10,
        Some("MiB") => 1 << 20,
        Some("GiB") => 1 << 30,
        Some(_) => return None,
    };
    Some(number.saturating_mul(factor))
}

fn device_local(region: &str) -> bool {
    region.starts_with("vram") || region.starts_with("local")
}

/// The statistics of one fdinfo file; `None` when it is no DRM client (no `drm-driver`).
pub fn parse_fdinfo(text: &str) -> Option<Client> {
    let pairs: HashMap<&str, &str> = text
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim(), value.trim()))
        })
        .collect();
    let driver = (*pairs.get("drm-driver")?).to_owned();
    if driver.is_empty() {
        return None;
    }
    let capacity = |name: &str| {
        pairs
            .get(format!("drm-engine-capacity-{name}").as_str())
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|capacity| *capacity > 0)
            .unwrap_or(1)
    };
    let mut engines = Vec::new();
    for (key, value) in &pairs {
        if let Some(name) = key.strip_prefix("drm-engine-") {
            if name.starts_with("capacity-") {
                continue;
            }
            let Some(nanos) = value
                .strip_suffix("ns")
                .and_then(|n| n.trim().parse::<u64>().ok())
            else {
                continue;
            };
            engines.push(Engine {
                name: name.to_owned(),
                busy: Busy::Nanos(nanos),
                capacity: capacity(name),
            });
        } else if let Some(name) = key.strip_prefix("drm-cycles-") {
            let total = pairs
                .get(format!("drm-total-cycles-{name}").as_str())
                .and_then(|value| value.parse::<u64>().ok());
            let (Ok(busy), Some(total)) = (value.parse::<u64>(), total) else {
                continue;
            };
            engines.push(Engine {
                name: name.to_owned(),
                busy: Busy::Cycles { busy, total },
                capacity: capacity(name),
            });
        }
    }
    engines.sort_by(|a, b| a.name.cmp(&b.name));
    // `drm-memory-<region>` is amdgpu's older alias of `drm-resident-<region>`; a region with
    // both counts once.
    let mut regions: HashMap<&str, u64> = HashMap::new();
    for (key, value) in &pairs {
        if let Some(region) = key.strip_prefix("drm-memory-") {
            if device_local(region) {
                if let Some(bytes) = parse_size(value) {
                    regions.entry(region).or_insert(bytes);
                }
            }
        }
    }
    for (key, value) in &pairs {
        if let Some(region) = key.strip_prefix("drm-resident-") {
            if device_local(region) {
                if let Some(bytes) = parse_size(value) {
                    regions.insert(region, bytes);
                }
            }
        }
    }
    let memory = (!regions.is_empty()).then(|| {
        regions
            .values()
            .fold(0u64, |sum, bytes| sum.saturating_add(*bytes))
    });
    Some(Client {
        driver,
        pdev: pairs.get("drm-pdev").map(|value| (*value).to_owned()),
        id: pairs
            .get("drm-client-id")
            .and_then(|value| value.parse().ok()),
        engines,
        memory,
    })
}

/// How busy an engine was between two readings, in percent of its capacity; `None` when the
/// counter went backwards (a new client with a reused id) or no time passed.
pub fn busy_percent(before: Busy, after: Busy, elapsed: Duration, capacity: u32) -> Option<f64> {
    let capacity = f64::from(capacity.max(1));
    match (before, after) {
        (Busy::Nanos(before), Busy::Nanos(after)) => {
            let busy = after.checked_sub(before)?;
            let elapsed = elapsed.as_nanos() as f64;
            (elapsed > 0.0).then(|| busy as f64 / elapsed / capacity * 100.0)
        }
        (
            Busy::Cycles {
                busy: busy_before,
                total: total_before,
            },
            Busy::Cycles { busy, total },
        ) => {
            let busy = busy.checked_sub(busy_before)?;
            let total = total.checked_sub(total_before)?;
            (total > 0).then(|| busy as f64 / total as f64 / capacity * 100.0)
        }
        _ => None,
    }
}

/// One DRM file of one process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub pid: u32,
    pub fd: u32,
    pub client: Client,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Identity {
    Client(u64),
    /// A driver without `drm-client-id`: the file itself.
    File(u32, u32),
}

type ClientKey = (String, Identity);

/// Per process: its engines (`<device>/<engine>`) with their load in percent, and its device
/// memory in bytes, as [`super::GpuSnapshot`] keeps them.
pub type PerProcess = (HashMap<u32, Vec<(String, f64)>>, HashMap<u32, u64>);

/// The previous busy counters of every client, to turn the next reading into a load.
#[derive(Debug, Default)]
pub struct History {
    previous: HashMap<ClientKey, Vec<Engine>>,
    at: Option<Instant>,
}

impl History {
    /// The use of each process from the readings taken `now`. A client counts once, for the
    /// first process that holds it; its load needs a reading before, its memory does not.
    pub fn update(&mut self, readings: Vec<Reading>, now: Instant) -> PerProcess {
        let elapsed = self.at.map(|at| now.saturating_duration_since(at));
        let mut engines: HashMap<u32, Vec<(String, f64)>> = HashMap::new();
        let mut memory: HashMap<u32, u64> = HashMap::new();
        let mut next: HashMap<ClientKey, Vec<Engine>> = HashMap::new();
        let mut seen: HashSet<ClientKey> = HashSet::new();
        for Reading { pid, fd, client } in readings {
            let identity = client.id.map_or(Identity::File(pid, fd), Identity::Client);
            let key = (client.device().to_owned(), identity);
            if !seen.insert(key.clone()) {
                continue;
            }
            if let Some(bytes) = client.memory {
                let entry = memory.entry(pid).or_default();
                *entry = entry.saturating_add(bytes);
            }
            if let (Some(before), Some(elapsed)) = (self.previous.get(&key), elapsed) {
                for engine in &client.engines {
                    let Some(old) = before.iter().find(|old| old.name == engine.name) else {
                        continue;
                    };
                    if let Some(load) =
                        busy_percent(old.busy, engine.busy, elapsed, engine.capacity)
                    {
                        engines
                            .entry(pid)
                            .or_default()
                            .push((format!("{}/{}", client.device(), engine.name), load));
                    }
                }
            }
            next.insert(key, client.engines);
        }
        self.previous = next;
        self.at = Some(now);
        (engines, memory)
    }
}

/// One process of `/proc`: its id, its parent and its terminal session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessEntry {
    pub pid: u32,
    pub parent: i64,
    pub session: i64,
}

/// The processes of every session Hangar started: each session's child is a direct child of
/// Hangar and leads a terminal session of its own (portable-pty), and the session's processes
/// carry its id, as `proctree.rs` counts them.
pub fn session_members(processes: &[ProcessEntry], own: u32) -> Vec<u32> {
    let leaders: HashSet<i64> = processes
        .iter()
        .filter(|process| {
            process.parent == i64::from(own) && process.session == i64::from(process.pid)
        })
        .map(|process| process.session)
        .collect();
    processes
        .iter()
        .filter(|process| leaders.contains(&process.session))
        .map(|process| process.pid)
        .collect()
}

/// A number in a sysfs file (`gpu_busy_percent`, `mem_info_vram_used`).
pub fn parse_sysfs_number(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}

/// The machine from every device's reading: the busiest device as the load (with one decimal,
/// capped at 100), the memory of all devices summed; `None` where no device gives it.
pub fn combine(
    devices: impl IntoIterator<Item = (Option<f64>, Option<u64>)>,
) -> (Option<f64>, Option<u64>) {
    let mut load: Option<f64> = None;
    let mut memory: Option<u64> = None;
    for (device_load, device_memory) in devices {
        if let Some(value) = device_load.filter(|value| value.is_finite()) {
            load = Some(load.map_or(value, |load: f64| load.max(value)));
        }
        if let Some(bytes) = device_memory {
            memory = Some(memory.unwrap_or(0).saturating_add(bytes));
        }
    }
    (load.map(super::percent), memory)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1 << 20;

    /// The shape amdgpu prints (kernel 6.x): both the older and the newer memory keys.
    const AMDGPU: &str = "pos:\t0\nflags:\t02100002\nmnt_id:\t26\nino:\t1081\n\
        drm-driver:\tamdgpu\ndrm-client-id:\t42\ndrm-pdev:\t0000:03:00.0\n\
        pasid:\t32771\ndrm-memory-vram:\t65536 KiB\ndrm-memory-gtt:\t2048 KiB\n\
        drm-memory-cpu:\t0 KiB\ndrm-total-vram:\t70000 KiB\ndrm-resident-vram:\t65536 KiB\n\
        drm-engine-gfx:\t1500000000 ns\ndrm-engine-compute:\t0 ns\ndrm-engine-dec:\t20 ns\n";

    /// The shape i915 prints: no device memory on an integrated GPU, a video engine pair.
    const I915: &str = "drm-driver:\ti915\ndrm-pdev:\t0000:00:02.0\ndrm-client-id:\t7\n\
        drm-total-system0:\t12 MiB\ndrm-resident-system0:\t12 MiB\n\
        drm-engine-render:\t250000000 ns\ndrm-engine-video:\t100 ns\n\
        drm-engine-capacity-video:\t2\n";

    /// The shape xe prints: cycles instead of nanoseconds.
    const XE: &str = "drm-driver:\txe\ndrm-pdev:\t0000:00:02.0\ndrm-client-id:\t9\n\
        drm-resident-vram0:\t1 MiB\ndrm-cycles-rcs:\t1000\ndrm-total-cycles-rcs:\t5000\n\
        drm-cycles-vcs:\t0\ndrm-total-cycles-vcs:\t5000\ndrm-engine-capacity-vcs:\t2\n";

    #[test]
    fn sizes_carry_their_unit() {
        assert_eq!(parse_size("17"), Some(17));
        assert_eq!(parse_size("3 KiB"), Some(3 * 1024));
        assert_eq!(parse_size("2 MiB"), Some(2 * MIB));
        assert_eq!(parse_size("1 GiB"), Some(1024 * MIB));
        assert_eq!(parse_size("1 kB"), None);
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("x KiB"), None);
    }

    #[test]
    fn amdgpu_fdinfo_gives_engines_and_device_memory() {
        let client = parse_fdinfo(AMDGPU).expect("a DRM client");
        assert_eq!(client.driver, "amdgpu");
        assert_eq!(client.pdev.as_deref(), Some("0000:03:00.0"));
        assert_eq!(client.id, Some(42));
        // vram only, counted once although both keys name it; GTT and CPU are system memory.
        assert_eq!(client.memory, Some(64 * MIB));
        let names: Vec<&str> = client.engines.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["compute", "dec", "gfx"]);
        assert_eq!(client.engines[2].busy, Busy::Nanos(1_500_000_000));
        assert_eq!(client.engines[2].capacity, 1);
    }

    #[test]
    fn i915_fdinfo_has_no_device_memory_on_an_integrated_gpu() {
        let client = parse_fdinfo(I915).expect("a DRM client");
        assert_eq!(client.memory, None);
        let video = client.engines.iter().find(|e| e.name == "video").unwrap();
        assert_eq!(video.capacity, 2);
        assert!(client
            .engines
            .iter()
            .all(|e| !e.name.starts_with("capacity")));
    }

    #[test]
    fn xe_fdinfo_counts_cycles() {
        let client = parse_fdinfo(XE).expect("a DRM client");
        assert_eq!(client.memory, Some(MIB));
        assert_eq!(
            client.engines[0],
            Engine {
                name: "rcs".to_owned(),
                busy: Busy::Cycles {
                    busy: 1000,
                    total: 5000
                },
                capacity: 1,
            }
        );
        assert_eq!(client.engines[1].capacity, 2);
    }

    #[test]
    fn a_file_without_driver_is_no_client() {
        assert_eq!(parse_fdinfo("pos:\t0\nflags:\t02\n"), None);
        assert_eq!(parse_fdinfo("drm-driver:\t\n"), None);
        assert_eq!(parse_fdinfo(""), None);
    }

    #[test]
    fn broken_values_are_skipped() {
        let client = parse_fdinfo(
            "drm-driver:\tx\ndrm-engine-gfx:\tlots ns\ndrm-engine-dma:\t5\n\
             drm-cycles-rcs:\t10\ndrm-resident-vram:\tmuch\ndrm-engine-capacity-gfx:\t0\n",
        )
        .unwrap();
        assert!(client.engines.is_empty());
        assert_eq!(client.memory, None);
        assert_eq!(client.id, None);
    }

    #[test]
    fn busy_time_turns_into_a_share_of_the_time_passed() {
        let second = Duration::from_secs(1);
        assert_eq!(
            busy_percent(Busy::Nanos(0), Busy::Nanos(250_000_000), second, 1),
            Some(25.0)
        );
        // Two identical engines: a full second of busy time is half of them.
        assert_eq!(
            busy_percent(Busy::Nanos(0), Busy::Nanos(1_000_000_000), second, 2),
            Some(50.0)
        );
        let cycles = |busy, total| Busy::Cycles { busy, total };
        assert_eq!(
            busy_percent(cycles(1000, 5000), cycles(3000, 9000), second, 1),
            Some(50.0)
        );
        assert_eq!(
            busy_percent(Busy::Nanos(10), Busy::Nanos(5), second, 1),
            None
        );
        assert_eq!(
            busy_percent(Busy::Nanos(0), Busy::Nanos(5), Duration::ZERO, 1),
            None
        );
        assert_eq!(busy_percent(cycles(0, 5), cycles(0, 5), second, 1), None);
        assert_eq!(busy_percent(Busy::Nanos(0), cycles(1, 2), second, 1), None);
    }

    fn reading(pid: u32, fd: u32, text: &str) -> Reading {
        Reading {
            pid,
            fd,
            client: parse_fdinfo(text).unwrap(),
        }
    }

    #[test]
    fn the_first_reading_gives_memory_and_the_second_the_load() {
        let mut history = History::default();
        let start = Instant::now();
        let (engines, memory) = history.update(vec![reading(10, 5, AMDGPU)], start);
        assert!(engines.is_empty());
        assert_eq!(memory.get(&10), Some(&(64 * MIB)));
        let later = AMDGPU.replace("1500000000 ns", "2000000000 ns");
        let (engines, _) =
            history.update(vec![reading(10, 5, &later)], start + Duration::from_secs(2));
        let gfx = engines[&10]
            .iter()
            .find(|(name, _)| name == "0000:03:00.0/gfx")
            .expect("gfx engine");
        assert_eq!(gfx.1, 25.0);
    }

    #[test]
    fn a_shared_client_counts_once() {
        let mut history = History::default();
        let start = Instant::now();
        // The same client id through a duplicated descriptor and an inherited one.
        let (_, memory) = history.update(
            vec![
                reading(10, 5, AMDGPU),
                reading(10, 6, AMDGPU),
                reading(11, 3, AMDGPU),
            ],
            start,
        );
        assert_eq!(memory.get(&10), Some(&(64 * MIB)));
        assert_eq!(memory.get(&11), None);
        // Without a client id every file counts on its own.
        let no_id = AMDGPU.replace("drm-client-id:\t42\n", "");
        let (_, memory) = history.update(
            vec![reading(10, 5, &no_id), reading(10, 6, &no_id)],
            start + Duration::from_secs(1),
        );
        assert_eq!(memory.get(&10), Some(&(128 * MIB)));
    }

    #[test]
    fn a_client_that_went_away_is_forgotten() {
        let mut history = History::default();
        let start = Instant::now();
        history.update(vec![reading(10, 5, AMDGPU)], start);
        history.update(Vec::new(), start + Duration::from_secs(1));
        let (engines, _) =
            history.update(vec![reading(10, 5, AMDGPU)], start + Duration::from_secs(2));
        assert!(engines.is_empty(), "no reading before this one any more");
    }

    #[test]
    fn only_processes_of_hangars_sessions_are_read() {
        let entry = |pid, parent, session| ProcessEntry {
            pid,
            parent,
            session,
        };
        let processes = [
            entry(100, 1, 100),   // Hangar itself
            entry(200, 100, 200), // a session's child, leader of its session
            entry(201, 200, 200), // its agent
            entry(202, 1, 200),   // a daemon of it, reparented but still in the session
            entry(300, 100, 100), // the webview, a child in Hangar's own session
            entry(400, 1, 400),   // another program
            entry(401, 400, 400),
        ];
        let mut members = session_members(&processes, 100);
        members.sort_unstable();
        assert_eq!(members, [200, 201, 202]);
        assert!(session_members(&processes, 999).is_empty());
    }

    #[test]
    fn devices_combine_into_the_machine() {
        assert_eq!(combine([]), (None, None));
        assert_eq!(
            combine([
                (Some(30.0), Some(MIB)),
                (Some(72.25), None),
                (None, Some(MIB))
            ]),
            (Some(72.3), Some(2 * MIB))
        );
        assert_eq!(combine([(Some(140.0), None)]), (Some(100.0), None));
        assert_eq!(combine([(Some(f64::NAN), None)]), (None, None));
        assert_eq!(parse_sysfs_number("37\n"), Some(37));
        assert_eq!(parse_sysfs_number("busy"), None);
    }
}

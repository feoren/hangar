//! Whether the computer runs on battery, for the power saving mode (contract and evidence:
//! docs/wiki/pages/platforms/power-detection.md).
//!
//! One thread reads the power state every [`POLL_INTERVAL`] and emits [`EVENT`] only when it
//! changed; `power_state` reads it afresh, so the window also gets the current state when it
//! starts or comes back into focus. Polling instead of `RegisterPowerSettingNotification`: the
//! notification needs a window procedure of its own (tao does not forward `WM_POWERBROADCAST`),
//! while `GetSystemPowerStatus` is one cheap call, and a delay of up to 30 s before saving starts
//! or ends costs nothing worth the extra window.
//!
//! - Windows: `GetSystemPowerStatus` (AC line, "no system battery" flag, battery saver).
//! - Linux: `/sys/class/power_supply` (kernel ABI `sysfs-class-power`); no battery saver.
//! - macOS: `pmset -g batt` (power source, internal battery) and `NSProcessInfo`'s Low Power
//!   Mode as the battery saver.
//! - Other systems: [`PowerState::Unknown`], and the window hides the setting.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::Serialize;

use crate::lock;

/// Event with the new [`PowerState`], sent only on a change.
pub const EVENT: &str = "power-changed";
/// How often the thread reads the power state.
pub const POLL_INTERVAL: Duration = Duration::from_secs(30);
/// Whether this platform can tell battery from AC at all; elsewhere no thread is started.
pub const SUPPORTED: bool = cfg!(any(windows, target_os = "linux", target_os = "macos"));

/// Where the power comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PowerState {
    /// The platform cannot tell (or is not supported).
    #[default]
    Unknown,
    /// A machine without a system battery, such as a desktop: always on AC.
    NoBattery,
    /// Plugged in.
    Ac,
    /// On battery.
    Battery,
    /// On battery with the system's battery saver on.
    Saver,
}

/// `GetSystemPowerStatus` "no system battery" flag of `BatteryFlag`.
const NO_SYSTEM_BATTERY: u8 = 128;
/// `GetSystemPowerStatus` "unknown status" of `ACLineStatus` and `BatteryFlag`.
const UNKNOWN_STATUS: u8 = 255;

/// The state from the members of `SYSTEM_POWER_STATUS`. A battery saver while plugged in counts
/// as AC: on AC, Hangar changes nothing.
pub fn classify_windows(
    ac_line_status: u8,
    battery_flag: u8,
    system_status_flag: u8,
) -> PowerState {
    if battery_flag != UNKNOWN_STATUS && battery_flag & NO_SYSTEM_BATTERY != 0 {
        return PowerState::NoBattery;
    }
    match ac_line_status {
        1 => PowerState::Ac,
        0 if system_status_flag == 1 => PowerState::Saver,
        0 => PowerState::Battery,
        _ => PowerState::Unknown,
    }
}

/// One entry of `/sys/class/power_supply`, its attribute files trimmed; `None` where missing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Supply {
    pub kind: Option<String>,
    pub online: Option<String>,
    pub status: Option<String>,
    pub scope: Option<String>,
}

impl Supply {
    /// A device's own battery or charger (a mouse, a headset) says nothing about the computer.
    fn belongs_to_system(&self) -> bool {
        self.scope.as_deref() != Some("Device")
    }

    fn is(&self, kind: &str) -> bool {
        self.kind.as_deref() == Some(kind) && self.belongs_to_system()
    }
}

/// The state from the power supplies: no system battery, then any external supply online (Mains,
/// or USB for USB-C chargers), then a discharging battery or only offline supplies.
pub fn classify_supplies(supplies: &[Supply]) -> PowerState {
    if !supplies.iter().any(|supply| supply.is("Battery")) {
        return PowerState::NoBattery;
    }
    let external: Vec<&Supply> = supplies
        .iter()
        .filter(|supply| supply.is("Mains") || supply.is("USB"))
        .collect();
    let online = |supply: &&Supply| matches!(supply.online.as_deref(), Some(value) if value != "0");
    if external.iter().any(online) {
        return PowerState::Ac;
    }
    let discharging = supplies
        .iter()
        .any(|supply| supply.is("Battery") && supply.status.as_deref() == Some("Discharging"));
    if discharging || (!external.is_empty() && external.iter().all(|s| s.online.is_some())) {
        return PowerState::Battery;
    }
    PowerState::Unknown
}

/// The state from the output of `pmset -g batt` and the Low Power Mode. The first line names the
/// source power is drawn from (`Now drawing from 'AC Power'`, `'Battery Power'` or `'UPS Power'`);
/// a Mac with a battery lists it as `-InternalBattery-0`, a desktop lists none. A UPS is no system
/// battery, as on Linux. Low Power Mode on AC counts as AC: on AC, Hangar changes nothing.
pub fn classify_pmset(batt: &str, low_power: bool) -> PowerState {
    if !batt.lines().any(|line| line.contains("-InternalBattery")) {
        return PowerState::NoBattery;
    }
    let source = batt.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("Now drawing from '")?;
        rest.split('\'').next()
    });
    match source {
        Some("AC Power") => PowerState::Ac,
        Some("Battery Power") if low_power => PowerState::Saver,
        Some("Battery Power") => PowerState::Battery,
        _ => PowerState::Unknown,
    }
}

/// Reads every entry of a `power_supply` class folder; an unreadable folder gives none.
pub fn read_supplies(dir: &Path) -> Vec<Supply> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let attribute = |path: &Path, name: &str| {
        fs::read_to_string(path.join(name))
            .ok()
            .map(|text| text.trim().to_owned())
    };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            Supply {
                kind: attribute(&path, "type"),
                online: attribute(&path, "online"),
                status: attribute(&path, "status"),
                scope: attribute(&path, "scope"),
            }
        })
        .collect()
}

/// The power state right now.
#[cfg(windows)]
pub fn read() -> PowerState {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    let mut status = SYSTEM_POWER_STATUS {
        ACLineStatus: UNKNOWN_STATUS,
        BatteryFlag: UNKNOWN_STATUS,
        BatteryLifePercent: UNKNOWN_STATUS,
        SystemStatusFlag: 0,
        BatteryLifeTime: u32::MAX,
        BatteryFullLifeTime: u32::MAX,
    };
    // SAFETY: `status` is a valid, writable SYSTEM_POWER_STATUS that outlives the call; the
    // function only writes into it.
    let ok = unsafe { GetSystemPowerStatus(&mut status) };
    if ok == 0 {
        return PowerState::Unknown;
    }
    classify_windows(
        status.ACLineStatus,
        status.BatteryFlag,
        status.SystemStatusFlag,
    )
}

/// The power state right now.
#[cfg(target_os = "linux")]
pub fn read() -> PowerState {
    classify_supplies(&read_supplies(Path::new("/sys/class/power_supply")))
}

/// The power state right now. `pmset` by its absolute path and as an argument list, so no `PATH`
/// entry can stand in for it, and with a time limit; a `pmset` that hangs reads as unknown.
#[cfg(target_os = "macos")]
pub fn read() -> PowerState {
    use crate::bounded_output::{output_within, TOOL_LIMIT};

    let Some(output) = output_within("/usr/bin/pmset", &["-g", "batt"], TOOL_LIMIT) else {
        return PowerState::Unknown;
    };
    // The power thread and the command's worker thread have no autorelease pool of their own.
    let low_power = objc2::rc::autoreleasepool(|_| {
        objc2_foundation::NSProcessInfo::processInfo().isLowPowerModeEnabled()
    });
    classify_pmset(&String::from_utf8_lossy(&output), low_power)
}

/// The power state right now.
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn read() -> PowerState {
    PowerState::Unknown
}

/// The polling thread and the last state it saw.
#[derive(Default)]
pub struct PowerMonitor {
    latest: Arc<Mutex<PowerState>>,
    stop: Arc<AtomicBool>,
}

impl PowerMonitor {
    /// Reads once, then starts the thread that reads every [`POLL_INTERVAL`] and hands every
    /// change to `emit`. Does nothing where the platform cannot tell.
    pub fn start(
        &self,
        read: impl Fn() -> PowerState + Send + 'static,
        emit: impl Fn(PowerState) + Send + 'static,
    ) -> io::Result<()> {
        if !SUPPORTED {
            return Ok(());
        }
        *lock(&self.latest) = read();
        let latest = Arc::clone(&self.latest);
        let stop = Arc::clone(&self.stop);
        thread::Builder::new()
            .name("power".into())
            .spawn(move || loop {
                thread::sleep(POLL_INTERVAL);
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                if let Some(state) = Self::store(&latest, read()) {
                    emit(state);
                }
            })
            .map(drop)
    }

    /// Takes a fresh reading, as `power_state` does; the thread then emits only later changes.
    pub fn refresh(&self, state: PowerState) -> PowerState {
        Self::store(&self.latest, state);
        state
    }

    /// The last state seen.
    pub fn current(&self) -> PowerState {
        *lock(&self.latest)
    }

    /// Ends the thread after its current sleep.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Stores `state`; `Some` when it differs from the one before.
    fn store(latest: &Mutex<PowerState>, state: PowerState) -> Option<PowerState> {
        let mut current = lock(latest);
        if *current == state {
            return None;
        }
        *current = state;
        Some(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn windows_status_maps_to_the_state() {
        // Plugged in, battery high and charging.
        assert_eq!(classify_windows(1, 1 | 8, 0), PowerState::Ac);
        assert_eq!(classify_windows(0, 1, 0), PowerState::Battery);
        assert_eq!(classify_windows(0, 2, 1), PowerState::Saver);
        // Battery between low and high, not charging: the flag is zero.
        assert_eq!(classify_windows(0, 0, 0), PowerState::Battery);
        assert_eq!(classify_windows(255, 1, 0), PowerState::Unknown);
        // Every documented battery flag on battery: high, low, critical, low and critical.
        for flag in [1, 2, 4, 2 | 4] {
            assert_eq!(
                classify_windows(0, flag, 0),
                PowerState::Battery,
                "flag {flag}"
            );
            assert_eq!(
                classify_windows(0, flag, 1),
                PowerState::Saver,
                "flag {flag}"
            );
        }
        // Charging on AC, whatever the charge.
        for flag in [8, 1 | 8, 2 | 8, 4 | 8] {
            assert_eq!(classify_windows(1, flag, 0), PowerState::Ac, "flag {flag}");
        }
        // The line is the authority: an unreadable battery flag on battery is still battery.
        assert_eq!(classify_windows(0, 255, 0), PowerState::Battery);
        assert_eq!(classify_windows(0, 255, 1), PowerState::Saver);
        assert_eq!(classify_windows(255, 1, 1), PowerState::Unknown);
    }

    #[test]
    fn battery_saver_on_ac_changes_nothing() {
        assert_eq!(classify_windows(1, 1, 1), PowerState::Ac);
    }

    #[test]
    fn a_machine_without_battery_says_so() {
        assert_eq!(classify_windows(1, 128, 0), PowerState::NoBattery);
        // An unknown battery flag (255 has the 128 bit set) is not "no battery".
        assert_eq!(classify_windows(1, 255, 0), PowerState::Ac);
        assert_eq!(classify_windows(255, 255, 0), PowerState::Unknown);
        // "No system battery" wins over the line and the saver flag.
        assert_eq!(classify_windows(0, 128, 0), PowerState::NoBattery);
        assert_eq!(classify_windows(255, 128, 1), PowerState::NoBattery);
    }

    #[test]
    fn odd_supply_values_do_not_mislead() {
        // `online` 2 is "Online Programmable" (sysfs-class-power): plugged in.
        assert_eq!(
            classify_supplies(&[
                supply("Battery", None, Some("Not charging")),
                supply("USB", Some("2"), None),
            ]),
            PowerState::Ac
        );
        // A battery that is not charging behind an offline charger: on battery.
        assert_eq!(
            classify_supplies(&[
                supply("Battery", None, Some("Not charging")),
                supply("Mains", Some("0"), None),
            ]),
            PowerState::Battery
        );
        // A charger without an `online` file says nothing; a charging battery alone neither.
        assert_eq!(
            classify_supplies(&[
                supply("Battery", None, Some("Charging")),
                supply("Mains", None, None),
            ]),
            PowerState::Unknown
        );
        assert_eq!(
            classify_supplies(&[supply("Battery", None, Some("Unknown"))]),
            PowerState::Unknown
        );
        // An entry without a readable `type` is neither battery nor charger.
        let nameless = Supply {
            online: Some("1".to_owned()),
            ..Supply::default()
        };
        assert_eq!(classify_supplies(&[nameless]), PowerState::NoBattery);
        // UPS and Wireless are no system battery and no charger here.
        assert_eq!(
            classify_supplies(&[supply("UPS", Some("1"), Some("Discharging"))]),
            PowerState::NoBattery
        );
    }

    fn supply(kind: &str, online: Option<&str>, status: Option<&str>) -> Supply {
        Supply {
            kind: Some(kind.to_owned()),
            online: online.map(str::to_owned),
            status: status.map(str::to_owned),
            scope: None,
        }
    }

    #[test]
    fn supplies_map_to_the_state() {
        let battery = supply("Battery", None, Some("Discharging"));
        let charging = supply("Battery", None, Some("Charging"));
        let mains_on = supply("Mains", Some("1"), None);
        let mains_off = supply("Mains", Some("0"), None);
        let usb_on = supply("USB", Some("1"), None);
        assert_eq!(classify_supplies(&[]), PowerState::NoBattery);
        assert_eq!(
            classify_supplies(std::slice::from_ref(&mains_on)),
            PowerState::NoBattery
        );
        assert_eq!(
            classify_supplies(&[charging.clone(), mains_on]),
            PowerState::Ac
        );
        assert_eq!(
            classify_supplies(&[charging.clone(), mains_off.clone(), usb_on]),
            PowerState::Ac
        );
        assert_eq!(
            classify_supplies(std::slice::from_ref(&battery)),
            PowerState::Battery
        );
        assert_eq!(
            classify_supplies(&[supply("Battery", None, Some("Full")), mains_off]),
            PowerState::Battery
        );
        // A full battery without any supply to ask: cannot tell.
        assert_eq!(
            classify_supplies(&[supply("Battery", None, Some("Full"))]),
            PowerState::Unknown
        );
    }

    #[test]
    fn a_device_battery_is_not_the_system_battery() {
        let mut mouse = supply("Battery", None, Some("Discharging"));
        mouse.scope = Some("Device".to_owned());
        assert_eq!(classify_supplies(&[mouse]), PowerState::NoBattery);
    }

    #[test]
    fn supplies_are_read_from_their_folder() {
        let dir = TempDir::new("power-supplies");
        let bat = dir.path().join("BAT0");
        let ac = dir.path().join("AC");
        fs::create_dir_all(&bat).unwrap();
        fs::create_dir_all(&ac).unwrap();
        fs::write(bat.join("type"), "Battery\n").unwrap();
        fs::write(bat.join("status"), "Discharging\n").unwrap();
        fs::write(ac.join("type"), "Mains\n").unwrap();
        fs::write(ac.join("online"), "0\n").unwrap();
        let mut supplies = read_supplies(dir.path());
        supplies.sort_by(|a, b| a.kind.cmp(&b.kind));
        assert_eq!(
            supplies,
            vec![
                supply("Battery", None, Some("Discharging")),
                supply("Mains", Some("0"), None),
            ]
        );
        assert_eq!(classify_supplies(&supplies), PowerState::Battery);
        assert!(read_supplies(&dir.path().join("missing")).is_empty());
    }

    const PMSET_LAPTOP_AC: &str = "Now drawing from 'AC Power'\n \
        -InternalBattery-0 (id=4653155)\t100%; charged; 0:00 remaining present: true\n";
    const PMSET_LAPTOP_BATTERY: &str = "Now drawing from 'Battery Power'\n \
        -InternalBattery-0 (id=4653155)\t87%; discharging; 5:12 remaining present: true\n";

    #[test]
    fn pmset_output_maps_to_the_state() {
        assert_eq!(classify_pmset(PMSET_LAPTOP_AC, false), PowerState::Ac);
        assert_eq!(
            classify_pmset(PMSET_LAPTOP_BATTERY, false),
            PowerState::Battery
        );
        assert_eq!(
            classify_pmset(PMSET_LAPTOP_BATTERY, true),
            PowerState::Saver
        );
        // Charging and "AC attached; not charging" are both drawn from AC.
        let not_charging = PMSET_LAPTOP_AC.replace("charged", "AC attached; not charging");
        assert_eq!(classify_pmset(&not_charging, false), PowerState::Ac);
    }

    #[test]
    fn low_power_mode_on_ac_changes_nothing() {
        assert_eq!(classify_pmset(PMSET_LAPTOP_AC, true), PowerState::Ac);
    }

    #[test]
    fn a_mac_without_battery_says_so() {
        assert_eq!(
            classify_pmset("Now drawing from 'AC Power'\n", false),
            PowerState::NoBattery
        );
        // A UPS is no system battery, also while power comes from it.
        let ups = "Now drawing from 'UPS Power'\n -Back-UPS 700 (id=1)\t95%; discharging\n";
        assert_eq!(classify_pmset(ups, true), PowerState::NoBattery);
        assert_eq!(classify_pmset("", false), PowerState::NoBattery);
    }

    #[test]
    fn an_unknown_pmset_source_is_unknown() {
        let odd = PMSET_LAPTOP_AC.replace("AC Power", "Solar Power");
        assert_eq!(classify_pmset(&odd, false), PowerState::Unknown);
        let missing = " -InternalBattery-0 (id=1)\t50%; charging\n";
        assert_eq!(classify_pmset(missing, false), PowerState::Unknown);
    }

    #[test]
    fn the_state_is_emitted_only_on_change() {
        let latest = Mutex::new(PowerState::Ac);
        assert_eq!(PowerMonitor::store(&latest, PowerState::Ac), None);
        assert_eq!(
            PowerMonitor::store(&latest, PowerState::Battery),
            Some(PowerState::Battery)
        );
        assert_eq!(PowerMonitor::store(&latest, PowerState::Battery), None);
    }

    #[test]
    fn a_refresh_updates_the_current_state() {
        let monitor = PowerMonitor::default();
        assert_eq!(monitor.current(), PowerState::Unknown);
        assert_eq!(monitor.refresh(PowerState::Saver), PowerState::Saver);
        assert_eq!(monitor.current(), PowerState::Saver);
    }

    #[test]
    fn the_state_serializes_in_camel_case() {
        assert_eq!(
            serde_json::to_string(&PowerState::NoBattery).unwrap(),
            "\"noBattery\""
        );
        assert_eq!(serde_json::to_string(&PowerState::Ac).unwrap(), "\"ac\"");
    }
}

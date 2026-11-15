---
title: Power status sources
type: source
tags: [power, battery, windows, linux, measurement]
sources: []
raw: raw/2026-11-14-power-status-sources.md
url: https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-system_power_status
retrieved: 2026-11-14
created: 2026-11-14
updated: 2026-11-14
---

# Power status sources

Microsoft's reference of `SYSTEM_POWER_STATUS`, the kernel's sysfs ABI for power supplies, and
one own reading on a desktop.

**Key points.**

- `ACLineStatus` is 0 (offline), 1 (online) or 255 (unknown).
- `BatteryFlag` is a bit set; 128 means "No system battery", 255 means unknown. 255 has the 128
  bit set, so "no battery" must exclude 255.
- `SystemStatusFlag` is the battery saver (1 on), since Windows 10; before it was reserved and 0.
  Changes can be notified through `RegisterPowerSettingNotification` with
  `GUID_POWER_SAVING_STATUS`.
- Linux: `type` is Battery, UPS, Mains, USB or Wireless; `online` is 0, 1 or 2; `status` of a
  battery is Unknown, Charging, Discharging, Not charging or Full. The fetched ABI page has no
  entry for `scope`.
- Own reading, Windows 11 10.0.26300 desktop: AC 1, BatteryFlag 128, percent 255, saver 0.

**Significance for Hangar.** The pure mapping in `src-tauri/src/power.rs` follows these values;
the desktop reading confirms that a machine without battery is told apart, so the setting stays
hidden there. See [[power-detection]].

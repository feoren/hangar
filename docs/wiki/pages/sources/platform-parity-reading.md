---
title: Platform parity reading
type: source
tags: [macos, linux, notifications, power, gpu, badge, reading]
sources: []
raw: raw/2026-11-20-platform-parity-reading.md
url: https://docs.kernel.org/gpu/drm-usage-stats.html
retrieved: 2026-11-20
created: 2026-11-20
updated: 2026-11-20
---

# Platform parity reading

Docs and crate sources read to close four gaps between Windows and the other platforms: macOS
notifications, macOS battery state, GPU figures on Linux and macOS, the Linux launcher badge.
Nothing ran on a Mac or a Linux desktop; the new code was type-checked for both targets only.

**Key points.**

- **macOS notifications.** `objc2-user-notifications` 0.3.2 binds `UNUserNotificationCenter`
  (not main thread only), its delegate protocol (`willPresentNotification`,
  `didReceiveNotificationResponse`), permission requests and removal by identifier; the delegate
  is held weakly. Two independent projects (par-term, desktop-notifier) use the center only from a
  bundled, signed app and fall back elsewhere; a bare executable without a bundle identifier
  cannot use it. No source settles whether an ad hoc signature is enough for permission.
  mac-notification-sys still uses the deprecated `NSUserNotification` API.
- **macOS power.** `pmset -g batt` names the source ("AC Power", "Battery Power", "UPS Power")
  and lists `-InternalBattery-0` only on Macs with a battery. `NSProcessInfo` has
  `isLowPowerModeEnabled`.
- **Linux GPU.** The kernel's DRM client statistics in fdinfo give busy nanoseconds (or cycles,
  xe) per engine, an engine capacity, and resident memory per region; `drm-client-id` (with
  `drm-pdev`) deduplicates shared descriptors; `drm-memory-*` is amdgpu's alias of
  `drm-resident-*`. amdgpu adds `gpu_busy_percent` and `mem_info_vram_used` in sysfs. NVML's
  `nvmlMemory_t` and `nvmlUtilization_t` layouts come from `nvml.h`.
- **macOS GPU.** `ioreg -r -d 1 -w 0 -c IOAccelerator` prints a one-line `PerformanceStatistics`
  dictionary per accelerator with `Device Utilization %` and `In use system memory` (sample from
  an M3 Pro); the keys are undocumented and vary by GPU generation.
- **Linux badge.** Tauri 2.12.5's `set_badge_count` reaches tao 0.37.2's `TaskbarIndicator`,
  which loads libunity and does nothing unless the Unity shell runs; it sends no D-Bus signal of
  its own.

**Significance for Hangar.** Basis of the macOS notifier (`src-tauri/src/notify/macos.rs`), the
macOS branch of [[power-detection]], [[gpu-readings]], and the decision to keep the Linux badge
hidden ([[0013-linux-release]], [[0017-macos-release]]).

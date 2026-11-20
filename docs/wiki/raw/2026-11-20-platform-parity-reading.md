# Platform parity: reading of docs and source (2026-11-20)

Retrieved on 2026-11-20 for macOS notifications and power state, GPU figures on Linux and macOS,
and the launcher badge on Linux. Quotes are verbatim excerpts. Nothing was run on a Mac or a Linux
desktop; the macOS and Linux code was type-checked with `cargo check` and `cargo clippy` for
`aarch64-apple-darwin` and `x86_64-unknown-linux-gnu` in a scratch crate holding copies of the
changed modules (no link step, no Tauri build).

## macOS notifications

Local crate sources in the cargo registry (versions from `src-tauri/Cargo.lock`):

- `objc2-user-notifications` 0.3.2, `src/generated/UNUserNotificationCenter.rs`:
  `pub fn currentNotificationCenter() -> Retained<UNUserNotificationCenter>`;
  `setDelegate` with the doc line "This is a [weak property][objc2::topics::weak_property].";
  `requestAuthorizationWithOptions_completionHandler(&self, options: UNAuthorizationOptions,
  completion_handler: &block2::DynBlock<dyn Fn(Bool, *mut NSError)>)`;
  `addNotificationRequest_withCompletionHandler(&self, request: &UNNotificationRequest,
  completion_handler: Option<&block2::DynBlock<dyn Fn(*mut NSError)>>)`;
  `removeDeliveredNotificationsWithIdentifiers(&self, identifiers: &NSArray<NSString>)`;
  `removePendingNotificationRequestsWithIdentifiers`. The class is not marked main thread only.
- Same file, protocol `UNUserNotificationCenterDelegate`, both methods `#[optional]`:
  `userNotificationCenter:willPresentNotification:withCompletionHandler:` (handler takes
  `UNNotificationPresentationOptions`) and
  `userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:`.
- `UNNotificationPresentationOptions`: `Badge = 1<<0`, `Sound = 1<<1`, `Alert = 1<<2`,
  `List = 1<<3`, `Banner = 1<<4`.
- `UNNotificationResponse.rs`: `pub static UNNotificationDefaultActionIdentifier: &'static
  NSString;`, `UNNotificationDismissActionIdentifier`, `actionIdentifier()`, `notification()`.
- `objc2` 0.6.5, `src/macros/define_class.rs`, "Thread safety": "The Objective-C runtime is
  thread-safe, so any classes you create yourself will automatically be [`Send`] and [`Sync`]
  (via auto traits) provided that: 1. The superclass is thread-safe, or is [`NSObject`]. 2. The
  ivars are thread-safe. 3. The thread kind is not [`MainThreadOnly`]."
- `objc2-foundation` 0.3.2: `NSBundle::mainBundle()`, `bundleIdentifier(&self) ->
  Option<Retained<NSString>>`; `NSProcessInfo::isLowPowerModeEnabled(&self) -> bool`.

Web:

- docs.rs, `par-term` 0.40.0 `src/platform/notify_macos.rs` (summary of the fetched page): uses
  `UNUserNotificationCenter` only when the process has a bundle identifier; the module
  documentation says the native API "only functions from a signed/bundled app"; a bare
  `cargo run` binary falls back to `osascript`; the center holds its delegate weakly, so the code
  keeps the delegate alive; a request with the same identifier replaces the earlier one.
- desktop-notifier (Python), `src/desktop_notifier/backends/macos.py`, module docstring: "Only
  available from signed app bundles if called from the main executable or from a signed Python
  framework (for example from python.org)."; "Requires a running CFRunLoop to invoke callbacks."
- Apple Developer Forums search for "Notifications are not allowed for this application"
  (`UNErrorDomain` code 1): reports exist for unsigned command line tools and after OS updates;
  no source states whether an ad hoc signature is enough. One Stack Overflow comment claims "only
  code-signed applications will be granted authorisation". Apple DTS notes that an ad hoc
  signature gives TCC no stable identity, so grants can be lost after each build.
- github.com/h4llow3En/mac-notification-sys README: license "Apache License, Version 2.0 or MIT
  license"; TODO "Switch to UserNotification if possible" (so it uses the older
  `NSUserNotification` API).

## macOS power

- osxdaily, "Get MacBook Battery Life Percent Time Remaining from Command Line" (2015-12-10) and
  its comments: `pmset -g batt` prints "Now drawing from 'AC Power'" or "'Battery Power'" and a
  line per battery like ` -InternalBattery-0 (id=4653155)	100%; charged; 0:00 remaining
  present: true`; a commenter: "if you use this command on a Mac that does not have an internal
  battery it won't report anything" (for the battery line).
- iFixit answer 601599 shows the same two-line shape on a MacBook with a failed battery.

## GPU, Linux

URL: https://docs.kernel.org/gpu/drm-usage-stats.html

- "File shall contain one key value pair per one line of text." "Colon character (:) must be
  used to delimit keys and values."
- `drm-driver: <valstr>` is mandatory. `drm-pdev: <aaaa:bb.cc.d>`: "For PCI devices this should
  contain the PCI slot address of the device in question." `drm-client-id: <uint>`: "Unique value
  relating to the open DRM file descriptor used to distinguish duplicated and shared file
  descriptors." "Userspace should make sure to not double account any usage statistics by using
  the above described criteria." The id is globally unique, or unique per device together with
  `drm-pdev`.
- `drm-engine-<keystr>: <uint> ns`: time "the respective GPU engine spent busy executing
  workloads". `drm-engine-capacity-<keystr>: <uint>`: "In the absence of this tag parser shall
  assume capacity of one." `drm-cycles-<keystr>` and `drm-total-cycles-<keystr>`: busy and total
  cycles of the engine.
- Memory: "Default unit shall be bytes with optional unit specifiers of 'KiB' or 'MiB'". The
  region name "memory" is reserved for system memory. `drm-total-<region>`,
  `drm-shared-<region>`, `drm-resident-<region>`; `drm-memory-<region>` is deprecated, "only
  printed by amdgpu; it is an alias for drm-resident-<region>".

URL: https://docs.kernel.org/gpu/amdgpu/thermal.html

- "The amdgpu driver provides a sysfs API for reading how busy the GPU is as a percentage. The
  file gpu_busy_percent is used for this." "The SMU firmware computes a percentage of load based
  on the aggregate activity level in the IP cores." The page gives no folder.

URL: https://docs.kernel.org/gpu/amdgpu/driver-misc.html

- "The file mem_info_vram_used is used for this and returns the total amount of currently used
  VRAM in bytes."

URL: https://raw.githubusercontent.com/NVIDIA/go-nvml/main/gen/nvml/nvml.h

- `nvmlMemory_t`: `unsigned long long total; unsigned long long free; unsigned long long used;`
- `nvmlUtilization_t`: `unsigned int gpu; unsigned int memory;`
- `typedef struct nvmlDevice_st* nvmlDevice_t;` `NVML_SUCCESS = 0`.
- `nvmlReturn_t DECLDIR nvmlDeviceGetMemoryInfo(nvmlDevice_t device, nvmlMemory_t *memory);`
- `#define nvmlInit nvmlInit_v2` and similar aliases for the `_v2` names. The declarations of
  `nvmlInit_v2`, `nvmlShutdown`, `nvmlDeviceGetCount_v2`, `nvmlDeviceGetHandleByIndex_v2` and
  `nvmlDeviceGetUtilizationRates` were not in the fetched excerpts.

## GPU, macOS

- docs.rs, `syswatch` 0.2.2 `src/collect/gpu.rs` (summary of the fetched page): runs
  `ioreg -r -d 1 -w 0 -c IOAccelerator`, finds lines starting with
  `"PerformanceStatistics" = {`, pairs `"Key"=value` split on commas, numbers only. Test sample
  "Captured verbatim from `ioreg -r -d 1 -w 0 -c IOAccelerator` on M3 Pro":
  `"PerformanceStatistics" = {"In use system memory (driver)"=0,"Alloc system memory"=16749051904,"Tiler Utilization %"=7,`
  and further `"Device Utilization %"=16`, `"In use system memory"=568164352`.
- docs.rs, `vor` 0.1.0 `src/gpu/mac.rs` (search result): keys of `PerformanceStatistics` "vary
  across GPU generations", so a miss is tolerated.
- No Apple documentation of these keys was found.

## Launcher badge, Linux

Local sources:

- `tauri` 2.12.5 `src/window/mod.rs`: `set_badge_count(&self, count: Option<i64>)` passes
  `Some(format!("{}.desktop", self.package_info().name))` as the desktop file.
- `tauri-runtime-wry` 2.12.3 `src/lib.rs`: on Linux `window.set_badge_count(_count,
  _desktop_filename)` (tao).
- `tao` 0.37.2 `src/platform_impl/linux/taskbar.rs`: `TaskbarIndicator` loads
  `libunity.so.4`, `libunity.so.6` or `libunity.so.9` through dlopen2, and `update_count` returns
  at once `if !self.is_unity_running()`, which asks `unity_inspector_get_unity_running`. Only
  then does it call `unity_launcher_entry_set_count` and `unity_launcher_entry_set_count_visible`.
  No D-Bus signal is sent by tao itself.

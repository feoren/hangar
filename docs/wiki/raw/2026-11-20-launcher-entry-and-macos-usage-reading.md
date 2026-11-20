# Linux launcher entry and macOS usage: reading of docs and source (2026-11-20)

Retrieved on 2026-11-20. Quotes are verbatim excerpts. Nothing was run on a Linux desktop or a
Mac; the code was type-checked for `x86_64-unknown-linux-gnu` and `aarch64-apple-darwin` only.

## Desktop file of the .deb

URL: https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/linux/freedesktop/mod.rs

```rust
  let product_name = settings.product_name();
  let desktop_file_name = format!("{product_name}.desktop");
  let path = PathBuf::from("usr/share/applications").join(desktop_file_name);
```

`tauri-codegen` 2.7.4 `src/context.rs` (local): `PackageInfo.name` is `config.product_name` when
set, so Tauri's own `set_badge_count` names the same `Hangar.desktop` (`productName` "Hangar" in
`src-tauri/tauri.conf.json`).

## The desktop file a launcher started from

Web search over GLib sources and docs:

- GAppInfo documentation (gtk-doc copy): "On UNIX, this function sets the
  `GIO_LAUNCHED_DESKTOP_FILE` environment variable with the path of the launched desktop file and
  `GIO_LAUNCHED_DESKTOP_FILE_PID` to the process id of the launched process. This can be used to
  ignore `GIO_LAUNCHED_DESKTOP_FILE`, should it be inherited by further processes."
- GLib commit of June 2010 ("Pass on the desktop file path when lauching"): "Set
  GIO_LAUNCHED_DESKTOP_FILE and GIO_LAUNCHED_DESKTOP_FILE_PID env vars."
- `gio/gio-launch-desktop.c` header: "Executable wrapper to set GIO_LAUNCHED_DESKTOP_FILE_PID."
- `gio/tests/appinfo-test.c` checks that the PID variable equals the launched process's id.

No source was found for what KDE Plasma's launcher or AppImageLauncher set in the environment.
AppImageLauncher's desktop file names (`appimagekit_<hash>-<name>.desktop` in
`~/.local/share/applications`, `Exec=` the AppImage's path) are from memory, not from a fetched
source.

## macOS bindings in libc 0.2.191 (local source, `src/unix/bsd/apple/mod.rs`)

- `host_statistics`, `host_statistics64`, `HOST_CPU_LOAD_INFO` (3), `HOST_VM_INFO64` (4),
  `HOST_CPU_LOAD_INFO_COUNT` (4), `HOST_VM_INFO64_COUNT` (size of `vm_statistics64` in
  `integer_t`), `host_cpu_load_info { cpu_ticks: [natural_t; CPU_STATE_MAX] }`, `CPU_STATE_IDLE`
  2, `KERN_SUCCESS` 0.
- `vm_statistics64` fields include `free_count`, `active_count`, `inactive_count`, `wire_count`,
  `purgeable_count`, `speculative_count`, `compressor_page_count`, `external_page_count`,
  `internal_page_count`, plus newer tag storage fields at the end.
- `mach_host_self` and `mach_timebase_info` carry `#[deprecated(since = "0.2.55", note = "Use the
  `mach2` crate instead")]`.
- `proc_listallpids(buffer, buffersize) -> c_int`, `proc_pidinfo(pid, flavor, arg, buffer,
  buffersize) -> c_int`, `PROC_PIDTBSDINFO` 3, `proc_bsdinfo` with `pbi_ppid` and `pbi_pgid`
  (no session id), `proc_pid_rusage(pid, flavor, buffer)`, `RUSAGE_INFO_V2` 2, `rusage_info_v2`
  with `ri_user_time`, `ri_system_time`, `ri_resident_size`, `ri_phys_footprint`,
  `ri_child_user_time`, `ri_child_system_time`; `getsid(pid)`.
- No document read here states the unit of the `rusage_info` times. That they count Mach time
  units, which Apple silicon ticks at 24 MHz (timebase 125/3), is from memory.

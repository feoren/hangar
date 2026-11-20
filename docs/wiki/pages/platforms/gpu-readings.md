---
title: GPU readings per platform
type: platform
tags: [gpu, windows, linux, macos, contract]
sources: ["[[platform-parity-reading]]", "[[system-usage-measurement]]"]
created: 2026-11-20
updated: 2026-11-20
---

# GPU readings per platform

Where the GPU load and GPU memory of the system bar and the session rows come from
(`src-tauri/src/gpu.rs`, `GpuMonitor`). The contract of the two commands is in
[[session-resources-and-phone-push]]; one snapshot serves both and is cached for 1 s.

| Platform | Machine (system bar) | Per session (rows) |
| --- | --- | --- |
| Windows | PDH counters, busiest engine, dedicated adapter memory | PDH per process id |
| Linux | amdgpu sysfs and NVIDIA's `nvidia-smi`; else none | DRM client statistics in fdinfo |
| macOS | IOAccelerator `PerformanceStatistics` via `ioreg` | none |

The window shows whatever comes back: a gauge or cell with `null` is hidden. The rows keep a GPU
column (empty cells for sessions without GPU use) only where the machine reports a GPU and the
platform has per-session figures (`keepsGpuColumn`, `hasSessionGpu` in `src/platform.ts`), so on
macOS no row gets an empty GPU column.

## Linux

**Per process** (`src-tauri/src/gpu/drm.rs`, [[platform-parity-reading]]): only the processes of
Hangar's sessions are read, not the whole machine: a process whose terminal session is led by a
direct child of Hangar (the same rule `proctree.rs` uses for CPU and memory). For each open file
whose link target is under `/dev/dri/`, `/proc/<pid>/fdinfo/<fd>` is parsed:

- `drm-engine-<name>: <n> ns` (or `drm-cycles-<name>` with `drm-total-cycles-<name>` on xe) is a
  busy counter; the load is the growth since the previous reading divided by the time passed (or
  by the total cycles) and by `drm-engine-capacity-<name>` (default 1). The first reading only
  sets the baseline, as on Windows.
- Memory counts only device memory: regions `vram*` and `local*`, `drm-resident-*` first, else
  amdgpu's older `drm-memory-*`. GTT and system memory are left out, like the dedicated memory on
  Windows, so an integrated GPU shows load but no memory.
- A client counts once per `drm-pdev` and `drm-client-id` (a shared or duplicated descriptor
  carries the same id); without an id each file counts on its own.
- Engines are keyed by device and engine name, so a tree's processes on the same engine add up
  and the busiest engine is the tree's load, as on Windows. At most 4096 open files per process.

**Machine:** `gpu_busy_percent` and `mem_info_vram_used` of every `/sys/class/drm/card<n>/device`
(amdgpu only), plus `/usr/bin/nvidia-smi --query-gpu=utilization.gpu,memory.used
--format=csv,noheader,nounits` when the driver installed it (one line per device, memory in MiB;
run at most every 5 s, with the 2 s limit of `bounded_output.rs`). NVML was first loaded into
Hangar with `dlopen`; on a machine without the driver the failed `dlopen` crashed the process
(SIGSEGV in CI on ubuntu-latest and in WSL with the library hidden; a minimal program with the same
call did not crash, the cause inside Hangar's binary is unknown), so the driver's own tool runs in
a process of its own instead. The busiest device is the load, the memory is summed. An Intel GPU
has neither source, so its system bar shows no GPU while the rows still do.

> [!question] Unverified
> - Nothing ran on a Linux desktop. The fdinfo shapes in the tests follow the kernel
>   documentation, not captured output; the xe cycles formula (divided by capacity) and the
>   `/sys/class/drm/card<n>/device` folder of the amdgpu files are assumptions.
> - NVIDIA's proprietary driver gives no fdinfo engine statistics (assumed), so NVIDIA sessions
>   show no per-session GPU.
> - The `nvidia-smi` output shape follows NVIDIA's documented CSV query format, not captured
>   output; `[N/A]` for a missing value is assumed. A driver that installs the tool elsewhere than
>   `/usr/bin` gives no machine figure.

## macOS

`/usr/sbin/ioreg -r -d 1 -w 0 -c IOAccelerator`, parsed by `src-tauri/src/gpu/ioreg.rs`: per
accelerator `Device Utilization %` (fallback `GPU Activity(%)`) and `In use system memory`
(fallback `vramUsedBytes`); the busiest accelerator is the load, the memory is summed. The values
are gauges, so the first reading already counts. A machine whose registry has no
`PerformanceStatistics` (a virtual machine) is given up for good after the first try. `ioreg` gets 2 s (killed past that, no reading that time). The GPU monitor does not hold its lock while a reading runs: a second caller meanwhile gets the previous snapshot. The
registry gives no per-process figures without root, so the rows show none.

> [!question] Unverified
> The keys are undocumented; only an Apple silicon sample was seen. The Intel and AMD fallbacks
> are assumptions, and Apple silicon's "In use system memory" is unified memory, not VRAM.

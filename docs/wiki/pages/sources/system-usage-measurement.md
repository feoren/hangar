---
title: System usage measurement
type: source
tags: [resources, windows, pdh, gpu, measurement]
sources: []
raw: raw/2026-11-12-system-usage-measurement.md
url: ""
retrieved: 2026-11-12
created: 2026-11-12
updated: 2026-11-12
---

# System usage measurement

Own measurement on Windows 11 (10.0.26300) of the system bar's backend reading
(`src-tauri/src/system_usage.rs`).

**Key points.**

- One reading (CPU, memory and a PDH collection of `GPU Engine(*)\Utilization Percentage` with 806
  engine instances) took about 2 ms on a debug build.
- The first reading has no CPU and no GPU value (both are rates); the second had CPU 7.3 %,
  RAM 31.4 %, GPU 13.0 %.
- `Get-Counter` on the same counter, grouped per engine and summed over processes, showed the
  busiest engine at 15.4 % a few seconds later: the aggregation in `gpu_percent` reads the counter
  the same way.
- Not measured: a side-by-side comparison with the task manager.

**Significance for Hangar.** Polling every 1.5 s costs a few milliseconds per reading, so one
shared poll while the window is visible is cheap enough; see
[[session-resources-and-phone-push]].

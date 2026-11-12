# System usage measurement (2026-11-12)

Platform: Windows 11 Pro 10.0.26300, x64. Code: `src-tauri/src/system_usage.rs` (Hangar 0.6.0).

## 1. Backend reading

Command:

```
cargo test --manifest-path src-tauri/Cargo.toml --lib system_usage -- --nocapture
```

Output of `reads_this_machine` (two readings 1.1 s apart, the second one timed):

```
first SystemUsage { cpu: None, memory: Some(31.4), gpu: None }, second SystemUsage { cpu: Some(7.3), memory: Some(31.4), gpu: Some(13.0) }, reading took 2.1449ms
```

## 2. Cross-check with Get-Counter

Command (PowerShell 7), same counter, grouped by engine (luid, phys, eng), summed:

```
$s = (Get-Counter '\GPU Engine(*)\Utilization Percentage').CounterSamples
$s | Group-Object { if ($_.InstanceName -match '(luid_.*?)_engtype_') { $matches[1] } else { 'x' } } |
  ForEach-Object { [pscustomobject]@{ k = $_.Name; v = ($_.Group | Measure-Object CookedValue -Sum).Sum } } |
  Sort-Object v -Descending | Select-Object -First 3
```

Output (a few seconds after section 1, so not the same instant):

```
k                                           v
-                                           -
luid_0x00000000_0x00013f4f_phys_0_eng_0 15,37
luid_0x00000000_0x00013f4f_phys_0_eng_6  8,23
luid_0x00000000_0x00013f4f_phys_0_eng_2  2,90

instances: 806
```

Not done: a side-by-side comparison with the task manager's GPU figure.

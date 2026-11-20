---
title: Power detection per platform
type: platform
tags: [power, battery, windows, linux, macos, contract]
sources: ["[[power-status-sources]]", "[[platform-parity-reading]]"]
created: 2026-11-14
updated: 2026-11-20
---

# Power detection per platform

How Hangar tells battery from AC for the power saving mode (`src-tauri/src/power.rs`,
`src/power.ts`).

## Contract

- State (`PowerState`, serde camelCase): `unknown`, `noBattery`, `ac`, `battery`, `saver`
  (battery with the system's battery saver on).
- Command `power_state` (main window only) reads the state afresh. Event `power-changed` carries
  the new state, sent only on a change. One thread reads every 30 s; the window also asks when it
  starts, gains focus or becomes visible.
- The window offers the setting only for `ac`, `battery` and `saver`. Saving is on for `battery`
  and `saver` with the setting on (default on); on AC nothing changes, also with a battery saver
  that was turned on while plugged in.

## Windows

`GetSystemPowerStatus` (`windows-sys` feature `Win32_System_Power`), mapped by
`classify_windows` ([[power-status-sources]]):

| `BatteryFlag` | `ACLineStatus` | `SystemStatusFlag` | State |
| --- | --- | --- | --- |
| 128 bit set, not 255 | any | any | `noBattery` |
| other | 1 | any | `ac` |
| other | 0 | 1 | `saver` |
| other | 0 | 0 | `battery` |
| other | 255 | any | `unknown` |

Polling instead of `RegisterPowerSettingNotification`: the notification goes to a window
procedure or service handler, which Tauri's window loop does not hand out, while the poll is one
cheap call every 30 s. A desktop measured as AC 1, flag 128, so it reads `noBattery`
([[power-status-sources]]).

> [!question] Unverified
> A laptop on battery and with battery saver on was not measured; the mapping relies on the docs.

## Linux

`/sys/class/power_supply/*/{type,online,status,scope}`, mapped by `classify_supplies`: no
`Battery` of the system means `noBattery`; any `Mains` or `USB` supply with `online` other than 0
means `ac`; a `Discharging` battery, or only offline supplies, means `battery`; else `unknown`.
Entries with `scope` `Device` (a mouse, a headset) are ignored. No battery saver (that would need
power-profiles-daemon over D-Bus).

> [!question] Unverified
> The `scope` attribute is not on the fetched ABI page, and no Linux machine was measured. USB-C
> chargers reporting as type `USB` is an assumption.

## macOS

`/usr/bin/pmset -g batt` (argument list, absolute path) mapped by `classify_pmset`, with
`NSProcessInfo.isLowPowerModeEnabled` as the battery saver ([[platform-parity-reading]]): no
`-InternalBattery` line means `noBattery` (a desktop, also one on a UPS); "Now drawing from 'AC
Power'" means `ac`, also with Low Power Mode on; "'Battery Power'" means `battery`, or `saver`
with Low Power Mode on; anything else `unknown`. Same 30 s poll as elsewhere; `pmset` gets 2 s (`src-tauri/src/bounded_output.rs`, killed past that, then `unknown`), and the command `power_state` reads on a blocking thread. Chosen over IOKit's
`IOPSCopyPowerSourcesInfo`: no CoreFoundation dictionary handling in unsafe code, and the parser
is a pure function tested on every platform.

> [!question] Unverified
> No Mac was measured; the `pmset` lines in the tests follow published output of MacBooks, the
> desktop case a user comment.

## What saving changes in the window

The system bar polls every 5 s instead of 1.5 s and shows still bars instead of sparklines (their
history starts over when saving turns on or off); the session rows every 5 s instead of 2 s; the
git branches every 60 s instead of 15 s; the stall hint compares the screens of working agent
sessions every 30 s instead of 10 s (`StallMonitor.setSampleInterval`, its own timer on an
injected clock rather than `PollTimer`, so its tests keep fake time; the time since the last
progress is kept, so the hint comes at most half a minute late). The root element carries `power-saving`, which stills
the row pulses, blinking state dots, working dots, the running sub-agent dot and the rail's
waiting ring, like reduced motion. The previews in the attention settings (`.look-card`) and the
terminal settings' cursor preview keep moving: they show what a choice does once plugged in. Hidden terminal panes are already moved out of
the viewport (`.pane`), so nothing is added for them. A leaf at the system bar's end with the
label "Energy saving on" shows while saving.

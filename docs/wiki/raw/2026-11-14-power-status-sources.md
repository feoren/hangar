# Power status sources (2026-11-14)

## Microsoft Learn: SYSTEM_POWER_STATUS (winbase.h)

URL: https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-system_power_status
Retrieved: 2026-11-14 (page updated 2024-02-22)

Excerpts, verbatim:

- `ACLineStatus`: "The AC power status." Values: 0 "Offline", 1 "Online", 255 "Unknown status".
- `BatteryFlag`: "The battery charge status. This member can contain one or more of the following
  flags." Values: 1 "High", 2 "Low", 4 "Critical", 8 "Charging", 128 "No system battery",
  255 "Unknown status" with the explanation "unable to read the battery flag information"
  (the page joins both with a dash). "The value is zero if the
  battery is not being charged and the battery capacity is between low and high."
- `SystemStatusFlag`: "The status of battery saver. To participate in energy conservation, avoid
  resource intensive tasks when battery saver is on. To be notified when this value changes, call
  the RegisterPowerSettingNotification function with the power setting GUID,
  GUID_POWER_SAVING_STATUS." Values: 0 "Battery saver is off.", 1 "Battery saver on. Save energy
  where possible." Note: "This flag and the GUID_POWER_SAVING_STATUS GUID were introduced in
  Windows 10. This flag was previously reserved, named Reserved1, and had a value of 0."
- Requirements: minimum supported client Windows XP [desktop apps only].

## Linux kernel: Documentation/ABI/testing/sysfs-class-power

URL: https://www.kernel.org/doc/Documentation/ABI/testing/sysfs-class-power
Retrieved: 2026-11-14

- `/sys/class/power_supply/<supply_name>/type`: "Battery", "UPS", "Mains", "USB", "Wireless".
- `/sys/class/power_supply/<supply_name>/online`: 0 "Offline", 1 "Online Fixed - Fixed Voltage
  Supply", 2 "Online Programmable - Programmable Voltage Supply".
- `/sys/class/power_supply/<supply_name>/status`: "Unknown", "Charging", "Discharging",
  "Not charging", "Full".
- The fetched page has no entry for `scope`.

## Own measurement

Windows 11 Pro 10.0.26300, desktop without battery, `GetSystemPowerStatus` through a PowerShell
P/Invoke wrapper:

```
AC    : 1
Flag  : 128
Pct   : 255
Saver : 0
Life  : 4294967295
Full  : 4294967295
```

Not measured: a laptop on battery, battery saver on, any Linux machine.

# Measurement: app execution alias of the Store PowerShell (2026-11-12)

Platform: Windows 11 Pro 10.0.26300, PowerShell 7.7.0 from the Microsoft Store (MSI copy removed).
`pwsh` on PATH resolves to `C:\Users\<user>\AppData\Local\Microsoft\WindowsApps\pwsh.exe`.

Git Bash `ls -la` on the alias:

```
pwsh -> /c/Program Files/WindowsApps/Microsoft.PowerShell_7.7.0.0_x64__8wekyb3d8bbwe/pwsh.exe
```

Rust std (temporary test in `src-tauri/tests/`, removed afterwards), output:

```
metadata: Ok((0, true, Some(SystemTime { .. })))
symlink_metadata: Ok((0, true, false))
open: Err(Os { code: 1920, kind: Uncategorized, message: "Das System kann auf die Datei nicht zugreifen." })
read_link: Err(Error { kind: Uncategorized, message: "Unsupported reparse point type" })
canonicalize: Err(Os { code: 1920, kind: Uncategorized, message: "Das System kann auf die Datei nicht zugreifen." })
is_file: true
resolve: Some("C:\\Users\\<user>\\AppData\\Local\\Microsoft\\WindowsApps\\pwsh.exe")
launchable: true
spawn: Ok("PowerShell 7.7.0\r\n")
```

`Get-AuthenticodeSignature` (Status, SignatureType, timestamp present):

```
alias:                       error "Das System kann auf die Datei nicht zugreifen.", Status ""
package pwsh.exe (target):   Valid, Authenticode, timestamped
System32 powershell.exe:     Valid, Catalog, timestamped
python.exe (python.org):     Valid, Authenticode, timestamped
```

`icon_of_pwsh_if_present` failed with `called Option::unwrap() on a None value` (icon of the alias).

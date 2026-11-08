# Measurement: ConPTY blocks until ESC[6n is answered

- Date: 2026-11-07
- Platform: Windows 11 Pro 10.0.26300, x64
- Library: portable-pty 0.9.0 (ConPTY), Rust 1.99.1
- Test: `cargo test --manifest-path src-tauri/Cargo.toml --test pty`

## Observation (first run, without a reply)

```
test eingabe_kommt_beim_prozess_an ... FAILED
test echo_und_exit_code ... FAILED
test startgroesse_und_resize_erreichen_den_prozess ... FAILED
'hangar-input-ok' kam nicht; Output bisher: "\u{1b}[6n"
kein Exit-Ereignis innerhalb von 20s; Output bisher: "\u{1b}[6n"
```

The only output of every session was `ESC[6n` (Device Status Report, cursor position). No further
output arrived, not even that of `cmd /c echo ...`, and the process did not exit within
20 s.

## Cross-check (second run)

For every `ESC[6n` in the output, the test writes the reply `ESC[1;1R` into the PTY.

```
test fehler_werden_klar_gemeldet ... ok
test beenden_und_neustart ... ok
test echo_und_exit_code ... ok
test eingabe_kommt_beim_prozess_an ... ok
test startgroesse_und_resize_erreichen_den_prozess ... ok
test result: ok. 5 passed; 0 failed
```

## Unverified guess at the cause
portable-pty creates the pseudo console with the flag `PSEUDOCONSOLE_INHERIT_CURSOR`. ConPTY then queries
the host for the cursor position and waits for the reply. Not checked in the portable-pty source code.

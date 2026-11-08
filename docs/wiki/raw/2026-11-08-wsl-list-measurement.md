# Measurement: `wsl.exe --list --quiet` output and Windows Credential Manager through keyring

- Date: 2026-11-08
- Platform: Windows 11 Pro 10.0.26300, x64; WSL 2.7.14.0, kernel 6.18.33.2-2, WSLg 1.0.73.2
  (from `wsl.exe --version`); Git Bash as the calling shell; Rust 1.99.1.
- Installed distributions: Ubuntu, plus Docker Desktop's `docker-desktop`.

## `wsl.exe --list --quiet` into a pipe

Command: `wsl.exe --list --quiet | od -c`

```
0000000   U  \0   b  \0   u  \0   n  \0   t  \0   u  \0  \r  \0  \n  \0
0000020   d  \0   o  \0   c  \0   k  \0   e  \0   r  \0   -  \0   d  \0
0000040   e  \0   s  \0   k  \0   t  \0   o  \0   p  \0  \r  \0  \n  \0
0000060
```

UTF-16LE without byte order mark, CRLF line ends, exit code 0.

## The same with `WSL_UTF8=1`

Command: `WSL_UTF8=1 wsl.exe --list --quiet | od -c`

```
0000000   U   b   u   n   t   u  \r  \n   d   o   c   k   e   r   -   d
0000020   e   s   k   t   o   p  \r  \n
0000030
```

UTF-8, CRLF line ends.

## Detection through Hangar

`cargo test --lib -- --ignored wsl_on_this_machine --nocapture` (src-tauri/src/wsl.rs, wsl.exe from
`%SystemRoot%\System32`, `CREATE_NO_WINDOW`, Job Object, `WSL_UTF8` removed):

```
["wsl-ubuntu"] in 44.9289ms
```

## keyring 4.2.0 against Windows Credential Manager

`cargo test --lib -- --ignored keychain_round_trip` (src-tauri/src/secrets.rs, service
`hangar-tests`): set, read, overwrite, delete twice (the second delete reports no entry, mapped to
success), read after delete gives `NoEntry`, and a value of 1280 ASCII characters is stored and
read back unchanged. Result: `test secrets::tests::keychain_round_trip ... ok`.

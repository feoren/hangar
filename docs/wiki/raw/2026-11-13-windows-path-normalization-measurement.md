# Windows path normalization: trailing dots and spaces, 8.3 names

Date: 2026-11-13. Windows 11 Pro 10.0.26300, NTFS volume E:, Rust stable (`std::fs`).
Test: `cargo test --manifest-path src-tauri/Cargo.toml path_links` (test
`real_names_decide_what_opens` in `src-tauri/src/path_links.rs`), files under `tests/.tmp/`.

Setup: `tests/.tmp/path_links_real/marker.cmd` (content `rem marker`) and `marker.md`.

| Printed path | `fs::symlink_metadata` | `fs::canonicalize` |
|---|---|---|
| `...\marker.cmd.` | file | `\\?\E:\...\marker.cmd` |
| `...\marker.cmd ` (trailing space) | file | `\\?\E:\...\marker.cmd` |
| `...\marker.md` | file | `\\?\E:\...\marker.md` |

Result: `test path_links::tests::real_names_decide_what_opens ... ok`. The extension of the
printed name (`cmd.` gives no extension, `cmd ` gives `cmd `) differs from the file Windows opens;
the canonical name carries the real extension. Canonical paths of local drives start with
`\\?\<drive>:\`.

8.3 short names: `cmd /c "for %I in ("...\marker long name.cmd") do @echo %~sI"` printed the long
name unchanged, so volume E: creates no short names; the short name branch of the test was
skipped here and is not measured.

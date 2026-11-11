# Measurement: tauri-plugin-single-instance 2.5.5 lock tests on Windows

- Date: 2026-11-11
- Platform: Windows 11 Pro, `ver`: Microsoft Windows [Version 10.0.26300.9818]
- Toolchain: rustc 1.99.1 (58cf8f9c3 2026-10-13), cargo 1.99.1 (5f94df478 2026-08-27)
- Crate: tauri-plugin-single-instance 2.5.5 from crates.io (copied out of the Cargo registry into a
  scratch folder inside the project, removed afterwards), tauri 2.12 with the `test` feature
  (MockRuntime)

## What the tests do (crate file `tests/instance_lock.rs`)

The test process runs a primary instance with the plugin under a unique identifier and starts the
test binary again as a second instance with the same identifier.

- `destroy_releases_the_lock`: two second instances are forwarded to the primary; after
  `tauri_plugin_single_instance::destroy` a third one starts normally.
- `cleanup_before_exit_releases_the_lock`: forwarded while the lock is held; after
  `AppHandle::cleanup_before_exit` the next one starts normally.
- `lock_is_held_during_the_exit_event`: a second instance launched from inside the
  `RunEvent::Exit` handler is forwarded (the lock is still held); after `App::run` returns, the
  next one starts normally.

## Command

```
cargo test --test instance_lock
```

## Output (filtered to test lines)

```
test second_instance ... ignored, launched by the other tests
test cleanup_before_exit_releases_the_lock ... ok
test lock_is_held_during_the_exit_event ... ok
test destroy_releases_the_lock ... ok
test result: ok. 3 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.09s
```

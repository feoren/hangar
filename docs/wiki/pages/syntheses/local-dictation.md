---
title: Local dictation
type: synthesis
tags: [dictation, audio, whisper, privacy, shortcuts]
sources: ["[[local-dictation-research]]", "[[dictation-measurement]]"]
created: 2026-11-15
updated: 2026-11-15
---

# Local dictation

Hold a key, speak, let go: the text lands in the prompt of the session that was active on key
down, without Enter. Recognition runs on this machine with whisper.cpp; the only network use is
the model download the user agreed to. Choice of engine and models: [[local-dictation-research]],
numbers: [[dictation-measurement]].

Code: `src-tauri/src/dictation/` (`mod.rs` service and states, `audio.rs` capture and audio math,
`model.rs` models and download, `whisper.rs` recognition), commands in `src-tauri/src/commands.rs`,
`src/dictation.ts` (types and checks), `src/useDictation.ts` (events and the hold key),
`src/DictationIndicator.tsx`, `src/DictationSettings.tsx`, `isDictationChord` and `KeyHold` in
`src/shortcuts.ts`.

## Behavior

- Off by default. Settings > Dictation turns it on, picks the model and the language
  (automatic, German, English). These choices live in the window's localStorage with the other
  settings, not with the protected settings: rewriting them could only claim the key, while the
  microphone and the download stay behind main-window commands and the consent box.
- **Key:** hold Ctrl+Shift+Space (`DICTATION_SHORTCUT`). Ctrl+Space alone stays with the shell
  (Readline set-mark) and the IME. While dictation is on, the terminal leaves the chord to the app
  (`terminalKeyAction`, all of keydown, keypress and keyup), so the program never sees it. The hold
  starts once despite key repeat and ends on the release of Ctrl, Shift or Space, or when the
  window loses the focus. A dialog open: the key does nothing. No model installed: the key opens
  the settings section.
- **Target:** the session active on key down, checked by the backend (`UnknownSession`). The text
  goes through the terminal's usual paste path (`pasteText`: control characters stripped,
  bracketed paste when the program asked for it), never with Enter. One trailing space follows
  (`pasteForm`), so the next hold continues the sentence instead of gluing onto its last word.
- **Indicator:** a pill at the top of that session's pane: red dot and input level while the
  microphone is open, a spinner while transcribing, a short notice (4 s) when the microphone is
  missing, blocked or recognition failed. Reduced motion stops the animations.
- **Models:** `base` q5_1 (60 MB, default) and `small` q5_1 (190 MB), pinned to one Hugging Face
  revision with size and SHA-256 (`model.rs`). Download only after a consent box that names model,
  size and host; into `<app local data>/models/`, written to `<file>.part` and renamed only after
  size and hash match; a body larger than the pinned size stops at once. Redirects only to https
  hosts of Hugging Face, at most 5. Remove deletes the file and a leftover `.part`.
- **Installed** means the file exists with the pinned size; the hash is checked once on download.

## States

`Phase` in `mod.rs`, shown as `DictationState` (Rust and `src/dictation.ts` mirror each other):

| Phase | Events out |
| --- | --- |
| Idle (Ready or NotInstalled) | Install, Record, Removed |
| Downloading { model, progress } | Progress, Installed, InstallFailed |
| Recording { session } | Stop, Discard, CaptureFailed |
| Transcribing { session } | Transcribed, Discard, Blocked, TranscribeFailed |
| NoMicrophone, PermissionDenied, Failed | as Idle (a retry is allowed) |

Busy states refuse a new recording, a download and a removal (`Busy`). A stop that arrives while
the microphone is still opening is remembered and runs once it is open. The window turns a move
from a recording to a failure state into the pane notice (`failureAfter`).

## Audio and privacy

- cpal opens the default input device on its own thread (streams are not `Send` everywhere);
  samples stay in memory (`Zeroizing`), capped at 60 s, never logged or written.
- Downmix to mono, box low-pass and linear resampling to 16 kHz.
- Energy gate: at least 200 ms of 20 ms frames above about -40 dBFS, otherwise nothing goes to
  whisper (it invents text for silence).
- No data at all, or only exact zeros, for half a second or more counts as a blocked microphone
  (`PermissionDenied`). A shorter recording is never judged: a quick tap often ends before the
  device's first buffer (`Recording::elapsed`).
- The transcript becomes one line: whitespace collapsed, control, bidi and zero-width characters
  removed, whisper's `[BLANK_AUDIO]`-style tags dropped. It is sent to the window once and not kept.
- whisper.cpp loads the model per transcription and drops it afterwards (load 0.1 to 0.3 s against
  250 to 500 MB kept in memory); threads: half the logical cores, at most 8.

## Interface

| Command | Purpose |
| --- | --- |
| `dictation_status(model)` | state for the chosen model, installed models, source host |
| `dictation_start(id, model, language)` | open the microphone for session `id` |
| `dictation_stop()` | close it and transcribe; harmless when nothing records |
| `dictation_install_model(model)` | download after consent |
| `dictation_remove_model(model)` | delete the model file |

Errors are stable names (`busy`, `notInstalled`, `unknownSession`, `noMicrophone`,
`permissionDenied`, `microphone`, `download`, `storage`); the window shows its own text. Events to
the main window only: `dictation-status`, `dictation-level` (ten times a second while recording),
`dictation-text` (`{ session, text }`). All commands are allowed for the main window alone
(`capabilities/default.json`, `tests/contract.rs`).

## Build

whisper.cpp builds from source: CMake, a C++ compiler and libclang (bindgen); on Linux the ALSA
headers for cpal. `.cargo/config.toml` sets `GGML_NATIVE=OFF`, `GGML_AVX2=ON` and the
`CMAKE_PROJECT_INCLUDE` that restores MSVC's optimization flags ([[dictation-measurement]]). CI
installs these on all three runners (`.github/workflows/ci.yml`).

## Platforms

- Windows 11: built and unit-tested. The live microphone path is not yet checked by hand.
- macOS: `src-tauri/Info.plist` carries `NSMicrophoneUsageDescription`, which the Tauri bundler
  merges into the app's Info.plist.

> [!question] Unverified
> macOS with hardened runtime may also need the `com.apple.security.device.audio-input`
> entitlement; to be settled when the macOS bundle is set up. Whether Windows hands out exact zeros
> when the microphone privacy switch blocks desktop apps is unverified; the zero check covers that
> case if it does. Ctrl+Shift+Space against Japanese and Chinese IMEs is untested.

- Linux: cpal through ALSA; not run yet.

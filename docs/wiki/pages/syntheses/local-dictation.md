---
title: Local dictation
type: synthesis
tags: [dictation, audio, whisper, privacy, shortcuts]
sources: ["[[local-dictation-research]]", "[[dictation-measurement]]"]
created: 2026-11-15
updated: 2026-11-21
---

# Local dictation

Hold a key, speak, let go: the text lands in the prompt of the session that was active on key
down, without Enter. Recognition runs on this machine with whisper.cpp; the only network use is
the model download the user agreed to. Choice of engine and models: [[local-dictation-research]],
numbers: [[dictation-measurement]].

Code: `src-tauri/src/dictation/` (`mod.rs` service and states, `audio.rs` capture and audio math,
`model.rs` models and download, `whisper.rs` recognition), commands in `src-tauri/src/commands.rs`,
`src/dictation.ts` (types and checks), `src/useDictation.ts` (events and the hold key),
`src/DictationIndicator.tsx`, `src/DictationSettings.tsx`, `DictationKey`, `startsDictation`,
`isDictationKeyEvent` and `KeyHold` in `src/shortcuts.ts`.

## Behavior

- Off by default. Settings > Dictation turns it on, picks the key, the model and the language
  (automatic, German, English). Whether the key is on lives with the backend's main-window
  preferences (`dictationEnabled` in `MainPrefs`, `main_store.rs`), not in localStorage: the panel
  and viewer windows share that storage, and one that turned the key on would make the next
  press of the key open the microphone instead of reaching the program. A Hangar that kept the
  switch in localStorage hands it over once (`legacyDictationEnabled`, `loadMainPrefs`; a
  `main-prefs.json` from before dictation lacks the field and reads as `None`); afterwards the old
  copy is removed and never read again. Model, language and key stay in localStorage: rewriting them
  could only pick the other pinned model, language or key, every key records only while it is
  held, and a download still needs the consent box. The key is parsed like the rest
  (`parseDictationPrefs`): a stored value this Hangar does not know, or none (settings from before
  the choice), reads as right Ctrl.
- **No microphone:** `dictation_status` asks the audio system for its default input device each
  time the window reads the status, never on a state change alone. Without one, the settings
  section says so ("No microphone found. Connect one, then you can dictate here.") in place of the
  download buttons, so nobody downloads a model they cannot use.
- **Key:** held, never pressed. Settings > Dictation offers right Ctrl (default, also for
  settings stored before the choice), F8, F9, F10 and Ctrl+Shift+Space (`DictationKey`,
  `DICTATION_KEYS`). On the Mac the default is Ctrl+Shift+Space (`defaultDictationKey`): most Mac
  keyboards have no right control key. Every key: the hold starts once despite key repeat and ends on the release of
  the key, or when the window loses the focus. A dialog open: the key does nothing. No model
  installed: letting the key go opens the settings section, unless a shortcut or a lost focus ended
  the hold (`useDictationKey`). The window listens in the capture phase
  (`useDictationKey`), so it also sees keys the terminal handles and stops.
  - **Right Ctrl** (`event.code === "ControlRight"`, no Shift, Alt or Meta). A modifier alone sends
    the program nothing, so the terminal keeps every event of it (`isDictationKeyEvent` is false)
    and shortcuts typed with it work as before. AltGr on Windows arrives as `ControlLeft` plus
    `AltRight` and never matches. The recording starts only after `CONTROL_HOLD_DELAY_MS` (300 ms)
    with no other key, so a tap, or Ctrl+C typed with the right Ctrl, never opens the microphone.
    Another key, a mouse button or the wheel while it is down makes it that shortcut (`KeyHold`
    phase `spoiled`): before the start nothing happens, after it the recording is discarded. The
    backend has no command to cancel a recording, so the window stops it with `dictation_stop` and
    drops what comes out (`DiscardGate` in `src/dictation.ts`): the backend signals a text before
    the state change that ends the transcription, so a `dictation-text` signal that arrives while
    a hold is marked belongs to that hold. The mark goes when the state leaves recording and
    transcribing, when the hold's start failed, or when the backend accepted a later hold. A pane
    notice for a failure of a discarded recording is not shown either.
  - **F8, F9, F10** without a modifier. While dictation is on, the terminal leaves the chosen key
    to the app (`terminalKeyAction`, all of keydown, keypress and keyup), so the program never sees
    it; with a modifier (Shift+F9) or with dictation off it goes to the program as before.
  - **Ctrl+Shift+Space** (`isDictationChord`). Ctrl+Space alone stays with the shell (Readline
    set-mark) and the IME. The terminal leaves the chord to the app like an F key, and the hold
    also ends on the release of Ctrl or Shift. Released Shift before Space, the held Space keeps
    repeating as Ctrl+Space (NUL for the program) or as spaces; the terminal swallows repeated
    Space events from the chord until Space goes up (`dictationTail` in `src/terminalKeys.ts`).
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
- **Installed** means the file exists with the pinned size. The SHA-256 is checked on download and
  again once per run before the first load (`VerifiedModels` in `model.rs`): size and modification
  time of the last match stand for "unchanged" afterwards, so the hash runs once per run and file,
  not per recording. A file that no longer matches never reaches whisper; it is removed, the
  recording ends as an error, and the settings offer the download again.

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
the microphone is still opening is remembered and runs once it is open. A recording whose stop
never comes (the window reloaded or crashed while the key was held) ends on its own after
`RECORDING_DEADLINE` (the 60 s cap plus 5 s) and keeps what was said; each recording carries a
generation, so an old deadline never ends a later one. When the main window loads, it first
drops the texts that finished while no window listened (otherwise the next dictation would paste
them along) and then sends `dictation_stop` once, so the text of the recording that stop ends
still arrives (`endStaleRecording`). The window turns a move
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
  removed, whisper's `[BLANK_AUDIO]`-style tags dropped. It waits in a queue of at most 8
  (`MAX_WAITING_TEXTS`, oldest dropped first) until the main window takes it, once
  (`dictation_take_text`), and is not kept after that.
- whisper.cpp loads the model per transcription and drops it afterwards (load 0.1 to 0.3 s against
  250 to 500 MB kept in memory); threads: half the logical cores, at most 8.

## Interface

| Command | Purpose |
| --- | --- |
| `dictation_status(model)` | state for the chosen model, installed models, source host, `microphone` |
| `dictation_start(id, model, language)` | open the microphone for session `id` |
| `dictation_stop()` | close it and transcribe; harmless when nothing records |
| `dictation_take_text()` | the waiting transcripts `[{ session, text }]`, each handed out once |
| `dictation_install_model(model)` | download after consent |
| `dictation_remove_model(model)` | delete the model file |

Errors are stable names (`wrongWindow`, `busy`, `notInstalled`, `unknownSession`, `noMicrophone`,
`permissionDenied`, `microphone`, `download`, `storage`); the window shows its own text. All
commands are allowed for the main window alone (`capabilities/default.json`) and check the caller
themselves (`dictation_main` in `commands.rs`); `tests/contract.rs` holds both.

Events are emitted to the main window, but Tauri 2.12 hands such an event to every listener with
target Any, and the panel window may listen ([[process-launch-hardening]]). The rule, as for the
gate, the claims and the update results: no payload the panel must not read.

| Event | Payload | The main window then |
| --- | --- | --- |
| `dictation-status` | none | reads `dictation_status(model)`; only the newest answer counts (`LatestOnly`) |
| `dictation-text` | none | reads `dictation_take_text()` and pastes each text |
| `dictation-level` | one number, 0 to 1 | draws the meter from the payload |

The level stays a payload, deliberately. It comes ten times a second while the microphone is open;
as a bare signal each would become a command call, ten invokes a second for a meter. The number
tells a listener only how loud the room is at that moment and that a recording runs: no text, no
session, no file. `Notice::Level(f32)` keeps it a number, and the contract test checks that type.

## Build

whisper.cpp builds from source: CMake, a C++ compiler and libclang (bindgen); on Linux the ALSA
headers for cpal. `.cargo/config.toml` sets `GGML_NATIVE=OFF`, `GGML_AVX2=ON` and the
`CMAKE_PROJECT_INCLUDE` that restores MSVC's optimization flags ([[dictation-measurement]]). CI
installs these on all three runners (`.github/workflows/ci.yml`).

Licenses of what ends up in the program: whisper.cpp and the ggml tree inside it (MIT, compiled
and linked statically, so their notice ships with Hangar), whisper-rs and whisper-rs-sys
(Unlicense), cpal (Apache-2.0, no NOTICE file in the crate). Texts in `THIRD_PARTY_NOTICES.md`.

## Platforms

- Windows 11: built and unit-tested. The live microphone path is not yet checked by hand.
- macOS: `src-tauri/Info.plist` carries `NSMicrophoneUsageDescription`. Tauri looks for an
  `Info.plist` next to `tauri.conf.json` and merges it into the bundle's (doc comment of
  `MacConfig::info_plist` in tauri-utils 2.10.2; tauri-build 2.7.4 watches the file). Signing
  uses the hardened runtime by default (`hardened_runtime`, same file), which opens the microphone
  only with the `com.apple.security.device.audio-input` entitlement: `src-tauri/Entitlements.plist`,
  referenced as `bundle.macOS.entitlements` (`tests/contract.rs` checks both files).

> [!question] Unverified
> No macOS bundle has been built and run yet, so neither the merged Info.plist nor the signed
> entitlement is checked on a Mac. Whether Windows hands out exact zeros
> when the microphone privacy switch blocks desktop apps is unverified; the zero check covers that
> case if it does. Ctrl+Shift+Space against Japanese and Chinese IMEs is untested. That AltGr
> arrives as `ControlLeft` plus `AltRight` in WebView2 follows from Windows sending a left Ctrl
> before the right Alt for AltGr layouts; it is covered by a unit test of the event shape, not
> measured in the app. Right Ctrl on macOS (no right Control on many Mac keyboards) and on Linux
> desktops that bind it (some use it as the Compose key) is untested.

- Linux: cpal through ALSA; not run yet.

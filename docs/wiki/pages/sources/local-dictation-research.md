---
title: "Research: local dictation (hold a key, speak, text lands in the prompt)"
type: source
tags: [research, dictation, audio, privacy]
sources: []
raw: raw/2026-11-12-local-dictation-research.md
url: https://github.com/ggml-org/whisper.cpp
retrieved: 2026-11-12
created: 2026-11-12
updated: 2026-11-12
---

# Research: local dictation (2026-11)

Push-to-talk dictation into the active session's prompt, recognition strictly on the device.
Evidence and links: `raw/2026-11-12-local-dictation-research.md`. Items marked `[UNVERIFIED]` were
not confirmed from a source and must be measured.

## Recommendation
- Engine: whisper.cpp through the `whisper-rs` crate (0.16.0, Unlicense; whisper.cpp and the models MIT).
- Models: `ggml-small-q5_1.bin` (190 MB) as default, `ggml-base-q5_1.bin` (60 MB) as "lite". Both
  multilingual, German and English.
- Capture: `cpal` (Apache-2.0) in the Rust backend. No `getUserMedia` in the WebView.
- Mode: push-to-talk without streaming. Record while the key is held, transcribe once on release,
  write the text into the session without Enter.
- Key: hold Ctrl+Shift+Space (configurable). Never Ctrl+letter alone.
- Models are downloaded on demand after an explicit consent that names size and source.

## Options compared
| Option | License | Size | Offline | German | Verdict |
| --- | --- | --- | --- | --- | --- |
| whisper.cpp via whisper-rs, small q5_1 | MIT / Unlicense | 190 MB | yes | good `[UNVERIFIED]` | recommended |
| whisper.cpp, base q5_1 | MIT / Unlicense | 60 MB | yes | usable, weaker `[UNVERIFIED]` | lite option |
| whisper.cpp, tiny q5_1 | same | 32 MB | yes | poor `[UNVERIFIED]` | not offered |
| Vosk (`vosk` 0.3.1 crate, MIT badge) | Apache-2.0 models | small de 45 MB, big de 1.9 GB | yes | small model WER 13.75 (Tuda-de) | second choice, no punctuation, weak in free speech |
| sherpa-onnx (Rust API) | Apache-2.0 | model dependent | yes | no German ASR model listed on its README | watch, revisit |
| Windows.Media.SpeechRecognition dictation | OS | none | no, web service | yes | rejected |
| macOS SFSpeechRecognizer on device | OS | system | yes if `supportsOnDeviceRecognition` | locale dependent `[UNVERIFIED]` | optional macOS backend later |
| Web Speech API in the WebView | n/a | none | cloud or absent | n/a | rejected |
| Linux OS speech API | none | none | none | none | whisper only |

## Why whisper.cpp
- One engine and one model format on all three platforms, same behavior and same tests.
- Multilingual models with punctuation and casing, which matters for prompts.
- Quantized models cut disk and RAM; the full ggml sizes are tiny 75 MiB, base 142 MiB, small
  466 MiB, medium 1.5 GiB (RAM roughly 273 MB, 388 MB, 852 MB, 2.1 GB).
- Silero VAD is supported by whisper.cpp and can trim silence.
- Costs: C++ build (cmake, compiler) in CI on three platforms `[UNVERIFIED]`; the `whisper-rs`
  GitHub repo is archived (since 2025-07-30), development continues on Codeberg. Mitigation: pin the
  version, keep the engine behind a trait so it can be swapped for the Codeberg fork or a sidecar
  `whisper-cli`.

## Why not the others
- Windows dictation grammar: Microsoft documents it as a remote web service, needs a network and the
  "Online speech recognition" privacy setting, and is limited to short phrases (about 10 seconds).
  Offline works only for custom grammars (no free text).
- Web Speech API: not enabled for third-party WKWebView embedders; no Microsoft statement for
  WebView2; reports of "Network Error" in Chromium-based Edge suggest a missing cloud backend. Even
  where it works it is normally cloud, which breaks the requirement.
- Vosk: Apache-2.0 models are fine, but the small German model is clearly weaker, the big one is
  1.9 GB, and there is no punctuation.

## Audio path
1. Key down in the frontend (keydown without `repeat`) calls the backend command `dictation_start`
   (session ID).
2. Backend: `cpal` input stream, default device, capture in the device's native format, downmix and
   resample to 16 kHz mono f32 off the audio callback. Ring buffer with a hard cap (60 s).
3. Key up (or window blur, or a second key press) calls `dictation_stop`. Backend runs `whisper-rs`
   on a worker thread (never the UI or IPC thread), with a fixed language from the setting (de or en)
   or auto detect as an option.
4. Result text is trimmed, control characters removed, no trailing newline, then sent to the PTY like
   a paste (bracketed paste when the app enabled it). The user sees the text in the prompt and presses
   Enter themselves. The result is bound to the session ID from key down.
5. The audio buffer is zeroed and dropped. Nothing is written to disk.

## UI states (explicit enum)
`NotInstalled` -> `Downloading(progress)` -> `Ready` -> `Recording` -> `Transcribing` -> `Ready`;
side states `NoMicrophone`, `PermissionDenied`, `Error(message)`. Indicators: recording dot on the
session row and a small level meter; "Transcribing" spinner. Texts in `src/i18n.ts`, 12 languages.

## Model download and storage
- Settings page "Dictation": toggle, model choice with size, language, key.
- First use shows a dialog: model name, size, source host, "audio never leaves this computer",
  buttons Download / Cancel.
- Download from the Hugging Face `ggerganov/whisper.cpp` repo (MIT), pinned file name and SHA-256
  compiled into the app. Write to a `.part` file, verify the hash, then rename.
- Store in the app data directory under `models/`. Provide a delete button.
- Use the existing `reqwest` (rustls); the download is the only network access of the feature.

## Keyboard
- Default hold key: Ctrl+Shift+Space, matched in `src/shortcuts.ts` with `keydown` and `keyup`
  (new pair of actions; today only keydown matching exists). Shells and agent CLIs do not bind it
  `[UNVERIFIED for every agent CLI]`.
- End recording on keyup of Space, on release of Ctrl or Shift, and on window blur, so a lost keyup
  never leaves the microphone open.
- Also offer a mic button and a toggle mode. No global hotkey.
- Windows IMEs use Ctrl+Space; Ctrl+Shift+Space must be tested with a Japanese and Chinese IME
  `[UNVERIFIED]`.

## Platform notes
- Windows: desktop apps depend on the "Let desktop apps access your microphone" privacy switch; no
  prompt `[UNVERIFIED]`. Detect an empty stream and show `PermissionDenied`.
- macOS: `NSMicrophoneUsageDescription` in `Info.plist`; hardened runtime needs the audio-input
  entitlement `[UNVERIFIED]`.
- Linux: ALSA headers needed to build `cpal` (`libasound2-dev`).

## Privacy
- Audio stays in memory, is never logged, written or sent. The only network use is the model
  download, after consent.
- Visible recording indicator while the microphone is open; stop on blur and after the cap.
- Transcripts are not stored by Hangar; they go into the PTY only.
- Silence produces typical Whisper hallucinations; gate on an energy threshold or VAD.

## Risks
- Build time and CI complexity from C++ on all three platforms; binary growth `[UNVERIFIED]`.
- Archived upstream of `whisper-rs`; Unlicense is unusual but permissive.
- German quality of small versus base in technical speech.
- Latency on CPUs without AVX2 or on weak laptops.
- Text in the wrong session if the user switches sessions while transcribing.

## What must be measured
- Real-time factor and latency for 3 s, 10 s, 30 s of speech: base-q5_1 and small-q5_1 on a mid
  laptop CPU, no GPU.
- Error counts on 30 German and 30 English prompts with technical terms.
- Peak RAM, installer size effect, cold model load.
- `cpal` with Bluetooth headsets and a missing device.
- Probe build of `whisper-rs` on Windows, macOS and Linux in CI.
- Ctrl+Shift+Space against Claude Code, Codex, Gemini CLI, Aider, PSReadLine and IMEs.

## Build plan
1. Spike in a worktree: `dictation` module, `cpal` capture, `whisper-rs` transcription of fixture
   audio. Gate on the measurements above.
2. Traits `Transcriber` and `AudioSource`; state enum with tested transitions.
3. Commands `dictation_status`, `dictation_start`, `dictation_stop`, `dictation_install_model`,
   `dictation_remove_model` in the capabilities file.
4. Frontend: hold shortcut, indicator, settings page, consent dialog, 12 languages.
5. Dependencies with justification: `whisper-rs` (Unlicense), `cpal` (Apache-2.0), a resampler such
   as `rubato` (MIT) `[UNVERIFIED version]`.
6. Later: optional macOS on-device backend, GPU features, VAD.

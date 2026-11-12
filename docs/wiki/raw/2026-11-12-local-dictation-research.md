# Raw notes: local dictation research (retrieved 2026-11-12)

`[UNVERIFIED]` = not confirmed from a source. Web search results are secondary; primary pages were
fetched where named.

## whisper.cpp
- Source: https://github.com/ggml-org/whisper.cpp (README). License MIT.
- ggml sizes: tiny 75 MiB (~273 MB RAM), base 142 MiB (~388 MB), small 466 MiB (~852 MB), medium
  1.5 GiB (~2.1 GB).
- Platforms: macOS (Intel, Arm), Linux, Windows (MSVC, MinGW). Integer quantization supported. VAD
  via Silero (`--vad`).
- Model files: https://huggingface.co/ggerganov/whisper.cpp/tree/main, license MIT.
  - tiny 77.7 MB, q5_1 32.2 MB, q8_0 43.5 MB
  - base 148 MB, q5_1 59.7 MB, q8_0 81.8 MB
  - small 488 MB, q5_1 190 MB, q8_0 264 MB
  - medium 1.53 GB, q5_0 539 MB, q8_0 823 MB
  - large-v3-turbo 1.62 GB, q5_0 574 MB, q8_0 874 MB
- OpenAI Whisper: https://github.com/openai/whisper, MIT for code and weights. Parameters tiny 39 M,
  base 74 M, small 244 M, medium 769 M. No German WER for tiny, base, small found; only fine-tuned
  large-v3 numbers, which do not transfer.

## whisper-rs
- https://docs.rs/crate/whisper-rs/latest: 0.16.0, released 2026-03-12, Unlicense. Optional features:
  cuda, hipblas, openblas, metal, vulkan, log_backend, tracing_backend, raw-api. Depends on
  `whisper-rs-sys` ^0.15.
- Codeberg repo https://codeberg.org/tazz4843/whisper-rs: last commit 2026-03-12. GitHub repo archived
  2025-07-30 (search result summary).
- Build: whisper.cpp as a git submodule; Linux "should just work"; Windows and macOS need extra steps.
  cmake, compiler and libclang requirements not stated `[UNVERIFIED]`.

## Vosk
- https://alphacephei.com/vosk/models
  - en small 0.15: 40 M, WER 9.85 (LibriSpeech clean), Apache-2.0
  - de 0.21: 1.9 G, Tuda-de 9.83, CV 12.82, podcast 24.00; small-de-0.15 45 M, Tuda-de 13.75,
    podcast 30.67
- Crates `vosk` 0.3.1 and `vosk-sys` 0.2.0, MIT per README badges, license file not checked.

## sherpa-onnx
- https://github.com/k2-fsa/sherpa-onnx: Apache-2.0, Rust API examples, models Whisper, Parakeet
  (English), Moonshine (English), SenseVoice (zh, yue, en, ko, ja). No German ASR on the README.

## OS speech
- Windows: https://learn.microsoft.com/en-us/windows/uwp/ui-input/speech-recognition. Predefined
  dictation and web search grammars are performed by a remote web service, need network and the
  "Online speech recognition" privacy setting; up to 10 seconds. Custom grammars work on device but
  are not free dictation. SAPI offline recognizers not researched `[UNVERIFIED]`.
- macOS: the Apple page for `supportsOnDeviceRecognition` could not be read; macOS 10.15+, per locale
  `[UNVERIFIED]`.
- Linux: no OS speech API.

## Web Speech API
- WebKit bug 239816 and Apple forum threads: not enabled for third-party WKWebView.
- Microsoft Edge docs: on-device model, few languages, Canary or Dev only. Nothing official for
  WebView2 `[UNVERIFIED]`.

## Microphone capture
- cpal (https://github.com/RustAudio/cpal): Apache-2.0; WASAPI, CoreAudio, ALSA (JACK, PipeWire,
  PulseAudio optional); needs `libasound2-dev` on Linux.
- Tauri: no official Tauri 2 documentation on `getUserMedia` found; macOS needs
  `NSMicrophoneUsageDescription`.

## Repo facts
- `src/shortcuts.ts`: Ctrl+Shift+Space is unused; only keydown matching exists.
- `src-tauri/Cargo.toml` has `reqwest` 0.13 (rustls) and `tokio`; no `cpal` or `whisper-rs`.

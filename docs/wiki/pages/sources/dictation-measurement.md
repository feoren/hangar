---
title: "Measurement: whisper.cpp through whisper-rs on Windows"
type: source
tags: [measurement, dictation, whisper, build, windows]
sources: []
raw: raw/2026-11-12-dictation-measurement.md
url: https://github.com/ggml-org/whisper.cpp
retrieved: 2026-11-12
created: 2026-11-15
updated: 2026-11-15
---

# Measurement: whisper.cpp through whisper-rs (2026-11-12/13)

Own measurement on Windows 11 (Ryzen 9 9900X, 12 cores, no GPU, machine under load from other
builds). whisper-rs 0.16.0, whisper-rs-sys 0.15.0, bundled whisper.cpp 1.8.3, cmake crate 0.1.58,
bindgen 0.72.1. Synthetic speech (System.Speech voices), single runs. Raw record:
`raw/2026-11-12-dictation-measurement.md`.

## Key points

- **Models** are pinned to Hugging Face revision `5359861c739e955e79d9a303bcbc70fb988958b1`:
  `ggml-base-q5_1.bin` 59 707 625 bytes, `ggml-small-q5_1.bin` 190 085 487 bytes; SHA-256 computed
  locally and equal to the LFS oid of that revision.
- **Build needs libclang** (bindgen) on Windows: whisper-rs-sys's bundled bindings were generated
  against glibc and fail with `WHISPER_DONT_GENERATE_BINDINGS=1` (three E0080 size checks).
- **CMake** must be on `PATH` or in `CMAKE`; the copy inside the Visual Studio Build Tools works
  with the default generator, no developer prompt needed.
- **The cmake crate drops MSVC's `/O` flags** for the Visual Studio generator, so whisper.cpp built
  unoptimized: 17 144 ms for 2.9 s of speech. `CMAKE_PROJECT_INCLUDE` with a file that appends
  ` /O2 /Ob2 /DNDEBUG` restores them: 714 ms for the same clip.
- **CPU baseline:** `GGML_NATIVE=OFF` plus `GGML_AVX2=ON` gives AVX, AVX2, BMI2, SSE4.2, F16C, FMA
  and no AVX-512 (`/arch:AVX2`).
- whisper-rs-sys declares no `rerun-if-env-changed` for these variables: a stale build folder has
  to be removed by hand after changing them.
- Binary growth about 1.2 MB (spike binary 1 365 504 bytes); clean C++ build about 20 s.

## Speed (optimized build, 8 threads)

| Model | 3 s clip | 10 s clip | 30 s clip | Peak RAM |
| --- | --- | --- | --- | --- |
| base q5_1 | 0.46 s | 0.54 s | 1.1 s | 257 to 298 MB |
| small q5_1 | 1.5 to 1.9 s | 1.6 to 1.7 s | 1.9 to 2.1 s | about 496 MB |

8 threads take about 35 % less time than 4. Short clips cost almost as much as long ones (whisper
encodes a padded 30 s window). Model load from the OS cache: base about 60 ms, small 150 to 260 ms.

## Accuracy (hand-scored word errors)

| | base | small |
| --- | --- | --- |
| German (83 words) | 36 % | 29 % |
| English (89 words) | 8 % | 4 % |

Almost all German errors are English technical terms inside German sentences (`npm install`,
`Claude Code`, file names); plain German words come out right. An initial prompt with agent and
tool names fixed some names, broke others and once made small take 12.6 s: not used.

## Significance for Hangar

- `base` is the default model (fast enough to feel instant), `small` the optional one.
- `.cargo/config.toml` carries the CPU baseline and the `CMAKE_PROJECT_INCLUDE` fix, so release
  builds are optimized and run on any x86-64 CPU with AVX2.
- Contributors and CI need CMake, a C++ compiler and libclang ([[local-dictation]]).

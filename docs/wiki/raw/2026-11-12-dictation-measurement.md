# Measurement: whisper.cpp through whisper-rs on Windows (2026-11-12/13)

Measured for the local dictation feature. Raw record, do not edit.

## Machine and versions

- Windows 11 Pro 10.0.26300, AMD Ryzen 9 9900X (12 cores, 24 threads), 62 GB RAM, no GPU used.
- The machine was not idle: other agents built Rust code and several WebView2 processes ran at the
  same time. Numbers are single runs, not medians.
- Rust 1.99.1 (MSVC target), Visual Studio 2022 Build Tools, CMake 3.31.6-msvc6 (bundled with the
  Build Tools, not on PATH), LLVM 23.1.5 (`winget install LLVM.LLVM`) for libclang.
- whisper-rs 0.16.0, whisper-rs-sys 0.15.0, bundled whisper.cpp 1.8.3, cmake crate 0.1.58,
  bindgen 0.72.1.
- Models from `https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/<file>`
  (repository revision `5359861c739e955e79d9a303bcbc70fb988958b1`, MIT). SHA-256 computed locally and
  equal to the LFS oid the Hugging Face tree API reports for that revision:

| File | Bytes | SHA-256 |
| --- | --- | --- |
| `ggml-base-q5_1.bin` | 59 707 625 | `422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898` |
| `ggml-small-q5_1.bin` | 190 085 487 | `ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb` |

## Build findings

1. Without libclang the build stops in bindgen:
   `Unable to find libclang: "couldn't find any valid shared libraries matching: ['clang.dll', 'libclang.dll'], set the LIBCLANG_PATH environment variable to a path where one of these files can be found (invalid: [])"`
2. `WHISPER_DONT_GENERATE_BINDINGS=1` makes whisper-rs-sys copy its bundled `src/bindings.rs`. That
   file was generated against glibc and fails on Windows:
   `attempt to compute 12_usize - 16_usize, which would overflow` (`Size of _G_fpos_t`,
   `_G_fpos64_t`) and `208_usize - 216_usize` (`Size of _IO_FILE`), 3x E0080. libclang is therefore
   required on Windows.
3. CMake must be found as `cmake` (PATH) or through the `CMAKE` variable. The Build Tools copy at
   `...\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin` works with the default
   Visual Studio 17 2022 generator, no developer prompt needed.
4. The cmake crate sets `CMAKE_C_FLAGS_RELEASE` and `CMAKE_CXX_FLAGS_RELEASE` itself for an MSVC
   target with the Visual Studio generator and strips every `/O` flag. CMakeCache.txt showed
   `CMAKE_C_FLAGS_RELEASE:STRING= -nologo -MD -Brepro -W0`. Result: whisper.cpp compiled without
   optimization. Measured with base-q5_1, 4 threads: 17 144 ms for 2.9 s of German speech,
   18 134 ms for 10.6 s, 39 177 ms for 27.8 s (real-time factor 5.9, 1.7, 1.4).
5. Fix used: `CMAKE_PROJECT_INCLUDE` pointing to a CMake file that appends ` /O2 /Ob2 /DNDEBUG` to
   both release flag variables when `MSVC`. whisper-rs-sys forwards every `CMAKE_*`, `GGML_*` and
   `WHISPER_*` environment variable as a CMake define. After the fix the generated
   `ggml-cpu.vcxproj` has `<Optimization>MaxSpeed</Optimization>` for `Release|x64` and the same run
   takes 714 ms instead of 17 144 ms.
6. CPU baseline: with `GGML_NATIVE=OFF` and `GGML_AVX2=ON` the cache shows `GGML_AVX=ON`,
   `GGML_AVX2=ON`, `GGML_BMI2=ON`, `GGML_SSE42=ON`, `GGML_AVX512=OFF`; the vcxproj uses
   `/arch:AVX2` (`AdvancedVectorExtensions2`). whisper's system info at run time:
   `SSE3 = 1 | SSSE3 = 1 | AVX = 1 | AVX2 = 1 | F16C = 1 | FMA = 1 | BMI2 = 1 | REPACK = 1`.
7. The whisper-rs-sys build script declares no `rerun-if-env-changed` for these variables: changing
   them does not rebuild whisper.cpp. A stale build directory has to be removed by hand.
8. Clean C++ build of whisper.cpp: about 20 s on this machine. The spike binary (whisper.cpp plus a
   WAV reader) is 1 365 504 bytes; the static libraries are whisper 1.8 MB, ggml-cpu 2.1 MB,
   ggml-base 2.0 MB, ggml 0.2 MB.

## Test speech

Generated offline with System.Speech (`SpeechSynthesizer`, `SetOutputToWaveFile` with
`SpeechAudioFormatInfo(16000, Sixteen, Mono)`), voices "Microsoft Hedda Desktop" (de-DE) and
"Microsoft Zira Desktop" (en-US). Durations computed from the PCM size.

| Clip | Seconds | Reference text |
| --- | --- | --- |
| de-03 | 2.92 | Führe bitte npm install aus. |
| de-10 | 10.55 | Öffne die Datei package.json und prüfe die Version. Danach startest du Claude Code im Ordner src-tauri und führst npm install aus. |
| de-30 | 27.81 | Ich möchte, dass du die Funktion dictation_start in der Datei commands.rs überarbeitest. Der Aufruf soll die Sitzungs-ID prüfen und einen Fehler zurückgeben, wenn kein Mikrofon gefunden wird. Schreib danach einen Test in der Datei contract.rs, führe cargo test aus und zeig mir das Ergebnis. Wenn alles grün ist, aktualisiere die README und starte Claude Code neu. |
| en-03 | 2.69 | Please run npm install now. |
| en-10 | 9.33 | Open the file package.json and check the version. Then start Claude Code in the src-tauri folder and run npm install. |
| en-30 | 25.09 | I want you to rework the function dictation_start in the file commands.rs. The call should check the session ID and return an error when no microphone is found. After that, write a test in the file contract.rs, run cargo test and show me the result. If everything is green, update the README, open a pull request on GitHub and restart Claude Code in the terminal. |

## Method

Spike program: loads the model once (`WhisperContext::new_with_params`, default parameters, CPU),
then per clip creates a state and runs `full` with greedy sampling (`best_of 1`), fixed language,
`no_context`, printing off. Wall time covers state creation plus `full` plus reading the segments.
Peak RAM is the process's `PeakWorkingSetSize` after the clip (model included). Each model and
thread count ran as its own process. Optimized build (finding 5), `GGML_NATIVE=OFF`, `GGML_AVX2=ON`.

## Results (optimized build)

| Model | Threads | Clip | Wall ms | RTF | Peak WS MB |
| --- | --- | --- | --- | --- | --- |
| base | 4 | de-03 | 714 | 0.245 | 257 |
| base | 4 | de-10 | 791 | 0.075 | 257 |
| base | 4 | de-30 | 1679 | 0.060 | 298 |
| base | 4 | en-03 | 732 | 0.273 | 256 |
| base | 4 | en-10 | 772 | 0.083 | 257 |
| base | 4 | en-30 | 1583 | 0.063 | 259 |
| base | 8 | de-03 | 468 | 0.161 | 257 |
| base | 8 | de-10 | 545 | 0.052 | 257 |
| base | 8 | de-30 | 1117 | 0.040 | 298 |
| base | 8 | en-03 | 464 | 0.173 | 257 |
| base | 8 | en-10 | 538 | 0.058 | 257 |
| base | 8 | en-30 | 1080 | 0.043 | 259 |
| small | 4 | de-03 | 2594 | 0.890 | 495 |
| small | 4 | de-10 | 2638 | 0.250 | 496 |
| small | 4 | de-30 | 3056 | 0.110 | 498 |
| small | 4 | en-03 | 2481 | 0.924 | 495 |
| small | 4 | en-10 | 2603 | 0.279 | 496 |
| small | 4 | en-30 | 2846 | 0.113 | 498 |
| small | 8 | de-03 | 1493 | 0.512 | 496 |
| small | 8 | de-10 | 1646 | 0.156 | 497 |
| small | 8 | de-30 | 2109 | 0.076 | 498 |
| small | 8 | en-03 | 1905 | 0.709 | 496 |
| small | 8 | en-10 | 1705 | 0.183 | 496 |
| small | 8 | en-30 | 1885 | 0.075 | 498 |

Model load (file in the OS cache, right after download): base 61 to 64 ms, small 153 to 257 ms.
Short clips cost almost as much as long ones: whisper encodes a padded 30 s window.

Transcripts are identical for 4 and 8 threads.

## Transcripts

- base de-03: `Führe bitte NPM in Stahl aus.`
- base de-10: `Öffne die DateiPayKitch.js in und prüfe die Version. Danach start hast du Clodcode im Ordner SRC Tauri und fürst in PmInstall aus.`
- base de-30: `Ich möchte, dass du die Funktion-Diktation unter Strichstart in der Datei-Kommanz RS überarbeitest. Der Aufruf soll die Sitzungseidiprüfen und einen Fehler zurückgeben, wenn kein Mikrofon gefunden wird. Schreibt danach einen Test in der Datei-Kanträg.rs, für Rekargo-Test aus und zeig mir das Ergebnis. Wenn alles grün ist, aktualisiere die Itme und starte Klodkot neu.`
- base en-03: `Please run NPM install now.`
- base en-10: `Open the file package.js/oan and check the version. Then start Claude code in the SRC Tori folder and run npm install.`
- base en-30: `I want you to rework the function dictation_start in the file commands.r's. The call should check the session ID and return an error when no microphone is found. After that, write a test in the file contract.r's, run cargo test and show me the result. If everything is green, update the readme, open a poll request on GitHub and restart cloud code in the terminal.`
- small de-03: `Führe bitte NP-M-Install aus.`
- small de-10: `Öffne die Datei Package JSO in und prüfe die Version. Danach startest du Code im Ordner SRC Tauri und führst NPM-Install aus.`
- small de-30: `Ich möchte, dass du die Funktion Diktation unter Strich-Start in der Datei Kommans RS überarbeitest. Der Aufruf soll die Sitzung sei die prüfen und einen Fehler zurückgeben, wenn kein Mikrofon gefunden wird. Schreib danach einen Test in der Datei Karn trägt RS, führe Khargo-Test aus und zeig mir das Ergebnis. Wenn alles grün ist, aktualisiere die Itmer und starte Claude Code neu.`
- small en-03: `Please run NPM install now.`
- small en-10: `Open the file package.json and check the version. Then start clod code in the src-tori folder and run npm install.`
- small en-30: `I want you to rework the function dictation_start in the file commands.rs. The call should check the session ID and return an error when no microphone is found. After that, write a test in the file contract.rs, run cargo test and show me the result. If everything is green, update the readme, open a poll request on GitHub and restart cloud code in the terminal.`

## Word errors (hand-scored)

Strict count against the reference words: substitutions plus deletions plus insertions; case and
punctuation ignored; a split or merged token counts per word (for example `SRC Tauri` for
`src-tauri` = 2, `Clodcode` for `Claude Code` = 2).

| Clip | Words | base | small |
| --- | --- | --- | --- |
| de-03 | 5 | 2 | 2 |
| de-10 | 21 | 12 | 8 |
| de-30 | 57 | 16 | 14 |
| en-03 | 5 | 0 | 0 |
| en-10 | 20 | 3 | 2 |
| en-30 | 64 | 4 | 2 |
| German total | 83 | 30 (36 %) | 24 (29 %) |
| English total | 89 | 7 (8 %) | 4 (4 %) |

Almost all German errors are English technical terms spoken in a German sentence (file names,
`npm install`, `Claude Code`, `dictation_start`); plain German words come out right in both models.
Synthetic speech, one speaker per language: not representative of real voices or microphones.

## Repeat runs: small, 8 threads, 10 s clips (variance)

Same build, each clip three times in one process (model loaded once), machine still under load
from other builds. Wall ms:

| Clip | No prompt | With initial prompt |
| --- | --- | --- |
| de-10 | 1959, 1854, 1731 | 4002, 2794, 2627 |
| en-10 | 3029, 2010, 1764 | 2409, 1863, 1982 |

## Initial prompt (`FullParams::set_initial_prompt`)

Prompt: `Claude Code, Codex, Gemini CLI, Aider, npm, git, README.` (agent and tool names only, not
the clips' file names). One run per clip, 8 threads:

| Model | Clip | Wall ms | Transcript |
| --- | --- | --- | --- |
| base | de-03 | 525 | Führe bitte npm in Stahl aus. |
| base | de-10 | 634 | Öffne die DateiPayKids.js/in- und prüfe die Version. Dann nach Start das To-Clode Code im Ordner SRC Tauri und Fürst in PmIn-Stahl aus. |
| base | de-30 | 1960 | (as without prompt, except `Sitzungseidie prüfen`, `Clod Code`) |
| base | en-03 | 665 | Please run npm install now. |
| base | en-10 | 654 | Open the file package.js/on and check the version. Then start Claude Code in the SRC TORI folder and run npm install. |
| base | en-30 | 655 | (as without prompt, except `README`, `Clod Code`) |
| small | de-03 | 2005 | Führe bitte npm-Install aus. |
| small | de-10 | 12564 | Öffne die Datei Package JSO in und prüfe die Version. Danach startest du Claude Code im Ordner SRC Tauri und führst npm-Install aus. |
| small | de-30 | 2724 | (better: `Comments.rs`, `Cargo-Test`; worse: `Führre`, `Clode Code`) |
| small | en-03 | 2369 | Please run npm install now. |
| small | en-10 | 2251 | Open the file package.json and check the version. Then start claude code in the src-tori folder and run npm install. |
| small | en-30 | 2269 | (better: `README`, `pull request`; `Cloud Code` remains) |

The prompt fixes some names (`npm`, `Claude Code` in a few clips, `README`, `pull request`) and
breaks others; small de-10 took 12.6 s once with the prompt.

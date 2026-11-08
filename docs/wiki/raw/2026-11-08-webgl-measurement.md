# Measurement record: xterm.js WebGL addon vs DOM renderer (2026-11-08)

Raw record. Immutable. Summary: [[webgl-measurement]].

## Platform

- Windows 11 Pro 26H2, build 10.0.26300.9818
- CPU AMD Ryzen 9 9900X; GPUs: AMD Radeon(TM) Graphics (iGPU), NVIDIA GeForce RTX 4070 (driver 32.0.16.2114). Edge/ANGLE picked the RTX 4070 (`ANGLE (NVIDIA, NVIDIA GeForce RTX 4070 (0x00002786) Direct3D11 vs_5_0 ps_5_0, D3D11)`), real GPU, not headless.
- Display 1920x1080, 60 Hz, devicePixelRatio 1 (only DPR 1 measured)
- Microsoft Edge 157.0.4341.42 (windowed `--app` mode, fresh `--user-data-dir`, `--disable-backgrounding-occluded-windows --disable-renderer-backgrounding --disable-background-timer-throttling`), viewport 1884x1061
- WebView2 Runtime on this machine: 157.0.4341.42 (`msedgewebview2.exe` file version; an older 156.0.4314.58 folder also exists). Same Chromium build as Edge.
- Not run inside a WebView2 host (no host available without starting Hangar, which was forbidden). Edge and WebView2 share the same Chromium engine and version; GPU process behavior may differ in the host.
- @xterm/xterm 6.0.0, @xterm/addon-webgl 0.19.0 (latest stable, peer `@xterm/xterm ^6.0.0`), Vite 7 build, node v26.13.0

## Method

Source: `tests/.tmp/webgl-bench/` (`main.js`, `serve.py`, `run-edge.ps1`, `summarize.py`). The page is built with `vite build` and served by `py -3 serve.py 5199`; the page POSTs its JSON to the server. Terminal 200x50, scrollback 10000, font 14 px Consolas. A fresh `Terminal` per run, one unrecorded warm-up per renderer, order alternated per run (DOM/WebGL, then WebGL/DOM). Writes use a window of 8 in-flight chunks of about 64 KB (frames: one write per frame). `write ms` is the time from first write to the callback of the last chunk; `settled ms` adds two requestAnimationFrame ticks. `rAF` is the frame count during the run, `LT` the PerformanceObserver `longtask` count and total ms (>50 ms tasks). Scenarios:

- a: 21.0 MB plain text lines
- b: 21.0 MB truecolor SGR lines, random RGB per word (worst case for a glyph atlas)
- c: 2000 full-screen redraws (`ESC[H` + 50 rows, random RGB per row)
- d: like b but 16 foreground colors and 4 backgrounds (agent-like palette)
- e: like c but 16-color palette
- f, g, h (paced one chunk per frame, frame-interval statistics): implemented, NOT measured (run aborted on request).

Commands:
```
cd tests/.tmp/webgl-bench && pnpm install && npx vite build
pwsh -File run-edge.ps1 -Name result-edge.json
py -3 summarize.py result-edge.json
```

## Raw numbers (ms; run1/run2/run3)

Run set 2 (scenarios a to e, the final build):

| scn | renderer | write ms | settled ms | rAF frames | longtasks | longtask total ms | max longtask ms |
|---|---|---|---|---|---|---|---|
| a | dom | 451/262/266 | 467/284/284 | 28/17/17 | 0/0/0 | 0 | 0 |
| a | webgl | 437/321/208 | 466/333/233 | 28/20/14 | 0/0/0 | 0 | 0 |
| b | dom | 629/422/432 | 651/438/456 | 26/19/18 | 0/0/0 | 0 | 0 |
| b | webgl | 1499/1873/1681 | 1588/1996/1775 | 25/28/29 | 15/17/17 | 1379/1713/1534 | 99/152/100 |
| c | dom | 230/117/106 | 250/150/117 | 15/9/7 | 0/0/0 | 0 | 0 |
| c | webgl | 229/185/157 | 249/216/183 | 12/11/10 | 0/0/0 | 0 | 0 |
| d | dom | 651/338/335 | 672/352/349 | 26/16/14 | 0/0/0 | 0 | 0 |
| d | webgl | 254/248/253 | 267/266/266 | 15/15/15 | 0/0/0 | 0 | 0 |
| e | dom | 149/104/106 | 167/117/117 | 10/7/7 | 0/0/0 | 0 | 0 |
| e | webgl | 171/192/111 | 183/200/133 | 11/12/8 | 0/0/0 | 0 | 0 |

Run set 1 (earlier build, scenarios a to c, repeat; write ms run1/run2/run3):

| scn | dom | webgl | webgl longtasks (count, total ms) |
|---|---|---|---|
| a | 445/231/229 | 226/226/217 | 0 |
| b | 592/462/392 | 1645/1627/2002 | 16/16/18, 1499/1479/1824 ms |
| c | 170/103/104 | 156/162/158 | 0 |

Context loss: none observed in any run (`ctxLost` false). JS heap delta per run was noisy (GC) and is not usable (range -572 MB to +353 MB).

The raw JSON files were deleted by the aborted re-run; the tables above are the verbatim `summarize.py` output.

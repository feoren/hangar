# Session row third line and system bar amounts: layout measurement (2026-11-13)

Platform: Windows 11 Pro 10.0.26300, Microsoft Edge headless (`--headless=new --dump-dom`), the
app's `src/styles.css` (UI font Segoe UI). A test page put session rows (plain and inside
`.agent-rows`, the indented project rows) and the system bar into `.sidebar` elements of 305, 320,
400 and 505 px. Rows: without resources, with CPU and RAM, with CPU, RAM and GPU, each with typical
values ("12%", "340 MB", "8% 180 MB", "2 hr", "34%", "137k / 1M") and the widest ones ("100%",
"12.3 GB", "100% 12.3 GB", "23 hr", "100%", "1.0M / 1M"), plus a row with GPU and no context meter.
System bar: "100 %" and "31.9 GB" as `Intl.NumberFormat` writes them in all 12 languages.

Per row: `.session-text` width, `.session-detail` clientWidth/scrollWidth, parts that end outside
the line (cut), whether the token count shows, and the parts on the visible line. Per gauge:
clientWidth/scrollWidth, whether the amount shows, space between name and value.

Natural widths of the parts (px): CPU and RAM 150 typical, 162 widest; with GPU 240 and 264;
context meter about 115 to 125 with the token count, 64 without, 52 with the bar at 12 px; names
to icons saves about 45. Room (`.session-text`): 272 / 224 (project row) at 305 px, 287 / 239 at
320, 367 / 319 at 400, 472 / 424 at 505.

Excerpt (thresholds 394, 334, 321 px with GPU; 284, 231 px with CPU and RAM):

```
305 res=1 gpu=1 worst=0 project=0 text=272 line=272/272 ok cut=[] tokens=false | cpu, memory, gpu, runtime, ·, context-bar, 34%
305 res=1 gpu=1 worst=1 project=1 text=224 line=224/224 ok cut=[] tokens=false | cpu, memory, ·, context-bar, 100%
320 res=1 gpu=0 worst=1 project=0 text=287 line=287/287 ok cut=[] tokens=true  | cpu, memory, runtime, ·, context-bar, 100%, context-tokens
505 res=1 gpu=1 worst=1 project=0 text=472 line=472/472 ok cut=[] tokens=true  | cpu, memory, gpu, runtime, ·, context-bar, 100%, context-tokens
305 de gauge-memory 106/106 amount=true space=11.3
305 fr gauge-cpu 74/74 space=19.3
```

All 56 rows: `scrollWidth == clientWidth`, no part cut, the context share always on the line.
All 144 gauges (4 widths, 12 languages, 3 gauges): no overflow, amounts visible from 305 px, at
least 11 px between name and value.

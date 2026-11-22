# Measurement: paints of the crew column at rest (2026-11-22)

- Platform: Windows 11 Pro 10.0.26300, Microsoft Edge 158.0.4370.33, headless (`--headless=new`,
  `--disable-gpu`), driven over the Chrome DevTools Protocol with `tools/readme-shots/browser.mjs`.
- Page: a local test page served by Vite that renders the real `src/CrewColumn.tsx` with the real
  `src/styles.css` and bundled figures, the sub-agent watch and the figure library faked. One
  column and one strip, each with six sub-agents: working with text, working without a tool yet
  (typing dots), waiting (breathing ring), done, quiet, picked. Viewport 1000 x 1500, scale 1,
  dark theme. The component's 500 ms clock keeps re-rendering React during the window.
- Method: after load and 0.9 s of settling, `Tracing.start` with the categories
  `devtools.timeline,disabled-by-default-devtools.timeline` (`transferMode: ReportEvents`), wait
  3000 ms, `Tracing.end`, count trace events by name.
- Modes: normal; `Emulation.setEmulatedMedia` with `prefers-reduced-motion: reduce`; the
  `power-saving` class on the root element.

## Output

Before the redesign (block plinth, no loops):

```
{"normal":{"Paint":0,"PrePaint":0,"Layout":0},"reduced":{"Paint":0,"PrePaint":0,"Layout":0},"power":{"Paint":0,"PrePaint":0,"Layout":0}}
```

After the redesign (round stage, typing dots and waiting ring animating opacity):

```
{"normal":{"Paint":0,"PrePaint":39,"Layout":0},"reduced":{"Paint":0,"PrePaint":0,"Layout":0},"power":{"Paint":0,"PrePaint":0,"Layout":0}}
```

`PrePaint` in the normal mode comes from the running opacity animations (the lifecycle updates
while animations tick); no `Paint` and no `Layout` occur. With reduced motion and power saving
nothing runs at all.

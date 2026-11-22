# Measurement: size and cost of the figures' idle loops (2026-11-22)

- Platform: Windows 11 Pro 10.0.26300, Microsoft Edge 158.0.4370.33, headless (`--headless=new`,
  `--disable-gpu`), driven over the Chrome DevTools Protocol with `tools/readme-shots/browser.mjs`;
  Vite 8.3.6, pnpm build of Hangar 0.14.0.
- Files: three test loops, `backend-03`, `frontend-02` and `social-media-02`, animated WebP,
  160 x 240, RGBA, 80 frames, loop 0, read with Pillow.

## Bundle size

`pnpm build`, then `du -sb dist`:

```
before (no loops):   7981787 bytes, 191 files in dist/assets
after (3 loops):    11512700 bytes, 194 files in dist/assets
dist/assets/backend-03-CU_tPNBB.webp                1,059.05 kB
```

The three loops are 1059058, 1015078 and 1439320 bytes (mean 1171152). The script chunks change
by a few hundred bytes (three URLs). 41 loops at that mean: about 48.0 MB.

## Place of the figure

Bounding box of alpha above 8, first frame of the loop against the still file scaled by 2/3:

```
backend-03 anim (6, 1, 153, 240) still*2/3 (6.7, 1.3, 152.7, 238.7)
frontend-02 anim (19, 0, 141, 239) still*2/3 (20.0, 1.3, 140.0, 238.7)
social-media-02 anim (0, 32, 159, 240) still*2/3 (1.3, 33.3, 158.7, 238.7)
```

## CPU and memory, ten visible figures

A plain test page with ten `<img>` of 59 x 88 px, the three files in turn, either the still or the
animated files. After 3 s of settling two screenshots 0.4 s apart are compared (they differ only
for the loops), then the summed user and kernel time of every browser process
(`Win32_Process`) over 15 s and their summed working set before and after. Two rounds in the
order still, animated, still, animated; the first round includes the browser's start.

```
still: frames differ: false, images loaded: true
still: CPU 4.0 % of one core, working set 812 -> 601 MB
animated: frames differ: true, images loaded: true
animated: CPU 3.1 % of one core, working set 656 -> 549 MB
still: frames differ: false, images loaded: true
still: CPU 0.0 % of one core, working set 551 -> 570 MB
animated: frames differ: true, images loaded: true
animated: CPU 7.9 % of one core, working set 614 -> 625 MB
```

An earlier run with 10 s windows: still 8.1 % and 1.3 %, animated 4.8 % and 1.8 %. The working set
of the whole browser moves by tens of MB between readings either way; a difference caused by the
loops is not visible above that noise.

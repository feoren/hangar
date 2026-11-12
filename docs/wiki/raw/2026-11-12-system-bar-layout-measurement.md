# System bar layout measurement (2026-11-12)

Platform: Windows 11 Pro 10.0.26300, Microsoft Edge headless (`--headless=new --dump-dom`), the
app's `src/styles.css` and `src/fonts.css` (UI font Segoe UI Variable Text). A test page put the
system bar's markup into a `.sidebar` of 200, 272 and 560 px and set every gauge's value to
`Intl.NumberFormat(language, { style: "percent" }).format(1)` with the gauge names of each
language (ru: CPU, ОЗУ, GPU; zh: CPU, 内存, GPU; else CPU, RAM, GPU).

Per gauge: `clientWidth/scrollWidth`, space between name and value, sparkline width.

```
200 en "100%" h=24 | 60/60 gap 8.9 spark 60 | 60/60 gap 5.8 spark 60 | 60/60 gap 8.1 spark 60
200 fr "100&nbsp;%" h=24 | 60/60 gap 6.1 spark 60 | 60/60 gap 3.0 spark 60 | 60/60 gap 5.4 spark 60
200 ru "100&nbsp;%" h=24 | 60/60 gap 6.1 spark 60 | 60/60 gap 6.3 spark 60 | 60/60 gap 5.4 spark 60
200 zh "100%" h=24 | 60/60 gap 8.9 spark 60 | 60/60 gap 8.0 spark 60 | 60/60 gap 8.1 spark 60
272 de "100&nbsp;%" h=24 | 84/84 gap 30.1 spark 84 | 84/84 gap 27.0 spark 84 | 84/84 gap 29.4 spark 84
272 fr "100&nbsp;%" h=24 | 84/84 gap 30.1 spark 84 | 84/84 gap 27.0 spark 84 | 84/84 gap 29.4 spark 84
272 zh "100%" h=24 | 84/84 gap 32.9 spark 84 | 84/84 gap 32.0 spark 84 | 84/84 gap 32.1 spark 84
bad gauges: 0
```

(Excerpt; all 36 combinations of width and language passed: no overflow, name and value apart.)

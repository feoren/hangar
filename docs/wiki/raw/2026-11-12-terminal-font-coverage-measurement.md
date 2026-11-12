# Measurement: glyph coverage of monospace web fonts for agent CLIs

Date: 2026-11-12. Platform: Windows 11, Python 3.14 with fontTools 4.66.0 and brotli (to read
woff2). Packages fetched with `npm pack` into a scratch folder, unpacked, then deleted.

Method: union of the `cmap` (best cmap) of the regular woff2 files of each package. Counted:
box drawing U+2500..U+257F (128), block elements U+2580..U+259F (32), braille U+2800..U+28FF
(256), and single symbols agent CLIs print: U+23FA, U+23BF, U+2714, U+276F, U+273B, U+2026,
U+2192, U+25CF, U+25B8, U+26A0, U+2713, U+2717, U+00B7 (shown in this order, `_` = missing).

## Output (verbatim)

```
bundled fira-code                                  100K box=128/128 block=32/32 braille=0/256 _____…→●__✓_·
bundled geist-mono                                  49K box=128/128 block=32/32 braille=0/256 _____…→●____·
bundled hack                                       103K box=128/128 block=32/32 braille=0/256 ___❯_…→●▸___·
bundled jetbrains-mono                              90K box=128/128 block=32/32 braille=0/256 ___❯_…→●▸⚠✓✗·
fontsource-cascadia-code-5.3.0 ALL                 144K box=128/128 block=32/32 braille=256/256 _____…______·
fontsource-commit-mono-5.3.0 ALL                    47K box=127/128 block=32/32 braille=256/256 __✔__…→●__✓✗·
fontsource-ibm-plex-mono-5.3.0 ALL                  48K box=0/128 block=0/32 braille=0/256 _____…______·
fontsource-inconsolata-5.3.0 ALL                    43K box=0/128 block=0/32 braille=0/256 _____…______·
fontsource-intel-one-mono-5.3.0 ALL                 44K box=128/128 block=32/32 braille=0/256 _____…______·
fontsource-iosevka-5.3.0 ALL                       961K box=128/128 block=32/32 braille=256/256 ⏺_✔❯_…→●▸_✓✗·
fontsource-maple-mono-5.3.0 ALL                     72K box=128/128 block=32/32 braille=256/256 _⎿✔❯_…→●▸⚠✓✗·
fontsource-martian-mono-5.3.0 ALL                   23K box=0/128 block=0/32 braille=0/256 _____…______·
fontsource-monaspace-neon-5.3.0 ALL                 43K box=0/128 block=0/32 braille=0/256 _____…→_▸___·
fontsource-red-hat-mono-5.3.0 ALL                   15K box=0/128 block=0/32 braille=0/256 _____…______·
fontsource-source-code-pro-5.3.1 ALL                51K box=0/128 block=0/32 braille=0/256 _____…______·
fontsource-space-mono-5.3.0 ALL                     38K box=0/128 block=0/32 braille=0/256 _____…______·
fontsource-ubuntu-mono-5.3.0 ALL                   106K box=0/128 block=0/32 braille=0/256 _____…______·
fontsource-victor-mono-5.3.0 ALL                    43K box=0/128 block=0/32 braille=0/256 _____…______·
SourceCodePro-Regular.ttf.woff2                     72K box=128/128 block=32/32 braille=0/256 _____…→●▸_✓_·
SourceCodePro-Bold.ttf.woff2                        72K box=128/128 block=32/32 braille=0/256 _____…→●▸_✓_·
IBMPlexMono-Regular.woff2                           48K box=128/128 block=32/32 braille=0/256 _____…→___✓_·
IBMPlexMono-Bold.woff2                              49K box=128/128 block=32/32 braille=0/256 _____…→___✓_·
VictorMono-Regular.woff2                            69K box=128/128 block=32/32 braille=0/256 _____…→●▸___·
VictorMono-Bold.woff2                               70K box=128/128 block=32/32 braille=0/256 _____…→●▸___·
```

`ALL` = every subset file of the package (latin, latin-ext, cyrillic, greek, vietnamese,
symbols2 where present). The last six rows are the upstream files from the fonts' own npm
packages: `source-code-pro` 2.42.0 (`WOFF2/TTF/`), `@ibm/plex-mono` 2.5.0
(`fonts/complete/woff2/`), `victormono` 1.5.6 (`dist/woff2/`).

# Research: agent icons, Simple Icons licenses and brand colors (2026-11-12)

## Simple Icons package

`https://registry.npmjs.org/simple-icons/latest`: version `16.37.0`, license `CC0-1.0`.
Data file `https://cdn.jsdelivr.net/npm/simple-icons@16.37.0/data/simple-icons.json`, 3470 icons.
Entries used (verbatim JSON):

```
{"title": "Claude", "slug": "claude", "hex": "D97757", "source": "https://claude.ai"}
{"title": "Claude Code", "slug": "claudecode", "hex": "D97757", "source": "https://code.claude.com"}
{"title": "Google Gemini", "slug": "googlegemini", "hex": "8E75B2", "source": "https://gemini.google.com", "aliases": {"old": ["Google Bard"]}}
{"title": "GitHub Copilot", "slug": "githubcopilot", "hex": "000000", "source": "https://primer.style/foundations/icons/copilot-24", "license": {"type": "MIT"}}
{"title": "Cursor", "slug": "cursor", "hex": "000000", "source": "https://cursor.com/brand"}
{"title": "Kimi", "slug": "kimi", "hex": "000000", "source": "https://moonshotai.github.io/Branding-Guide", "guidelines": "https://moonshotai.github.io/Branding-Guide"}
{"title": "QWen", "slug": "qwen", "hex": "6950EF", "source": "https://qwen.ai"}
{"title": "OpenCode", "slug": "opencode", "hex": "000000", "source": "https://github.com/anomalyco/opencode/blob/1251a870cb384543c150c4a72fb101b55eec971b/packages/identity/mark.svg"}
{"title": "Pi", "slug": "pi", "hex": "000000", "source": "https://pi.dev/favicon.svg"}
{"title": "Cline", "slug": "cline", "hex": "18181B", "source": "https://cline.bot/assets/branding/logos/cline-wordmark-black.svg"}
{"title": "Mistral AI", "slug": "mistralai", "hex": "FA520F", "source": "https://chat.mistral.ai"}
```

Searched titles, slugs and aliases for the other catalog products and found no entry: OpenAI or
Codex (only "OpenAI Gym"), Antigravity, Kiro, Aider, Goose, Crush or Charm, Oh My Pi, ForgeCode,
Kode, Codebuff, Factory or Droid, Amp (the `amp` slug is "AMP", Accelerated Mobile Pages, a
different product).

`pi.dev` serves a page titled "Pi" that links `earendil.com/posts/pi-1-0/`, the vendor of the
catalog's Pi (`github.com/earendil-works/pi`).

## Simple Icons DISCLAIMER.md (16.37.0), excerpts

> Simple Icons is released under CC0 - though that doesn't mean to imply that all icons within the
> project are also CC0. Please see individual licenses where available.

> The addition of licenses to Simple Icons is an ongoing project. Hence, the absence of license
> data for a particular icon does not imply that the icon is not released under a license.

> Simple Icons cannot be held responsible for any legal activity raised by a brand, or users of the
> package. We ask that our users seek the correct permissions to use the icons relevant to their
> project.

> Simple Icons provides a link to a brand's _branding guidelines_ (or similar) if the brand provides
> one. We ask our users read these guidelines and ensure their usage of the brand's icon is in
> accordance with them.

LICENSE.md is the CC0 1.0 Universal text; section 4.1: "No trademark or patent rights held by
Affirmer are waived, abandoned, surrendered, licensed or otherwise affected by this document."

## Primer Octicons license

`https://cdn.jsdelivr.net/npm/@primer/octicons@19/LICENSE`: "MIT License", "Copyright (c) 2026 GitHub Inc."

## Brand guideline pages

- `https://github.com/logos`: "The Invertocat and our wordmark should only appear in white, black,
  or in few cases grey or green. Like text, the logo should be legible and pass accessibility
  requirements in all settings."
- `https://cursor.com/brand`: brand assets download; "Refer to us as Cursor. Not Cursor AI or
  Cursor Code." No usage restriction for the cube found in the page text.
- `https://moonshotai.github.io/Branding-Guide/`: icon variants for light and dark backgrounds,
  "Rounded Corner Icon", "K Only" on light, dark and color backgrounds.

## Colors on vendor websites (most frequent `#rrggbb` in the HTML, theme-color meta)

Fetched with a plain HTTP GET, 2026-11-12:

```
antigravity.google           #121317 (13), #ff4641, #346bf1, #1a73e8
kiro.dev                     #938f9b, #211d25, #f2f1f4, #28242e, #c19aff
aider.chat                   none
block.github.io/goose/       redirect page, none
charm.land                   #fffdf5, #ffffff, #fffaf1, #ff80e3
forgecode.dev                theme-color #ffffff / #0f1012; #ea580c (9), #fb923c, #f97316, #c2410c
codebuff.com                 #f0f0f0
factory.ai                   theme-color #f9f9f9; #d15010 (7)
ampcode.com                  theme-color #dfdfc1 / #091c1e; #dfdfc1 (231), #f6833b (135)
developers.openai.com/codex  #8f8f8f, #252525, #202020, #606060
```

## Program icons on Windows today

Platform: Windows 11 Pro 10.0.26300. `where.exe` for each catalog command, then
`ExtractIconExW(file, -1, ...)` (number of icons in the file) on the executables Hangar starts:

```
claude.exe (native install, ~\.local\bin)                 2
codex.exe (OpenAI\Codex\bin; codex.cmd from npm wins on PATH)  0
copilot.exe (VS Code copilot-shim)                         0
aider.exe (uv tool, ~\.local\bin)                          0
codex, opencode, pi: npm .cmd shims (no icon by design, src-tauri/src/icon.rs)
```

All other catalog commands were not installed on this machine.

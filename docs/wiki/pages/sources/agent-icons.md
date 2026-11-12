---
title: "Agent icons: Simple Icons licenses, brand guidelines, brand colors"
type: source
tags: [agents, icons, licenses, ui]
sources: []
raw: raw/2026-11-12-agent-icons-research.md
url: https://github.com/simple-icons/simple-icons/blob/develop/DISCLAIMER.md
retrieved: 2026-11-12
created: 2026-11-12
updated: 2026-11-12
---

# Agent icons: Simple Icons licenses, brand guidelines, brand colors

Research of 2026-11-12 for the bundled agent icons (`src/assets/agent-icons/`, `src/agentIcons.ts`):
the Simple Icons 16.37.0 data file, its disclaimer and license, three brand guideline pages, the
colors on the vendors' websites and which catalog programs carry an icon on Windows.

## Key points

- Simple Icons 16.37.0 is CC0-1.0 as a project, but an icon may carry its own license in the
  `license` field of `data/simple-icons.json`; a missing field does not prove CC0. CC0 waives no
  trademark rights. Users are asked to follow the brand guidelines where listed.
- Marks present for 10 of the 22 catalog agents: `claude` (also `claudecode`, the pixel mascot),
  `googlegemini`, `githubcopilot` (license MIT, from Primer Octicons, "Copyright (c) 2026 GitHub
  Inc."), `cursor`, `kimi` (guidelines listed), `qwen`, `opencode`, `pi`, `cline`, `mistralai`.
- No entry for OpenAI or Codex, Antigravity, Kiro, Aider, Goose, Crush, Oh My Pi, ForgeCode, Kode,
  Codebuff, Factory Droid or Amp. The `amp` slug is a different product (Accelerated Mobile Pages).
- GitHub's logo guidelines allow the marks in white or black and ask for legibility; Kimi's guide
  offers a rounded-corner icon and variants for dark and color backgrounds.
- Vendor sites show these accents: Antigravity `#1a73e8`, Kiro `#c19aff` on `#211d25`, Charm
  `#ff80e3`, ForgeCode `#c2410c`/`#ea580c`, Factory `#d15010`, Amp `#f6833b` on `#091c1e`, Codex
  near black `#202020`. Aider, Goose and Codebuff showed no usable color.
- On Windows only the native `claude.exe` carries an icon; `codex.exe`, the VS Code `copilot.exe`
  shim and the uv `aider.exe` have none, and npm installs are `.cmd` shims without one.

## Significance for Hangar

- Catalog agents get a bundled tile each, so they look alike regardless of how the CLI was
  installed; the exe icon stays for shells and the user's own programs
  ([[catalog]], section "Icons").
- Marks come from Simple Icons with their per-icon license recorded in
  `src/assets/agent-icons/NOTICE.md`; products without a mark get a monogram drawn for Hangar, no
  copied artwork.
- Re-check the license field and guidelines when updating the marks; Simple Icons removes icons on
  brand request.

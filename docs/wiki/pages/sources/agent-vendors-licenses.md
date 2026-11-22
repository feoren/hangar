---
title: "Agent vendors and licenses: who makes each catalog agent, which are open source"
type: source
tags: [agents, catalog, licenses, i18n]
sources: []
raw: raw/2026-11-22-agent-vendors-licenses-research.md
url: https://docs.github.com/rest/licenses/licenses
retrieved: 2026-11-22
created: 2026-11-22
updated: 2026-11-22
---

# Agent vendors and licenses

Desk research of 2026-11-22 for the agent descriptions under "Available" in Settings > Programs
(`agent.<id>.description` in `src/i18n.ts`). For each of the 22 catalog agents: the vendor and
whether the CLI is open source, from the repository's LICENSE file (GitHub license API), the npm
package metadata and, where no repository exists, the vendor's terms page. Nothing was installed.

## Results

| Agent | Vendor named in the description | License | Evidence |
| --- | --- | --- | --- |
| Claude Code | Anthropic | proprietary | `anthropics/claude-code` LICENSE.md "All rights reserved" |
| Codex CLI | OpenAI | Apache-2.0 | `openai/codex` LICENSE, NOTICE "Copyright 2025 OpenAI" |
| Gemini CLI | Google | Apache-2.0 | `google-gemini/gemini-cli` LICENSE "Copyright 2025 Google LLC" |
| Antigravity CLI | Google | proprietary | antigravity.google terms, no public repository |
| GitHub Copilot CLI | GitHub | proprietary | `github/copilot-cli` LICENSE.md "GitHub Copilot CLI License" (run and redistribute unmodified only) |
| Cursor CLI | Anysphere | proprietary | cursor.com terms "Anysphere, Inc.", no public repository |
| Kimi Code CLI | Moonshot AI | MIT | `MoonshotAI/kimi-code` LICENSE "Copyright (c) 2026 Moonshot AI" |
| Qwen Code | Alibaba Cloud | Apache-2.0 | `QwenLM/qwen-code` LICENSE "Copyright 2025 Qwen"; org description "Alibaba Cloud's general-purpose AI models" |
| Kiro CLI | Amazon | proprietary | `kirodotdev/Kiro` README "©2026 Amazon.com, Inc. ... All Rights Reserved", no LICENSE file |
| OpenCode | Anomaly | MIT | `anomalyco/opencode` LICENSE, org name "Anomaly" |
| Aider | (none named, see pattern) | Apache-2.0 | `Aider-AI/aider` LICENSE |
| Goose | Agentic AI Foundation | Apache-2.0 | `aaif-goose/goose` LICENSE; README "part of the Agentic AI Foundation (AAIF) at the Linux Foundation" |
| Crush | Charm | FSL-1.1-MIT, not open source | `charmbracelet/crush` LICENSE.md "Copyright 2025-2026 Charmbracelet, Inc." |
| Pi | Earendil Works | MIT | `earendil-works/pi` LICENSE (author Mario Zechner) |
| Oh My Pi | Stencil Labs | MIT | `can1357/oh-my-pi` LICENSE, README "Built by Stencil Labs" |
| ForgeCode | Tailcall | Apache-2.0 | `tailcallhq/forgecode` LICENSE "Copyright 2025 Tailcall" |
| Kode CLI | ShareAI Lab | Apache-2.0 | `shareAI-lab/Kode-CLI` LICENSE "Copyright 2024 ShareAI Lab" |
| Codebuff | Freebuff | Apache-2.0 | `CodebuffAI/codebuff` (now `CodebuffAI/freebuff`) LICENSE "Copyright 2025 Freebuff, Inc." |
| Cline CLI | (none named, see pattern) | Apache-2.0 | `cline/cline` LICENSE "Copyright 2026 Cline Bot Inc."; npm `cline` points to this repository |
| Mistral Vibe | Mistral AI | Apache-2.0 | `mistralai/mistral-vibe` LICENSE "Copyright 2025 Mistral AI" |
| Droid | Factory | proprietary | `Factory-AI/factory` README "All rights reserved", npm `UNLICENSED` |
| Amp | (none named, see pattern) | proprietary | ampcode.com terms: "Amp Frontier Corporation", a limited license to use |

"Proprietary" here means: no OSI license was found; the evidence is an all-rights-reserved notice,
a usage license or terms of service.

## Key points

- **Amp is no longer presented as a Sourcegraph product.** Its terms name Amp Frontier Corporation;
  the npm package still lives under the `@sourcegraph` scope.
- **Goose moved from Block to the Agentic AI Foundation** (Linux Foundation); `block/goose`
  redirects to `aaif-goose/goose`.
- **Crush is source available, not open source**: FSL-1.1-MIT turns into MIT only two years after
  each release. The description therefore names no license.
- **Kimi**: the Python `MoonshotAI/kimi-cli` repository is archived; the Kimi Code CLI that the
  catalog installs is `MoonshotAI/kimi-code` (MIT). The catalog homepage now points there.

> [!warning] Contradiction
> ForgeCode: the repository LICENSE is Apache-2.0, the npm package `forgecode` declares MIT (author
> Antinomy Inc., repository `antinomyhq/forge`, which now redirects to `tailcallhq/forgecode`).
> Codebuff: the repository LICENSE is Apache-2.0, the npm package `codebuff` declares MIT. Hangar
> follows the LICENSE file, which is the authoritative grant; both licenses are open source.

> [!question] Unverified
> Whether the npm packages `codebuff` and `forgecode` are built from exactly the public repository
> sources. Whether Antigravity CLI and Cursor CLI ship any open source parts; no repository was
> found.

## Description pattern (built 2026-11-22)

Every `agent.<id>.description` in all 12 languages has the same two-part form:

1. "Coding agent by {vendor} for your terminal." The vendor is the organization that owns the
   official repository or is named in the license or terms, without legal suffixes. It is left out
   ("Coding agent for your terminal.") where it would only repeat the product name (Aider AI, Cline
   Bot, Amp Frontier).
2. "Open source ({SPDX id})." only for an OSI license evidenced above; otherwise nothing.

`src/i18n.test.ts` checks that every description is at most 110 characters, ends a sentence, has
no dash characters U+2013 or U+2014 and names exactly the evidenced license (or none) in every
language, from a per-agent table that a new catalog entry must extend.

## Significance for Hangar

The descriptions make claims about third parties. Each claim now has a dated source; recheck this
page when a vendor or license changes (for example on a repository move or rename).

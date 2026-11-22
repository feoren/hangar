# Research: vendor and license of every catalog agent (2026-11-22)

Desk research for the agent descriptions in `src/i18n.ts`. Sources: the GitHub REST API
(`GET /repos/{owner}/{repo}`, `GET /repos/{owner}/{repo}/license`, `GET /orgs/{org}`, file
contents), `npm view <package> license repository.url author homepage version`, and the terms
pages of the vendors without a public repository. Nothing was installed or run.

## GitHub repositories (license as GitHub detects it, from the LICENSE file)

```
anthropics/claude-code        owner anthropics       license none (LICENSE.md, see below)
openai/codex                  owner openai           Apache-2.0
google-gemini/gemini-cli      owner google-gemini    Apache-2.0   LICENSE: "Copyright 2025 Google LLC"
github/copilot-cli            owner github           NOASSERTION  (LICENSE.md, see below)
MoonshotAI/kimi-cli           owner MoonshotAI       Apache-2.0   archived: true
MoonshotAI/kimi-code          owner MoonshotAI       MIT          LICENSE: "Copyright (c) 2026 Moonshot AI"
QwenLM/qwen-code              owner QwenLM           Apache-2.0   LICENSE: "Copyright 2025 Google LLC", "Copyright 2025 Qwen"
sst/opencode -> anomalyco/opencode                   MIT          LICENSE: "Copyright (c) 2025 opencode"
Aider-AI/aider                owner Aider-AI         Apache-2.0
block/goose -> aaif-goose/goose                      Apache-2.0
charmbracelet/crush           owner charmbracelet    NOASSERTION  (LICENSE.md, see below)
badlogic/pi-mono -> earendil-works/pi                MIT          LICENSE: "Copyright (c) 2025 Mario Zechner"
can1357/oh-my-pi              owner can1357 (User)   MIT          LICENSE: "Copyright (c) 2025 Mario Zechner",
                                                                  "Copyright (c) 2025-2026 Can Bölük",
                                                                  "Copyright (c) 2026 Stencil Labs, Inc."
antinomyhq/forge -> tailcallhq/forgecode             Apache-2.0   LICENSE: "Copyright 2025 Tailcall"
shareAI-lab/kode -> shareAI-lab/Kode-CLI             Apache-2.0   LICENSE: "Copyright 2024 ShareAI Lab"
CodebuffAI/codebuff -> CodebuffAI/freebuff           Apache-2.0   LICENSE: "Copyright 2025 Freebuff, Inc."
cline/cline                   owner cline            Apache-2.0   LICENSE: "Copyright 2026 Cline Bot Inc."
mistralai/mistral-vibe        owner mistralai        Apache-2.0   LICENSE: "Copyright 2025 Mistral AI"
Factory-AI/factory            owner Factory-AI       none (no LICENSE file)
kirodotdev/Kiro               owner kirodotdev       none (no LICENSE file)
sourcegraph/amp               404 Not Found
```

Verbatim file excerpts:

- `anthropics/claude-code` `LICENSE.md`: "© Anthropic PBC. All rights reserved. Use is subject to
  Anthropic's [Commercial Terms of Service](https://www.anthropic.com/legal/commercial-terms)."
- `github/copilot-cli` `LICENSE.md`: "GitHub Copilot CLI License ... GitHub grants you a
  non‑exclusive, non‑transferable, royalty‑free license to install and run copies of the GitHub
  Copilot CLI (the "Software")". Redistribution only "in unmodified form".
- `charmbracelet/crush` `LICENSE.md`: "Functional Source License, Version 1.1, MIT Future License
  ... FSL-1.1-MIT ... Copyright 2025-2026 Charmbracelet, Inc."
- `MoonshotAI/kimi-cli` description: "[Archived] Legacy Python Kimi CLI, no longer maintained.
  Please use Kimi Code CLI: https://github.com/MoonshotAI/kimi-code"
- `aaif-goose/goose` `README.md`: "goose is part of the [Agentic AI Foundation
  (AAIF)](https://aaif.io/) at the Linux Foundation."
- `Factory-AI/factory` `README.md`, section License: "Copyright © 2025-2026 Factory AI. All rights
  reserved."
- `kirodotdev/Kiro` `README.md`: "©2026 Amazon.com, Inc. or its affiliates (collectively,
  "Amazon"). All Rights Reserved." The README lists the CLI as one Kiro surface.
- `can1357/oh-my-pi` `README.md`: "Built by Stencil Labs · Fork of Pi by @mariozechner".
- `CodebuffAI/freebuff` description: "The free coding agent"; the repository contains a `cli`
  folder. `https://codebuff.com` redirects to `https://www.codebuff.com/` (title "Codebuff", a
  dash, then "AI Coding Assistant for Your Terminal"), whose privacy and terms links point to freebuff.com.

GitHub organization names (`GET /orgs/{org}`, field `name`, `description`):

```
anomalyco       Anomaly
Aider-AI        Aider AI
earendil-works  Earendil Works
QwenLM          Qwen            "Alibaba Cloud's general-purpose AI models"
tailcallhq      Tailcall
shareAI-lab     shareAI
charmbracelet   Charm
MoonshotAI      Moonshot AI
aaif-goose      goose
CodebuffAI      Freebuff
```

## npm packages

```
@anthropic-ai/claude-code       license 'SEE LICENSE IN README.md'  author 'Anthropic'
@openai/codex                   Apache-2.0  repo openai/codex
@google/gemini-cli              Apache-2.0  repo google-gemini/gemini-cli
@github/copilot                 'SEE LICENSE IN LICENSE.md'  author 'GitHub'  repo github/copilot-cli
@moonshot-ai/kimi-code          MIT  author 'Moonshot AI'  repo MoonshotAI/kimi-code
@qwen-code/qwen-code            (no license field)  repo QwenLM/qwen-code
opencode-ai                     MIT
@charmland/crush                FSL-1.1-MIT  repo charmbracelet/crush
@earendil-works/pi-coding-agent MIT  author 'Mario Zechner'  repo earendil-works/pi
@oh-my-pi/pi-coding-agent       MIT  author 'Stencil Labs, Inc.'  repo can1357/oh-my-pi
forgecode                       MIT  author 'Antinomy Inc.'  repo antinomyhq/forge
@shareai-lab/kode               Apache-2.0  author 'ShareAI-lab'  repo shareAI-lab/kode
codebuff                        MIT  repo CodebuffAI/codebuff
cline                           Apache-2.0  author 'Cline Bot Inc.'  repo cline/cline
@sourcegraph/amp                'SEE LICENSE IN LICENSE.md'  homepage https://ampcode.com/
droid, @factory/cli             UNLICENSED  repo Factory-AI/factory
```

## Vendor pages (no public repository)

- `https://ampcode.com/terms`: the service is provided by "Amp Frontier Corporation ("Amp")";
  users get a "limited, revocable, non-exclusive, non-transferable, non-sublicensable license" and
  "Amp owns all right, title, and interest in the Services."
- `https://cursor.com/terms-of-service`: "thank you for your interest in Anysphere, Inc."
- `https://antigravity.google/terms`: "This Agreement is a binding contract between you and Google
  regarding your use of the Service." No source repository or license file found.

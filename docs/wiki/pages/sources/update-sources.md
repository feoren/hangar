---
title: "Update sources of the agent CLIs (research and measurement)"
type: source
tags: [agents, updates, npm, pypi, github]
sources: []
raw: raw/2026-11-07-update-sources-research.md
url:
retrieved: 2026-11-07
created: 2026-11-07
updated: 2026-11-07
---

# Update sources of the agent CLIs

Research from 2026-11-07 (`raw/2026-11-07-update-sources-research.md`) plus our own measurement on the same
day (`raw/2026-11-07-update-sources-measurement.md`).

## Key points
- Endpoints: npm `https://registry.npmjs.org/<package>/latest` (`version`), PyPI
  `https://pypi.org/pypi/<package>/json` (`info.version`), GitHub
  `https://api.github.com/repos/<owner>/<repo>/releases/latest` (`tag_name`). Only `/latest`, because lists
  contain pre-releases.
- Without authentication, GitHub allows 60 requests per hour and IP and requires a user agent; prefer npm and
  PyPI over GitHub and cache the results.
- reqwest 0.13.5 and rustls 0.23.46 are already in the tree through tauri-plugin-updater 2.14.1
  (`rustls-no-provider`, `system-proxy`); the updater sets ring as the crypto provider.
- OpenCode: `@opencode/cli` (2.x, 2.2.6) and `opencode-ai` (1.x, 1.19.5) both ship `opencode`;
  `opencode-ai` 1.15.7 is installed locally. `opencode upgrade` and `pi update` are confirmed locally.
- Live lookup through the backend: claude-code 2.1.319, aider-chat 0.86.2, PowerShell 7.6.9.

## Significance for Hangar
Values and limits: [[updates]]. Contract: [[program-updates]].

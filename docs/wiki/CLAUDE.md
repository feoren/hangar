# Hangar wiki: schema

Project wiki for Hangar, following the "LLM Wiki" pattern (Karpathy).
The LLM writes and maintains the wiki. The human curates sources, asks questions and steers.
This file is the schema. Change it only after asking.

**Scope:** rules and way of working live in the project rules ([`CONTRIBUTING.md`](../../CONTRIBUTING.md)).
The wiki holds knowledge and evidence: how libraries, platforms and agent CLIs
actually behave, and why decisions were made the way they were. Never duplicate a rule;
refer to the project rules instead.

## Structure

```
raw/              Raw sources. IMMUTABLE: read only, never edit, never delete (one-time exception: translated to English on 2026-11-08).
                  Doc excerpts, issues, changelogs, measurement records, logs, notes.
raw/assets/       Images and attachments belonging to the sources.
pages/            Pages written by the LLM.
  sources/        One summary per raw source.
  components/     Libraries, frameworks, OS APIs (portable-pty, xterm.js, Tauri, ConPTY …).
  platforms/      Windows, macOS, Linux (X11/Wayland): behavior, pitfalls, versions.
  agents/         Agent CLIs (Claude Code, Codex CLI, Gemini CLI, Aider …): invocation,
                  TTY expectations, environment, exit behavior.
  concepts/       Terms and mechanisms (PTY, VT/ANSI, bracketed paste, backpressure …).
  decisions/      Decisions in ADR style, numbered: `0001-<name>.md`.
  syntheses/      Comparisons, analyses, filed answers to questions.
  overview.md     The big picture: state of knowledge, open risks, open questions.
index.md          Catalog of all pages (organized by content).
log.md            Chronicle of all operations (append-only).
```

## Page conventions

- File names: `kebab-case.md`, ASCII.
- Links: Obsidian wikilinks `[[name]]` or `[[name|display text]]`. References to code as a
  path relative to the project root, e.g. `src-tauri/src/pty.rs:42`.
- Every page starts with YAML frontmatter:

```yaml
---
title: Readable title
type: source | component | platform | agent | concept | decision | synthesis | overview
tags: []
sources: []        # wikilinks to pages under sources/
created: YYYY-MM-DD
updated: YYYY-MM-DD
---
```

- `source`: additionally `raw: raw/<file>`, `url`, `retrieved: YYYY-MM-DD`.
- `component`: additionally `version` (verified version), `license`, `repo`.
- `decision`: additionally `status: proposed | accepted | superseded`, `superseded_by`.
  Outline: Context, Options, Decision, Consequences, Evidence.
- **Verified vs. unverified:** every statement about APIs, versions or platform behavior carries
  a source `([[source-x]])` or is marked with `> [!question] Unverified`.
  File results you measured yourself as a source under `raw/` (command, platform, version, output).
- Never resolve contradictions silently: `> [!warning] Contradiction` plus both positions.
- Version-bound: always state the version for behavior that changes between versions.
- Language: English; identifiers in their original form.

## Workflows

### Ingest (process a new source)
1. Read the source in `raw/` in full. Save URLs to `raw/` as Markdown first.
2. Briefly discuss the key points with the human; name their relevance to Hangar.
3. Create `pages/sources/<name>.md`: summary, key points, significance for Hangar.
4. Create or update the affected pages. Check new data against existing statements.
5. If the source touches a decision, update the affected `decisions/` page or
   propose a new decision as `proposed`.
6. Update `pages/overview.md`, `index.md` and `log.md`.

### Query (answer a question)
1. Read `index.md` first, then the relevant pages.
2. Answer with references to wiki pages and sources.
3. Offer to file valuable answers as `pages/syntheses/<name>.md`.
4. If the wiki is not enough: say so clearly, propose sources or a measurement experiment.

### Lint (health check)
Report first, fix after approval:
- Contradictions, outdated statements (new versions), unverified statements without follow-up work.
- Orphans, broken `[[...]]` links, pages missing from the index, missing frontmatter.
- Decisions whose evidence has changed.
- Code references whose file or line no longer matches.
- Gaps: known platform pitfalls without an evidenced wiki page.

## index.md
Organized by category. One line per page: `- [[name]]: one-liner (sources: n, updated: YYYY-MM-DD)`

## log.md
Append-only. Every entry starts with `## [YYYY-MM-DD] <ingest|query|lint|schema> | <title>`,
followed by a few lines: what was done, which pages changed.
Latest entries: `grep "^## \[" log.md | tail -5`

## Rules
- Never modify `raw/`.
- Never invent facts; label conclusions as such.
- No secrets, tokens or API keys in the wiki, not even in logs under `raw/` (mask them first).
- Commits follow Conventional Commits: `docs(wiki): ingest <title>` and so on.

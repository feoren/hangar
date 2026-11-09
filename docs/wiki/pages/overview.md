---
title: Overview
type: overview
tags: []
sources: []
created: 2026-11-07
updated: 2026-11-09
---

# Overview

No sources processed yet.

## Open questions
These points were set as assumptions at the start and still need evidence:

> [!question] Unverified
> - Target platforms and minimum versions (Windows 10 1809+ because of ConPTY, macOS 13+, Linux X11/Wayland).
> - portable-pty: resize, exit code and killing the process tree per platform (Job Objects, process groups).
> - Throughput: batching at 8 to 16 ms and backpressure at 10 MB/s of output with xterm.js.
> - macOS: load the login shell environment so that `claude`/`codex` are found on the PATH.
> - Path for config and layout (so far only "the platform's usual config path") and keychain integration.

## Restoring sessions
Workspace and scrollback live in the local app data folder; agents resume via `resume_args`
([[session-restore]], [[resume]]). Open: the scrollback is stored unencrypted in the user profile;
two sessions of the same agent in the same folder resume the same conversation.

## Release and code signing
Installer and auto update work ([[release-rollout]]). Authenticode code signing against SmartScreen is
prepared in `tools/release.py` (environment variables, verification via `Get-AuthenticodeSignature`), but
there is no certificate yet ([[code-signing-research]]). Open: whether Tauri signs the NSIS `setup.exe`,
and whether SimplySign can be automated.

## .cmd launcher scripts on Windows
npm shims get NoDefaultCurrentDirectoryInExePath; characters that are special to cmd.exe in the path or
arguments are rejected ([[cmd-launcher]]). Open: scripts that pass arguments on via `%~1`.

## Local claims (team service step A)
Hangar watches each running session's worktree and warns when two of the user's sessions change
the same file or lines; git decides, the watcher only triggers rescans ([[team-service-v1]],
[[claims-rescan-measurement]]). Open: cost and watcher limits on macOS and Linux (inotify watch
limit, FSEvents), committed but unmerged branch changes, configurable areas.

## Team relay interop (team service step F)
One run against a self-hosted relay showed the live protocol compatible and its account API
different from the website API the app speaks ([[relay-interop-measurement]],
[[hangar-account-api]]). Open: a run that includes task reservations, and an account API the
self-hosted relay and the app share.

# Measurement: Codex npm shim chain and Claude Code's `--option=value` form

Date: 2026-11-13. Platform: Windows 11 Pro (build 26300), Git Bash. No model was called.

## npm shim of Codex CLI 0.169.0

`%APPDATA%\npm\codex.cmd` (cmd-shim output, last line verbatim):

```
endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & set PATHEXT=%PATHEXT:;.JS;=;% & "%_prog%"  "%dp0%\node_modules\@openai\codex\bin\codex.js" %*
```

`%_prog%` is `node.exe` next to the shim, else `node`. The native binary sits at
`node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe`.

`bin/codex.js` (read, not run), in short: picks the target triple from `process.platform` and
`process.arch`, finds the platform package with `require.resolve("<platform package>/package.json")`
(fallback `../vendor`), joins `vendor/<triple>/bin/codex.exe`, detects the package manager (bun,
pnpm, vite-plus, otherwise npm), builds the environment with `CODEX_MANAGED_PACKAGE_ROOT` (real
path of the package) and one of `CODEX_MANAGED_BY_NPM`, `_BUN`, `_PNPM`, `_VITE_PLUS` set to `1`,
and spawns the binary with `stdio: "inherit"` (on Unix `process.execve`).

## Claude Code 2.1.323 option parsing

Run in an empty folder under `tests/.tmp/`, stdin from `/dev/null`:

```
$ claude --bogus-option=1 -p < /dev/null
Error: unknown option '--bogus-option=1'
$ claude --append-system-prompt=-dash-start -p < /dev/null
Error: Input must be provided either through stdin or as a prompt argument when using --print
```

The `=` form is parsed as the option with its value, also when the value starts with `-`; the
second call stops at the missing input, before any request.

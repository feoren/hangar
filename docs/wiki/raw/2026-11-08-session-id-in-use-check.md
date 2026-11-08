# Check: does `claude --session-id` refuse an id that already names a conversation? (2026-11-08)

- Platform: Windows 11 Pro 10.0.26300, x64; Claude Code 2.1.319 (`claude --version` prints
  `2.1.319 (Claude Code)`), native install (`~/.local/bin/claude`, a single executable with the
  bundled JavaScript).
- Method: printable strings of the executable searched with `grep -a` for `already in use` and
  for the function that the check calls. Claude was not started apart from `--version`; no
  conversation and no transcript was read.

## Argument check (verbatim excerpt, minified)

```text
let s=Nt(Ne);if(!s)return vn("Error: Invalid session ID. Must be a valid UUID.");if(!(o.forkSession&&o.resume===s)&&ypr(s))return vn(`Error: Session ID ${s} is already in use.`,BVn)
```

## The check function (verbatim excerpt, minified)

```text
function ypr(e){if(q())return!1;let n=gee(),r=Mp(n,`${e}.jsonl`),s=ie();try{return s.statSync(r),!0}catch{return!1}}
```

## Reading

- `--session-id <uuid>` is refused with `Error: Session ID <uuid> is already in use.` when
  `<uuid>.jsonl` exists in the folder `gee()` returns, except together with `--fork-session` and
  `--resume <same uuid>`.
- `gee()` is not resolved here; from the transcript layout it is most likely the project folder
  of the current working folder (`<config>/projects/<encoded cwd>/`). Unverified.
- The string `No conversation found with session ID: ` is present as well (the error of
  `--resume <unknown uuid>`, measured in 2026-11-08-resume-id-measurement.md).

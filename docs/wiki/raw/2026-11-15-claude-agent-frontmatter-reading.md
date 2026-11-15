# Claude Code agent frontmatter fields (binary reading)

- Date: 2026-11-15
- Platform: Windows 11 Pro (10.0.26300), Git Bash
- Program: Claude Code 2.1.325 (native install, `~/.local/bin/claude`)
- Method: `grep -a -o` on the binary for `permissionMode`, `mcpServers` and `frontmatter`; nothing run

Strings found (verbatim excerpts):

```
"main-thread agent":"agent", ... `frontmatter ${o==="hooks"?"hooks":"MCP servers"} for ${n} '${i}'`;t(`Skipping ${s}: the fold
Skipping ${s}: the folder its definition file came from is not trusted (source: ${e.source}).
Agent '${n}': PreToolUse/PermissionRequest is declared at the frontmatter top level, outside "hooks"
frontmatter permissionMode "${h}" ignored \u2014 it would widen the agent view's inher
```

Reading: an agent definition's frontmatter `hooks` and `mcpServers` are loaded for agents and for
the main-thread agent; both are skipped when the folder the definition came from is not trusted.
A frontmatter `permissionMode` is ignored where it would widen an inherited mode. Not measured
with a live session.

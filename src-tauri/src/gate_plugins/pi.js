
// Pi and Oh My Pi load this file with `-e`: a `tool_call` handler that can block a write or edit
// before it runs. Every error is caught here, because a throwing handler blocks the tool in Pi;
// a call that may run a shell command is then blocked with a reason, an edit goes ahead.
// Oh My Pi's `edit` in its apply_patch mode carries the patch in `input`.
const EDIT_TOOLS = {
  write: { tool: "Write", patchKeys: [] },
  edit: { tool: "Edit", patchKeys: ["input"] },
  ast_edit: { tool: "Edit", patchKeys: [] },
};

// Pi's shell tools and the dialect Hangar reads each in; both take `input.command` (Pi 1.5.2
// `dist/core/tools/bash.js` and `powershell.js`, docs/wiki/pages/sources/shell-tool-input-shapes.md).
const SHELL_TOOLS = { bash: "Bash", powershell: "PowerShell" };

export default function hangarGate(pi) {
  pi.on("tool_call", async (event, ctx) => {
    try {
      const cwd = typeof ctx?.cwd === "string" ? ctx.cwd : process.cwd();
      let verdict;
      const shell = commandTool(SHELL_TOOLS, EDIT_TOOLS, event?.toolName, event?.input);
      if (shell !== undefined) {
        verdict = await decideCommand(shell, event.input?.command, cwd, ctx?.signal);
      } else {
        const spec = lookup(EDIT_TOOLS, event?.toolName);
        if (spec === undefined) return undefined;
        const paths = editPaths(event.input, spec);
        verdict = await decideEdit(spec.tool, paths, cwd, ctx?.signal, piPath);
      }
      if (verdict.kind === "aborted") {
        return { block: true, reason: "Hangar: the tool call was cancelled." };
      }
      if (verdict.kind === "ask" && ctx?.hasUI) {
        const go = await ctx.ui.confirm("Hangar", verdict.reason);
        return go ? undefined : { block: true, reason: verdict.reason };
      }
      if (verdict.kind === "deny" || verdict.kind === "ask") {
        return { block: true, reason: verdict.reason };
      }
      if (verdict.notes.length > 0 && ctx?.hasUI) {
        ctx.ui.notify("Hangar gate: " + verdict.notes[0] + "; the tool call goes ahead", "warning");
      }
      return undefined;
    } catch {
      return failsClosed(SHELL_TOOLS, EDIT_TOOLS, event?.toolName, event?.input)
        ? { block: true, reason: UNCHECKED_COMMAND }
        : undefined;
    }
  });
}

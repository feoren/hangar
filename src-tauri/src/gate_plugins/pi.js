
// Pi and Oh My Pi load this file with `-e`: a `tool_call` handler that can block a write or edit
// before it runs. Every error is caught here, because a throwing handler blocks the tool in Pi.
// Oh My Pi's `edit` in its apply_patch mode carries the patch in `input`.
const EDIT_TOOLS = {
  write: { tool: "Write", patchKeys: [] },
  edit: { tool: "Edit", patchKeys: ["input"] },
  ast_edit: { tool: "Edit", patchKeys: [] },
};

export default function hangarGate(pi) {
  pi.on("tool_call", async (event, ctx) => {
    try {
      const spec = lookup(EDIT_TOOLS, event?.toolName);
      if (spec === undefined) return undefined;
      const cwd = typeof ctx?.cwd === "string" ? ctx.cwd : process.cwd();
      const paths = editPaths(event.input, spec);
      const verdict = await decideEdit(spec.tool, paths, cwd, ctx?.signal, piPath);
      if (verdict.kind === "aborted") {
        return { block: true, reason: "Hangar: the edit was cancelled." };
      }
      if (verdict.kind === "ask" && ctx?.hasUI) {
        const go = await ctx.ui.confirm("Hangar", verdict.reason);
        return go ? undefined : { block: true, reason: verdict.reason };
      }
      if (verdict.kind === "deny" || verdict.kind === "ask") {
        return { block: true, reason: verdict.reason };
      }
      if (verdict.notes.length > 0 && ctx?.hasUI) {
        ctx.ui.notify("Hangar gate: " + verdict.notes[0] + "; the edit goes ahead", "warning");
      }
      return undefined;
    } catch {
      return undefined;
    }
  });
}

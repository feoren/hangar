
// OpenCode loads this file as a plugin named in OPENCODE_CONFIG_CONTENT: a `tool.execute.before`
// hook. Throwing blocks the tool and hands the message to the model, so only a deny throws; every
// other error is caught. OpenCode has no "ask" here, so Hangar's "please confirm" becomes a deny
// that says so.
const EDIT_TOOLS = {
  write: { tool: "Write", patchKeys: [] },
  edit: { tool: "Edit", patchKeys: [] },
  multiedit: { tool: "Edit", patchKeys: [] },
  patch: { tool: "Edit", patchKeys: ["patchText", "patch", "input"] },
  apply_patch: { tool: "Edit", patchKeys: ["patchText", "patch", "input"] },
};

export const HangarGate = async (context) => {
  const cwd = typeof context?.directory === "string" ? context.directory : process.cwd();
  return {
    "tool.execute.before": async (input, output) => {
      let verdict;
      try {
        const spec = lookup(EDIT_TOOLS, input?.tool);
        if (spec === undefined) return;
        const paths = editPaths(output?.args, spec);
        verdict = await decideEdit(spec.tool, paths, cwd, undefined, asIs);
      } catch {
        return;
      }
      if (verdict.kind === "deny" || verdict.kind === "ask") throw new Error(verdict.reason);
    },
  };
};

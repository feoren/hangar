
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

// OpenCode's shell tool (measured: hooks see it as `bash`), with `args.command` and an optional
// `args.workdir` relative to the session's folder (OpenCode 1.19.5 binary,
// docs/wiki/pages/sources/shell-tool-input-shapes.md). On Windows OpenCode picks PowerShell or
// cmd.exe, so Hangar reads the command as "Shell", in every dialect.
const SHELL_TOOL = "bash";
const SHELL_KIND = process.platform === "win32" ? "Shell" : "Bash";

export const HangarGate = async (context) => {
  const cwd = typeof context?.directory === "string" ? context.directory : process.cwd();
  return {
    "tool.execute.before": async (input, output) => {
      let verdict;
      try {
        if (input?.tool === SHELL_TOOL) {
          const workdir = output?.args?.workdir;
          const where = typeof workdir === "string" && workdir !== "" ? absolutePath(workdir, cwd) : cwd;
          verdict = await decideCommand(SHELL_KIND, output?.args?.command, where, undefined);
        } else {
          const spec = lookup(EDIT_TOOLS, input?.tool);
          if (spec === undefined) return;
          const paths = editPaths(output?.args, spec);
          verdict = await decideEdit(spec.tool, paths, cwd, undefined, asIs);
        }
      } catch {
        return;
      }
      if (verdict.kind === "deny" || verdict.kind === "ask") throw new Error(verdict.reason);
    },
  };
};

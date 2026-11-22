// OpenCode 2.x loads this file as `index.ts` of a plugin folder named in OPENCODE_CONFIG_CONTENT
// (`plugins`), with OpenCode started `--standalone` so the session's own server reads that
// variable. The V2 API: a `tool` hook "execute.before" that runs before every tool call; throwing
// blocks the call and hands the message to the model. Only a deny or an ask throws; any other
// error lets an edit go ahead and blocks a call that may run a shell command. OpenCode resolves
// "@opencode/plugin" itself (measured with 2.3.2: nothing is installed in the folder).
// Measured with 2.3.2: `write` and `edit` carry an absolute `path`, the shell tool is `shell` with
// `command` and an absolute `workdir` (docs/wiki/pages/sources/opencode2-gate-live-measurement.md).
import { Plugin } from "@opencode/plugin";

const EDIT_TOOLS = {
  write: { tool: "Write", patchKeys: [] },
  edit: { tool: "Edit", patchKeys: [] },
  multiedit: { tool: "Edit", patchKeys: [] },
  patch: { tool: "Edit", patchKeys: ["patchText", "patch", "input"] },
  apply_patch: { tool: "Edit", patchKeys: ["patchText", "patch", "input"] },
};

// On Windows OpenCode runs PowerShell or cmd.exe, so Hangar reads the command as "Shell".
const SHELL_TOOLS = { shell: process.platform === "win32" ? "Shell" : "Bash" };

export default Plugin.define({
  id: "hangar.gate",
  async setup(ctx) {
    const cwd = typeof ctx?.directory === "string" ? ctx.directory : process.cwd();
    await ctx.tool.hook("execute.before", async (event) => {
      let verdict;
      try {
        const input = event?.input;
        const shell = commandTool(SHELL_TOOLS, EDIT_TOOLS, event?.tool, input);
        if (shell !== undefined) {
          const workdir = input?.workdir;
          const where = typeof workdir === "string" && workdir !== "" ? absolutePath(workdir, cwd) : cwd;
          verdict = await decideCommand(shell, input?.command, where, undefined);
        } else {
          const spec = lookup(EDIT_TOOLS, event?.tool);
          if (spec === undefined) return;
          verdict = await decideEdit(spec.tool, editPaths(input, spec), cwd, undefined, asIs);
        }
      } catch {
        if (failsClosed(SHELL_TOOLS, EDIT_TOOLS, event?.tool, event?.input)) {
          throw new Error(UNCHECKED_COMMAND);
        }
        return;
      }
      if (verdict.kind === "deny" || verdict.kind === "ask") throw new Error(verdict.reason);
    });
  },
});

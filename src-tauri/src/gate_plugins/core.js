// Hangar's agent gate for one session (docs/wiki/pages/syntheses/agent-gate-adapters.md).
// Hangar writes this file into the session's own folder before the start. Port and token come
// from the HANGAR_GATE variable of the session's environment only; neither is ever written here.
// Free of dependencies: only node: modules, which Node (Pi) and Bun (Oh My Pi, OpenCode) both
// provide. node:http rather than fetch, because Node's fetch sends Sec-Fetch-Mode, which Hangar's
// endpoint refuses as a request from a browser page.
// Shell commands go to the same endpoint as tool "Bash" with the command line; Hangar's command
// guard decides there whether the person is asked.
// Edits fail open: whatever goes wrong (no variable, endpoint unreachable, an answer it cannot
// read) lets the edit through, as every other hook of Hangar's gate does. A shell command whose
// question to the endpoint fails is blocked instead (the command guard fails closed); only a
// missing variable, a session without the gate, lets it through.
import { request } from "node:http";
import { homedir } from "node:os";
import { isAbsolute, resolve } from "node:path";

const GATE_ENV = "HANGAR_GATE";
const ROUTE = "/v1/pre-tool-use";
// Above Hangar's own decision time (600 s), so Hangar's answer always comes first.
const DEADLINE_MS = 630000;
const MAX_ANSWER = 1024 * 1024;
const MAX_PATHS = 64;
// Hangar's endpoint reads no longer command (command_guard.rs MAX_COMMAND_BYTES); it asks instead.
const MAX_COMMAND_BYTES = 64 * 1024;
const PATCH_MARKERS = ["*** Add File:", "*** Update File:", "*** Delete File:", "*** Move to:"];
const PATH_KEYS = ["path", "file_path", "filePath"];

function gateEndpoint() {
  const value = process.env[GATE_ENV] ?? "";
  const colon = value.indexOf(":");
  const port = Number(value.slice(0, colon));
  const token = value.slice(colon + 1);
  if (colon < 1 || !Number.isInteger(port) || port < 1 || port > 65535) return undefined;
  if (!/^[A-Za-z0-9]+$/.test(token)) return undefined;
  return { port, token };
}

// Pi's tools read a path so: a leading "@" dropped, "~" for the home folder (`resolveToCwd`).
// Other agents take the path as given.
function piPath(named) {
  const path = named.startsWith("@") ? named.slice(1) : named;
  if (path === "~") return homedir();
  if (path.startsWith("~/") || path.startsWith("~\\")) return homedir() + path.slice(1);
  return path;
}

function asIs(named) {
  return named;
}

// Relative paths count against the session's folder.
function absolutePath(path, cwd) {
  return isAbsolute(path) ? path : resolve(cwd, path);
}

// A path exactly as the tool names it, not trimmed: a file name may end in a space.
function addPath(paths, value) {
  if (typeof value !== "string" || value === "") return;
  if (paths.length <= MAX_PATHS && !paths.includes(value)) paths.push(value);
}

// Every file a patch adds, changes, deletes or moves to; the patch format pads the name.
function addPatchPaths(paths, text) {
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    const marker = PATCH_MARKERS.find((m) => line.startsWith(m));
    if (marker !== undefined) addPath(paths, line.slice(marker.length).trim());
  }
}

// Own keys only: a tool named "constructor" or "__proto__" is no edit tool.
function lookup(table, name) {
  return typeof name === "string" && Object.hasOwn(table, name) ? table[name] : undefined;
}

// The shell Hangar reads a tool call's command in: a known shell tool's, and "Shell" (every
// dialect) for any other tool with a string `command` that is no edit tool, so a shell tool
// Hangar does not know by name is still checked. Undefined for a tool that runs no command.
function commandTool(shellTools, editTools, name, input) {
  const known = lookup(shellTools, name);
  if (known !== undefined) return known;
  if (lookup(editTools, name) !== undefined) return undefined;
  return input !== null && typeof input === "object" && typeof input.command === "string"
    ? "Shell"
    : undefined;
}

// After the handler itself failed: whether the call may run a shell command, which is then
// blocked rather than let through (the command guard fails closed, edits fail open). A known
// shell tool, any tool with a string `command`, and one whose input cannot even be read count.
function failsClosed(shellTools, editTools, name, input) {
  if (lookup(editTools, name) !== undefined) return false;
  if (lookup(shellTools, name) !== undefined) return true;
  try {
    return input !== null && typeof input === "object" && typeof input.command === "string";
  } catch {
    return true;
  }
}

const UNCHECKED_COMMAND =
  "Hangar: the command guard could not check this command. Retry it in a moment.";

// The files a write or edit tool names: a path field, a list of paths, or patch text in the
// tool's own patch fields (`spec.patchKeys`), never in file content. At most one more than
// MAX_PATHS, enough to know a call names too many.
function editPaths(input, spec) {
  const paths = [];
  if (input === null || typeof input !== "object") return paths;
  for (const key of PATH_KEYS) addPath(paths, input[key]);
  if (Array.isArray(input.paths)) for (const path of input.paths) addPath(paths, path);
  for (const key of spec.patchKeys) {
    const value = input[key];
    if (typeof value === "string" && value.includes("*** Begin Patch")) addPatchPaths(paths, value);
  }
  return paths;
}

function verdictOf(text) {
  if (text.trim() === "") return { kind: "allow" };
  const output = JSON.parse(text)?.hookSpecificOutput ?? {};
  const reason =
    typeof output.permissionDecisionReason === "string"
      ? output.permissionDecisionReason
      : "Hangar: another session is changing this file.";
  if (output.permissionDecision === "deny") return { kind: "deny", reason };
  if (output.permissionDecision === "ask") return { kind: "ask", reason };
  return { kind: "allow" };
}

// One question to the endpoint, the same request Claude Code's HTTP hook sends. Never rejects.
function askGate(gate, tool, toolInput, cwd, signal) {
  return new Promise((done) => {
    let settled = false;
    let call;
    const finish = (verdict) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      signal?.removeEventListener?.("abort", onAbort);
      done(verdict);
    };
    const failed = (note) => {
      call?.destroy();
      finish({ kind: "error", note });
    };
    const onAbort = () => {
      call?.destroy();
      finish({ kind: "aborted" });
    };
    const timer = setTimeout(() => failed("no answer in time"), DEADLINE_MS);
    if (signal?.aborted) return onAbort();
    signal?.addEventListener?.("abort", onAbort, { once: true });
    const body = JSON.stringify({
      hook_event_name: "PreToolUse",
      tool_name: tool,
      tool_input: toolInput,
      cwd,
    });
    try {
      call = request(
        {
          host: "127.0.0.1",
          port: gate.port,
          method: "POST",
          path: ROUTE,
          agent: false,
          headers: {
            "Content-Type": "application/json",
            "Content-Length": Buffer.byteLength(body),
            Authorization: "Bearer " + gate.token,
          },
        },
        (answer) => {
          const chunks = [];
          let size = 0;
          answer.on("data", (chunk) => {
            size += chunk.length;
            if (size > MAX_ANSWER) failed("answer too large");
            else chunks.push(chunk);
          });
          answer.on("error", () => failed("answer cut off"));
          answer.on("end", () => {
            if (answer.statusCode !== 200) return failed("status " + answer.statusCode);
            try {
              finish(verdictOf(Buffer.concat(chunks).toString("utf8")));
            } catch {
              failed("unreadable answer");
            }
          });
        },
      );
      call.on("error", () => failed("endpoint not reachable"));
      call.end(body);
    } catch {
      failed("request not sent");
    }
  });
}

// Asks about every file of one tool call, one after the other; the first deny, ask or abort
// decides. `notes` lists what could not be asked about (those files go ahead).
async function decideEdit(tool, paths, cwd, signal, normalize) {
  const notes = [];
  if (paths.length === 0) return { kind: "allow", notes };
  if (paths.length > MAX_PATHS) {
    return {
      kind: "ask",
      reason:
        "Hangar: this edit touches more than " +
        MAX_PATHS +
        " files, more than the gate checks one by one. Split it into smaller edits, or ask the user to confirm.",
      notes,
    };
  }
  const gate = gateEndpoint();
  if (gate === undefined) return { kind: "allow", notes: [GATE_ENV + " is missing or malformed"] };
  for (const named of paths) {
    const filePath = absolutePath(normalize(named), cwd);
    const verdict = await askGate(gate, tool, { file_path: filePath }, cwd, signal);
    if (verdict.kind === "error") notes.push(verdict.note);
    else if (verdict.kind !== "allow") return { ...verdict, notes };
  }
  return { kind: "allow", notes };
}

// Asks about one shell command; anything but a non-empty string goes ahead unasked. `tool` names
// the shell for Hangar's reading: "Bash", "PowerShell", or "Shell" when it is not known.
async function decideCommand(tool, command, cwd, signal) {
  if (typeof command !== "string" || command.trim() === "") return { kind: "allow", notes: [] };
  const gate = gateEndpoint();
  if (gate === undefined) return { kind: "allow", notes: [GATE_ENV + " is missing or malformed"] };
  // A huge command is cut to twice the endpoint's limit: still over it, so the endpoint answers
  // "ask" as for the whole command, without sending megabytes.
  const limit = MAX_COMMAND_BYTES * 2;
  const text = Buffer.byteLength(command) > limit ? command.slice(0, limit) : command;
  const verdict = await askGate(gate, tool, { command: text }, cwd, signal);
  // A command the guard could not check is blocked, not let through: a client that floods the
  // endpoint must not be able to wave a command past it. The session runs inside Hangar, so the
  // endpoint is there while the agent is.
  if (verdict.kind === "error") {
    return {
      kind: "deny",
      reason:
        "Hangar: the command guard could not check this command (" +
        verdict.note +
        "). Retry it in a moment.",
      notes: [verdict.note],
    };
  }
  return { ...verdict, notes: [] };
}

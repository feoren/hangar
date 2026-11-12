// Hangar's agent gate for one session (docs/wiki/pages/syntheses/agent-gate-adapters.md).
// Hangar writes this file into the session's own folder before the start. Port and token come
// from the HANGAR_GATE variable of the session's environment only; neither is ever written here.
// Free of dependencies: only node: modules, which Node (Pi) and Bun (Oh My Pi, OpenCode) both
// provide. node:http rather than fetch, because Node's fetch sends Sec-Fetch-Mode, which Hangar's
// endpoint refuses as a request from a browser page.
// Fails open: whatever goes wrong (no variable, endpoint unreachable, an answer it cannot read)
// lets the edit through, as every other hook of Hangar's gate does. The gate coordinates the
// user's own sessions; it is not a security boundary.
import { request } from "node:http";
import { homedir } from "node:os";
import { isAbsolute, resolve } from "node:path";

const GATE_ENV = "HANGAR_GATE";
const ROUTE = "/v1/pre-tool-use";
// Above Hangar's own decision time (600 s), so Hangar's answer always comes first.
const DEADLINE_MS = 630000;
const MAX_ANSWER = 1024 * 1024;
const MAX_PATHS = 64;
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
function askGate(gate, tool, path, cwd, signal) {
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
      tool_input: { file_path: path },
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
    const verdict = await askGate(gate, tool, absolutePath(normalize(named), cwd), cwd, signal);
    if (verdict.kind === "error") notes.push(verdict.note);
    else if (verdict.kind !== "allow") return { ...verdict, notes };
  }
  return { kind: "allow", notes };
}

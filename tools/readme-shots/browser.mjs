// Finds a Chromium based browser (Edge, Chrome, Chromium) and drives it headless over the Chrome
// DevTools Protocol. No dependencies: Node's built-in fetch and WebSocket are enough.
import { spawn, spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

/** Candidate executables per platform, in order of preference. */
function candidates() {
  if (process.platform === "win32") {
    const roots = [
      process.env["PROGRAMFILES(X86)"],
      process.env.PROGRAMFILES,
      process.env.LOCALAPPDATA,
    ].filter((root) => typeof root === "string" && root !== "");
    const relative = [
      "Microsoft/Edge/Application/msedge.exe",
      "Google/Chrome/Application/chrome.exe",
      "Chromium/Application/chrome.exe",
    ];
    return relative.flatMap((rel) => roots.map((root) => path.join(root, rel)));
  }
  if (process.platform === "darwin") {
    const apps = [
      "Google Chrome.app/Contents/MacOS/Google Chrome",
      "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
      "Chromium.app/Contents/MacOS/Chromium",
    ];
    const home = process.env.HOME ?? "";
    return apps.flatMap((app) => [
      path.join("/Applications", app),
      path.join(home, "Applications", app),
    ]);
  }
  return ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "microsoft-edge"]
    .map(onPath)
    .filter((found) => found !== null);
}

/** Absolute path of a program on PATH, or null. */
function onPath(name) {
  const result = spawnSync("which", [name], { encoding: "utf8" });
  const found = result.status === 0 ? result.stdout.trim() : "";
  return found === "" ? null : found;
}

/**
 * The browser executable: `README_SHOTS_BROWSER` if set, otherwise the first installed candidate.
 * Throws with a clear message if none is found.
 */
export function findBrowser() {
  const override = process.env.README_SHOTS_BROWSER;
  if (override) {
    if (!existsSync(override))
      throw new Error(`README_SHOTS_BROWSER points to ${override}, which does not exist.`);
    return override;
  }
  const found = candidates().find((candidate) => existsSync(candidate));
  if (!found) {
    throw new Error(
      "No Chromium based browser found (Microsoft Edge, Google Chrome or Chromium). " +
        "Install one or set README_SHOTS_BROWSER to its executable.",
    );
  }
  return found;
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** Waits until `check` returns a truthy value, polling every `interval` ms. */
export async function waitFor(
  check,
  { timeout = 20_000, interval = 100, what = "condition" } = {},
) {
  const deadline = Date.now() + timeout;
  for (;;) {
    const value = await check();
    if (value) return value;
    if (Date.now() > deadline) throw new Error(`Timed out waiting for ${what}.`);
    await sleep(interval);
  }
}

/** Minimal CDP connection over one WebSocket. */
async function connect(url) {
  const ws = new WebSocket(url);
  await new Promise((resolve, reject) => {
    ws.onopen = resolve;
    ws.onerror = () => reject(new Error(`Cannot connect to ${url}`));
  });
  let nextId = 0;
  const pending = new Map();
  const listeners = new Map();
  ws.onmessage = (message) => {
    const data = JSON.parse(String(message.data));
    if (data.id !== undefined && pending.has(data.id)) {
      const { resolve, reject } = pending.get(data.id);
      pending.delete(data.id);
      if (data.error) reject(new Error(`${data.error.message} (${data.error.code})`));
      else resolve(data.result);
    } else if (data.method) {
      for (const listener of listeners.get(data.method) ?? []) listener(data.params);
    }
  };
  return {
    send(method, params = {}) {
      return new Promise((resolve, reject) => {
        const id = ++nextId;
        pending.set(id, { resolve, reject });
        ws.send(JSON.stringify({ id, method, params }));
      });
    },
    once(method) {
      return new Promise((resolve) => {
        const list = listeners.get(method) ?? [];
        const listener = (params) => {
          list.splice(list.indexOf(listener), 1);
          resolve(params);
        };
        list.push(listener);
        listeners.set(method, list);
      });
    },
    on(method, listener) {
      listeners.set(method, [...(listeners.get(method) ?? []), listener]);
    },
    close() {
      ws.close();
    },
  };
}

/**
 * Starts the browser headless with its own profile in `profileDir` (never the user's profile) and
 * returns a driver for its first page.
 */
export async function launchBrowser(executable, profileDir) {
  const proc = spawn(
    executable,
    [
      "--headless=new",
      "--remote-debugging-port=0",
      `--user-data-dir=${profileDir}`,
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-extensions",
      "--disable-sync",
      "--disable-gpu",
      "--hide-scrollbars",
      "--mute-audio",
      "--force-color-profile=srgb",
      "about:blank",
    ],
    { stdio: "ignore" },
  );
  const exited = new Promise((resolve) => proc.once("exit", resolve));
  proc.once("error", () => undefined);

  // With port 0 the browser picks a free port and writes it to DevToolsActivePort.
  const portFile = path.join(profileDir, "DevToolsActivePort");
  const [port, browserPath] = await waitFor(
    () => {
      // Exit code 0 is fine: Edge may relaunch itself and hand the work to a new process, which
      // then still writes the port file.
      if (proc.exitCode !== null && proc.exitCode !== 0)
        throw new Error(`The browser exited early (code ${proc.exitCode}).`);
      if (!existsSync(portFile)) return null;
      const content = readFileSync(portFile, "utf8").split(/\r?\n/);
      return content.length >= 2 && content[1] ? content : null;
    },
    { what: "the browser's DevTools port" },
  );
  const base = `http://127.0.0.1:${port}`;
  const page = await waitFor(
    async () => {
      const targets = await (await fetch(`${base}/json/list`)).json();
      return targets.find((target) => target.type === "page") ?? null;
    },
    { what: "the browser's first page" },
  );
  const browser = await connect(`ws://127.0.0.1:${port}${browserPath}`);
  const cdp = await connect(page.webSocketDebuggerUrl);
  const logs = [];
  cdp.on("Runtime.consoleAPICalled", (params) => {
    logs.push(`${params.type}: ${params.args.map((a) => a.value ?? a.description).join(" ")}`);
  });
  cdp.on("Runtime.exceptionThrown", (params) => {
    const details = params.exceptionDetails;
    logs.push(`exception: ${details.exception?.description ?? details.text}`);
  });
  await cdp.send("Runtime.enable");
  await cdp.send("Page.enable");

  return {
    logs,
    send: cdp.send,
    async goto(url) {
      const loaded = cdp.once("Page.loadEventFired");
      await cdp.send("Page.navigate", { url });
      await loaded;
    },
    // Runs an expression in the page (Runtime.evaluate) and returns its JSON value.
    async evaluate(expression) {
      const result = await cdp.send("Runtime.evaluate", {
        expression,
        awaitPromise: true,
        returnByValue: true,
      });
      if (result.exceptionDetails) {
        throw new Error(
          `Page script failed: ${result.exceptionDetails.exception?.description ?? result.exceptionDetails.text}`,
        );
      }
      return result.result.value;
    },
    async screenshot() {
      const result = await cdp.send("Page.captureScreenshot", { format: "png", fromSurface: true });
      return Buffer.from(result.data, "base64");
    },
    /** Asks the browser to quit, kills it if it does not, and waits until it is gone. */
    async close() {
      cdp.close();
      // Sent unconditionally: it also reaches a relaunched browser this process no longer owns.
      await browser.send("Browser.close").catch(() => undefined);
      if (proc.exitCode === null) {
        const quit = await Promise.race([exited.then(() => true), sleep(5000).then(() => false)]);
        if (!quit) {
          proc.kill();
          await exited;
        }
      }
      browser.close();
    },
  };
}

export { sleep };

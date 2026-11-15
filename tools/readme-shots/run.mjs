// Renders the README screenshots into docs/images from the real frontend with a mocked backend
// (mock.ts) in a headless Chromium based browser: one image per scene and color scheme, e.g.
// hangar-dark.png (the overview) and hangar-team-light.png.
//
// Usage: pnpm readme:shots [--scene <name>]...   (default: every scene)
//
// Starts its own Vite server on a free port, never touches a running Hangar, and keeps all
// temporary files (browser profile, Vite cache) under tests/.tmp/readme-shots, which it removes at
// the end.
import react from "@vitejs/plugin-react";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer as createNetServer } from "node:net";
import path from "node:path";
import { createServer } from "vite";

import { findBrowser, launchBrowser, sleep, waitFor } from "./browser.mjs";
import { optimizePng, pngSize } from "./png.mjs";

const HERE = import.meta.dirname;
const REPO = path.resolve(HERE, "../..");
const WORK = path.join(REPO, "tests/.tmp/readme-shots");
const OUT_DIR = path.join(REPO, "docs/images");

const WIDTH = 1296;
const HEIGHT = 839;
// Wide enough for the version and its beta badge in the sidebar header, and for a session row's
// second line with its branch and context meter.
const SIDEBAR_WIDTH = 352;
const KB = 1024;
const SCHEMES = ["dark", "light"];
const TITLE = "README-SHOTS-HARNESS";

/** Clicks the first element matching `selector` whose text contains `text` (if given). */
const click = (selector, text = null) => `(() => {
  const all = [...document.querySelectorAll(${JSON.stringify(selector)})];
  const target = all.find((el) => ${JSON.stringify(text)} === null || el.textContent.includes(${JSON.stringify(text)}));
  if (!target) throw new Error("Nothing to click: " + ${JSON.stringify(`${selector} ${text ?? ""}`)});
  target.click();
  return true;
})()`;

/** True once an element matching `selector` exists whose text contains `text` (if given). */
const shows = (selector, text = null) => `(() => {
  const all = [...document.querySelectorAll(${JSON.stringify(selector)})];
  return all.some((el) => ${JSON.stringify(text)} === null || el.textContent.includes(${JSON.stringify(text)}));
})()`;

/** True once at least `count` sidebar rows say their session is working. */
const working = (count) => `(() => {
  const rows = [...document.querySelectorAll(".session")];
  return rows.filter((row) => row.textContent.includes("working")).length >= ${count};
})()`;

/**
 * Every scene waits for the same sidebar first: Claude Code and Codex CLI working, and Gemini CLI
 * done (it went quiet after its answer). Aider asks from the start, PowerShell rests.
 */
const SETTLED = `(${working(2)} && ${shows(".session", "done")})`;

/** Takes the focus away from where a click left it, so no focus ring shows. */
const BLUR = `(() => {
  if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
  return true;
})()`;

/**
 * Scrolls the element into view, aligned to the bottom of its scrolling parent. In Settings this
 * brings the terminal preview, the color schemes and the fonts into one view.
 */
const reveal = (selector) => `(() => {
  const element = document.querySelector(${JSON.stringify(selector)});
  if (!element) return false;
  element.scrollIntoView({ block: "end", behavior: "instant" });
  return true;
})()`;

/**
 * The scenes. `file`: the image name without scheme; `layout`: changes to the stored layout;
 * `steps`: what to run once the app is up, each followed by what must show before the next.
 */
const SCENES = [
  {
    // Three sessions side by side with a dragged splitter, every row state in the sidebar.
    name: "overview",
    file: "hangar",
    maxBytes: 400 * KB,
    layout: {
      snapLayout: "mainStack",
      paneRatios: { mainStack: [0.55, 0.52] },
      // Room for every session row; the launcher shows in the other scenes.
      closedGroups: ["newSession"],
    },
    steps: [{ ready: shows(".subagent-node", "tester") }],
  },
  {
    name: "subagent",
    file: "hangar-subagent",
    maxBytes: 300 * KB,
    layout: { closedGroups: ["newSession"] },
    steps: [
      { ready: shows(".subagent-node", "tester") },
      {
        run: click(".subagent-node", "tester"),
        ready: shows(".subagent-log .log-line", "Checking the timing"),
      },
    ],
  },
  {
    // The Files tab with the folders open where this session and the teammates work.
    name: "files",
    file: "hangar-files",
    maxBytes: 300 * KB,
    layout: {
      snapLayout: "single",
      sideOpen: true,
      sideTab: "files",
      notesWidth: 360,
      closedGroups: ["newSession"],
    },
    steps: [
      { ready: shows(".team-pill", "online") },
      { ready: shows(".file-tree .tree-item", "src") },
      { run: click(".file-tree .tree-item", "src"), ready: shows(".tree-item", "payments") },
      { run: click(".tree-item", "payments"), ready: shows(".tree-item", "webhook.ts") },
      { run: click(".tree-item", "emails"), ready: shows(".tree-item", "checkout.de.json") },
      { run: click(".tree-item", "search"), ready: shows(".tree-item", "rank.ts") },
      { run: click(".tree-item", "webhook.ts"), ready: shows(".tree-item.selected", "webhook.ts") },
      { ready: shows(".tree-people") },
      { run: BLUR, ready: "true" },
    ],
  },
  {
    name: "team",
    file: "hangar-team",
    maxBytes: 300 * KB,
    // A narrower sidebar keeps the teammates' avatars on the worktree row clear of the window.
    layout: { sidebarWidth: 300 },
    steps: [
      { ready: shows(".team-pill", "online") },
      { run: click(".team-pill"), ready: shows(".team-window .team-peer", "Jonas Berg") },
    ],
  },
  {
    // The panel opens on the private notes; the Shared side shows the notes kept in the repository.
    name: "notes",
    file: "hangar-notes",
    maxBytes: 300 * KB,
    layout: { snapLayout: "single", sideOpen: true, sideTab: "notes" },
    steps: [
      { ready: shows(".notes-sides .segment", "Shared") },
      {
        run: click(".notes-sides .segment", "Shared"),
        ready: shows(".notes", "Release checklist"),
      },
    ],
  },
  {
    name: "settings",
    file: "hangar-settings",
    maxBytes: 300 * KB,
    steps: [
      { run: click(".footer-settings"), ready: shows("[role=tab]", "Appearance") },
      { run: click("[role=tab]", "Appearance"), ready: shows(".scheme-picker") },
      { run: BLUR, ready: reveal(".font-picker") },
    ],
  },
];

const readJson = (file) => JSON.parse(readFileSync(path.join(REPO, file), "utf8"));

/**
 * The version as the app would show it. tauri.conf.json is what the app reports at runtime; a
 * release (tools/release.py) writes X.Y.Z+Build there, a dev state only X.Y.Z, which then gets the
 * build number from Git the way vite.config.ts does it (ADR 0002).
 */
function appVersion() {
  const tauri = readJson("src-tauri/tauri.conf.json").version;
  const pkg = readJson("package.json");
  if (typeof tauri !== "string" || tauri !== pkg.version) {
    throw new Error(
      `Version mismatch: tauri.conf.json has ${tauri}, package.json has ${pkg.version}.`,
    );
  }
  const [base, build] = tauri.split("+");
  if (build !== undefined) return { base, version: tauri, build: null };
  const count = Number(
    execFileSync("git", ["rev-list", "--count", "HEAD"], { cwd: REPO, encoding: "utf8" }).trim(),
  );
  const offset = Number(pkg.buildNumberBase ?? 0);
  const valid = Number.isInteger(count) && count > 0 && Number.isInteger(offset) && offset >= 0;
  return { base, version: tauri, build: valid ? offset + count : null };
}

function freePort() {
  return new Promise((resolve, reject) => {
    const server = createNetServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

async function startVite(version) {
  const port = await freePort();
  const server = await createServer({
    configFile: false,
    root: HERE,
    plugins: [react()],
    define: {
      __HANGAR_BUILD__: JSON.stringify(version.build),
      __README_SHOTS_VERSION__: JSON.stringify(version.version),
    },
    cacheDir: path.join(WORK, "vite-cache"),
    clearScreen: false,
    logLevel: "warn",
    server: {
      host: "127.0.0.1",
      port,
      strictPort: true,
      hmr: false,
      watch: null,
      fs: { allow: [REPO] },
    },
  });
  await server.listen();
  return { server, url: `http://127.0.0.1:${port}/` };
}

/**
 * Stops every CSS animation on a fixed frame: finite ones (dialogs opening) at their end, endless
 * ones at a fixed point of their cycle. The active row's drifting tint stops where its light
 * band sits nicely inside the row.
 */
const FREEZE_ANIMATIONS = `(() => {
  for (const animation of document.getAnimations()) {
    const timing = animation.effect?.getComputedTiming();
    const duration = typeof timing?.duration === "number" ? timing.duration : 0;
    if (timing?.iterations !== Infinity) {
      animation.finish();
      continue;
    }
    animation.pause();
    const point = animation.animationName === "active-drift" ? 0.35 : 0.5;
    animation.currentTime = duration * point;
  }
  return true;
})()`;

/** Loads one scene in one color scheme and returns the raw screenshot. */
async function render(page, url, scene, scheme, version) {
  await page.send("Emulation.setDeviceMetricsOverride", {
    width: WIDTH,
    height: HEIGHT,
    deviceScaleFactor: 1,
    mobile: false,
  });
  await page.send("Emulation.setFocusEmulationEnabled", { enabled: true });
  await page.send("Emulation.setTimezoneOverride", { timezoneId: "UTC" }).catch(() => undefined);
  await page.send("Emulation.setEmulatedMedia", {
    features: [
      { name: "prefers-color-scheme", value: scheme },
      { name: "prefers-reduced-motion", value: "no-preference" },
    ],
  });

  // First load only to reach the origin's storage, then start the app with fixed settings.
  await page.goto(url);
  if ((await page.evaluate("document.title")) !== TITLE) {
    throw new Error(`${url} does not serve the screenshot harness.`);
  }
  const storage = {
    "hangar.layout": {
      snapLayout: "pair",
      sidebarWidth: SIDEBAR_WIDTH,
      fontSize: 14,
      ...scene.layout,
    },
    "hangar.settings": {
      language: "en",
      theme: "system",
      cursorBlink: false,
      subAgentExcerpts: true,
    },
  };
  await page.evaluate(`(() => {
    localStorage.clear();
    const entries = ${JSON.stringify(storage)};
    for (const [key, value] of Object.entries(entries)) localStorage.setItem(key, JSON.stringify(value));
    // The running version counts as seen, so "What's new" stays closed.
    localStorage.setItem("hangar.lastSeenVersion", ${JSON.stringify(version.base)});
  })()`);
  await page.goto(url);

  const shown =
    version.build === null ? version.version.replace("+", "-") : `${version.base}-${version.build}`;
  await waitFor(
    () =>
      page.evaluate(`(() => {
        const sessions = document.querySelectorAll(".session");
        const text = document.body.innerText;
        const version = document.querySelector(".brand-version");
        return (
          sessions.length === 5 &&
          text.includes("asks") &&
          text.includes("working") &&
          version !== null &&
          version.textContent.includes(${JSON.stringify(shown)})
        );
      })()`),
    { what: `five sessions, agents working, Aider asking and version ${shown}` },
  );
  await page.evaluate("document.fonts.ready.then(() => true)");
  // The version (with its beta badge) must be readable, not cut off with an ellipsis. If this
  // fails, raise SIDEBAR_WIDTH.
  const missing = await page.evaluate(`(() => {
    const element = document.querySelector(".brand-version");
    return element.scrollWidth - element.clientWidth;
  })()`);
  if (missing > 0) {
    throw new Error(`The version ${shown} is cut off in the sidebar header by ${missing} px.`);
  }
  for (const [index, step] of [{ ready: SETTLED }, ...scene.steps].entries()) {
    if (step.run) await page.evaluate(step.run);
    await waitFor(() => page.evaluate(step.ready), {
      what: index === 0 ? "the settled sidebar" : `step ${index} of the scene ${scene.name}`,
    });
  }
  // Let the terminals settle, then move the pointer away, stop the spinners and the clock, and
  // freeze every animation on a fixed frame.
  await sleep(1000);
  await page.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: WIDTH - 2, y: HEIGHT - 2 });
  await page.evaluate("window.__readmeShots.freeze = true");
  // The app's clocks tick once a second; let them pick up the frozen time.
  await sleep(1200);
  await page.evaluate(FREEZE_ANIMATIONS);
  await sleep(200);
  return page.screenshot();
}

/** Scene names from `--scene a --scene b` or `--scene a,b`; every scene without the option. */
function chosenScenes(argv) {
  const names = [];
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--scene") names.push(...(argv[(i += 1)] ?? "").split(","));
    else if (arg.startsWith("--scene=")) names.push(...arg.slice(8).split(","));
    else throw new Error(`Unknown argument ${arg}. Usage: run.mjs [--scene <name>]...`);
  }
  if (names.length === 0) return SCENES;
  const known = SCENES.map((scene) => scene.name);
  const unknown = names.filter((name) => !known.includes(name));
  if (unknown.length > 0) {
    throw new Error(`Unknown scene ${unknown.join(", ")}. Known: ${known.join(", ")}.`);
  }
  return SCENES.filter((scene) => names.includes(scene.name));
}

function removeWork() {
  try {
    rmSync(WORK, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 });
  } catch (error) {
    console.warn(`Could not remove ${WORK}: ${error instanceof Error ? error.message : error}`);
  }
}

async function main() {
  const scenes = chosenScenes(process.argv.slice(2));
  const version = appVersion();
  const browserPath = findBrowser();
  rmSync(WORK, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  mkdirSync(WORK, { recursive: true });

  let vite = null;
  let page = null;
  try {
    vite = await startVite(version);
    console.log(`Vite ${vite.url}, browser ${browserPath}, version ${version.version}`);
    page = await launchBrowser(browserPath, path.join(WORK, "profile"));
    for (const scene of scenes) {
      for (const scheme of SCHEMES) {
        const raw = await render(page, vite.url, scene, scheme, version);
        const optimized = optimizePng(raw);
        const { width, height } = pngSize(optimized);
        const name = `${scene.file}-${scheme}.png`;
        if (width !== WIDTH || height !== HEIGHT) {
          throw new Error(`${name}: got ${width} x ${height}, expected ${WIDTH} x ${HEIGHT}.`);
        }
        if (optimized.length > scene.maxBytes) {
          throw new Error(
            `${name}: ${optimized.length} bytes, more than the ${scene.maxBytes} allowed.`,
          );
        }
        const target = path.join(OUT_DIR, name);
        writeFileSync(target, optimized);
        console.log(
          `${path.relative(REPO, target)}: ${width} x ${height}, ${Math.round(optimized.length / KB)} KB` +
            ` (raw ${Math.round(raw.length / KB)} KB)`,
        );
      }
    }
    const problems = page.logs.filter((line) => /^(error|exception)|unmocked/i.test(line));
    if (problems.length > 0) console.warn(`Page reported:\n  ${problems.join("\n  ")}`);
  } finally {
    await page?.close().catch(() => undefined);
    await vite?.server.close().catch(() => undefined);
    removeWork();
  }
}

main().catch((error) => {
  console.error(`readme-shots: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
});

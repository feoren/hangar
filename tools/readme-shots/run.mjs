// Renders the README screenshots into docs/images from the real frontend with a mocked backend
// (mock.ts) in a headless Chromium based browser: one image per scene and color scheme, e.g.
// hangar-dark.png (the overview) and hangar-home-light.png.
//
// Usage: pnpm readme:shots [--scene <name>]...   (default: every scene)
//
// Starts its own Vite server on a free port, never touches a running Hangar, and keeps all
// temporary files (browser profile, Vite cache) under tests/.tmp/readme-shots, which it removes at
// the end. A scene that gets stuck leaves the page as it stood in
// tests/.tmp/readme-shots-failed-<scene>.png.
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
 * done (it went quiet after its answer). The security session asks, PowerShell rests. A scene can
 * wait for something else instead (`settled`).
 */
const SETTLED = `(${working(2)} && ${shows(".session", "done")})`;

/** The tree flow in its default style, without the rates at the rows' ends. */
const FLOW = { treeFlow: { enabled: true, style: "stream", showRate: false } };

/** Takes the focus away from where a click left it, so no focus ring shows. */
const BLUR = `(() => {
  if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
  return true;
})()`;

/** Types `value` into the text field matching `selector`, as a person would. */
const typeInto = (selector, value) => `(() => {
  const field = document.querySelector(${JSON.stringify(selector)});
  if (!field) throw new Error("No field " + ${JSON.stringify(selector)});
  const proto = field instanceof HTMLTextAreaElement ? HTMLTextAreaElement : HTMLInputElement;
  Object.getOwnPropertyDescriptor(proto.prototype, "value").set.call(field, ${JSON.stringify(value)});
  field.dispatchEvent(new Event("input", { bubbles: true }));
  return true;
})()`;

/** Right-clicks the first element matching `selector` whose text contains `text`. */
const contextMenu = (selector, text) => `(() => {
  const all = [...document.querySelectorAll(${JSON.stringify(selector)})];
  const target = all.find((el) => el.textContent.includes(${JSON.stringify(text)}));
  if (!target) throw new Error("Nothing to right-click: " + ${JSON.stringify(`${selector} ${text}`)});
  const box = target.getBoundingClientRect();
  target.dispatchEvent(
    new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      button: 2,
      clientX: box.left + 24,
      clientY: box.top + box.height / 2,
    }),
  );
  return true;
})()`;

/**
 * What the app shows once it is up, unless a scene says otherwise: `count` sessions, an agent
 * working and the version in the sidebar header.
 */
const BOOT = (version, count = 5) => `(() => {
  const sessions = document.querySelectorAll(".session");
  const text = document.body.innerText;
  const version = document.querySelector(".brand-version");
  return (
    sessions.length === ${count} &&
    text.includes("working") &&
    version !== null &&
    version.textContent.includes(${JSON.stringify(version)})
  );
})()`;

/**
 * The sidebar layout most scenes use: one terminal, "Recently opened" and "New session" folded, so
 * every session row stands whole.
 */
const FOLDED = ["recentlyOpened", "newSession"];
const SINGLE = { snapLayout: "single", closedGroups: FOLDED };

/**
 * The scenes, in README order. `file`: the image name without scheme; `maxBytes`: the size limit
 * of one image; `layout` and `settings`: changes to the stored layout and settings; `settled`: what
 * the sidebar must show before the steps start (default: SETTLED); `boot`: what must show for the
 * app to count as started, given the version shown (default: BOOT); `steps`: what to run once the
 * app is up, each followed by what must show before the next; `hover`: the element the pointer
 * rests on for the screenshot (default: none, the pointer leaves the window). The mock reads the
 * scene's name too (mock.ts `SCENE`).
 */
const SCENES = [
  {
    // The hero: the bar above the work area with the session's name, the support badge and the
    // panel switch; "Recently opened" above New session; the link group's chain and light flowing
    // to the working sessions; the asking session that has waited for 12 minutes; the lead with
    // its crew of three of Hangar's own sub-agents; the notes beside it in the right panel.
    name: "overview",
    file: "hangar",
    maxBytes: 300 * KB,
    layout: {
      ...SINGLE,
      closedGroups: ["newSession"],
      sideOpen: true,
      sideTab: "notes",
      notesWidth: 330,
    },
    settings: FLOW,
    boot: (version) => BOOT(version, 4),
    settled: `(${working(2)} && ${shows(".session", "done")} && ${shows(".session", "waiting 12")})`,
    steps: [
      {
        ready: `(document.querySelectorAll(".crew-item").length === 3 &&
          document.querySelectorAll(".recent-row").length === 3 &&
          document.querySelectorAll(".note-row").length === 3)`,
      },
    ],
  },
  {
    // "Recently opened": five programs started in other folders, newest first, one click from a
    // fresh start with their conversation; the pointer rests on one, so its × shows.
    name: "recently-opened",
    file: "hangar-recently-opened",
    maxBytes: 300 * KB,
    layout: { snapLayout: "pair", paneRatios: { pair: [0.55] }, closedGroups: ["newSession"] },
    boot: (version) => BOOT(version, 3),
    settled: `(${working(2)} && document.querySelectorAll(".recent-row").length === 5)`,
    steps: [],
    hover: { selector: ".recent-row", index: 1 },
  },
  {
    // The crew beside the lead: each sub-agent on a round stage whose light ring shows its state:
    // the manager done, the security agent searching, the researcher just started, typing dots.
    name: "sub-agents",
    file: "hangar-sub-agents",
    maxBytes: 300 * KB,
    layout: { snapLayout: "pair", paneRatios: { pair: [0.61] }, closedGroups: FOLDED },
    steps: [
      {
        ready: `(document.querySelectorAll(".crew-item").length === 3 &&
          document.querySelector(".crew-item.state-done") !== null &&
          document.querySelector(".crew-typing") !== null)`,
      },
    ],
  },
  {
    // The home view: the sessions step aside, the year's commits as a calendar with weekday
    // names and a legend, and below it the projects.
    name: "home",
    file: "hangar-home",
    maxBytes: 300 * KB,
    layout: SINGLE,
    steps: [
      {
        run: click(".sidebar-home"),
        ready: `(${shows(".home-back")} && document.querySelectorAll(".activity-day.level-4").length > 8)`,
      },
      { run: BLUR, ready: "true" },
    ],
  },
  {
    // Two reports of the link group wait below the lead's row: one with "Send", "Paste only" and
    // the close mark, one set to send, which waits for the busy lead to pause and can still be
    // cancelled.
    name: "waiting-message",
    file: "hangar-waiting-message",
    maxBytes: 300 * KB,
    layout: SINGLE,
    steps: [
      {
        // The cards take no click for a moment after they appear (useArmDelay.ts).
        ready: `document.querySelectorAll(".waiting-message-send:not([aria-disabled])").length === 2`,
      },
      {
        run: `(() => {
          const card = [...document.querySelectorAll(".waiting-message")].find((el) =>
            el.textContent.includes("Refund page done"),
          );
          if (!card) throw new Error("No waiting message from docs");
          card.querySelector(".waiting-message-send").click();
          return true;
        })()`,
        ready: shows(".waiting-message-status", "Sends once"),
      },
      { run: BLUR, ready: "true" },
    ],
  },
  {
    // "Edit group": the members' names, the lead and the group's cost today; the chain on the
    // members' rows in the sidebar.
    name: "link-groups",
    file: "hangar-link-groups",
    maxBytes: 300 * KB,
    layout: SINGLE,
    steps: [
      {
        run: contextMenu(".session-main", "Webhook retries"),
        ready: shows("[role=menuitem]", "Edit group"),
      },
      { run: click("[role=menuitem]", "Edit group"), ready: shows(".link-names input") },
      { run: BLUR, ready: "true" },
    ],
  },
  {
    // The secret guard holds a commit whose staged files seem to hold a key.
    name: "secret-guard",
    file: "hangar-secret-guard",
    maxBytes: 300 * KB,
    layout: { ...SINGLE, sideOpen: true, sideTab: "changes", notesWidth: 460 },
    steps: [
      { ready: shows(".commit-message") },
      {
        run: typeInto(".commit-message", "feat: retry failed webhook deliveries"),
        ready: `document.querySelector(".commit-message").value.length > 10`,
      },
      { run: click(".commit-box .primary-button"), ready: shows(".commit-secrets") },
      { run: BLUR, ready: "true" },
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
  await page.send("Emulation.setLocaleOverride", { locale: "en-US" }).catch(() => undefined);
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
    "readmeShots.scene": scene.name,
    "hangar.settings": {
      language: "en",
      theme: "system",
      cursorBlink: false,
      subAgentExcerpts: true,
      // The program badge as an avatar, so a session started as a sub-agent wears its figure.
      rowStyle: "avatar",
      // Two project groups instead of four program groups: the sidebar's rows fit the window.
      sessionGrouping: "project",
      ...scene.settings,
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
  await waitFor(() => page.evaluate(scene.boot ? scene.boot(shown) : BOOT(shown)), {
    what: `the started app (version ${shown})`,
  });
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
  for (const [index, step] of [
    { ready: scene.settled ?? SETTLED, timeout: 40_000 },
    ...scene.steps,
  ].entries()) {
    if (step.run) await page.evaluate(step.run);
    await waitFor(() => page.evaluate(step.ready), {
      timeout: step.timeout ?? 20_000,
      what: index === 0 ? "the settled sidebar" : `step ${index} of the scene ${scene.name}`,
    });
  }
  // Let the terminals settle, then move the pointer away, stop the spinners and the clock, and
  // freeze every animation on a fixed frame.
  await sleep(1000);
  await page.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: WIDTH - 2, y: HEIGHT - 2 });
  if (scene.hover) {
    const { selector, index } = scene.hover;
    const point = await page.evaluate(`(() => {
      const target = document.querySelectorAll(${JSON.stringify(selector)})[${index}];
      if (!target) return null;
      const box = target.getBoundingClientRect();
      return { x: Math.round(box.left + box.width / 2), y: Math.round(box.top + box.height / 2) };
    })()`);
    if (point === null) throw new Error(`Nothing to point at: ${selector} #${index}`);
    await page.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: point.x, y: point.y });
  }
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
        const raw = await render(page, vite.url, scene, scheme, version).catch(async (error) => {
          // What the page showed when the scene got stuck, kept next to the work folder.
          const failed = path.join(REPO, `tests/.tmp/readme-shots-failed-${scene.name}.png`);
          await page
            .screenshot()
            .then((shot) => writeFileSync(failed, shot))
            .then(() => console.error(`The page as it stood: ${path.relative(REPO, failed)}`))
            .catch(() => undefined);
          throw error;
        });
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

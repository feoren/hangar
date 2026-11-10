// Renders the README screenshots (docs/images/hangar-dark.png and hangar-light.png) from the real
// frontend with a mocked backend (mock.ts) in a headless Chromium based browser.
//
// Usage: pnpm readme:shots
//
// Starts its own Vite server on a free port, never touches a running Hangar, and keeps all
// temporary files (browser profile, raw images, Vite cache) under tests/.tmp/readme-shots, which
// it removes at the end.
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
// Wide enough for the version and its beta badge in the sidebar header.
const SIDEBAR_WIDTH = 320;
const MAX_BYTES = 400 * 1024;
const SCHEMES = ["dark", "light"];
const TITLE = "README-SHOTS-HARNESS";

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

/** Loads the scene in one color scheme and returns the raw screenshot. */
async function render(page, url, scheme, version) {
  await page.send("Emulation.setDeviceMetricsOverride", {
    width: WIDTH,
    height: HEIGHT,
    deviceScaleFactor: 1,
    mobile: false,
  });
  await page.send("Emulation.setFocusEmulationEnabled", { enabled: true });
  await page.send("Emulation.setEmulatedMedia", {
    features: [
      { name: "prefers-color-scheme", value: scheme },
      { name: "prefers-reduced-motion", value: "reduce" },
    ],
  });

  // First load only to reach the origin's storage, then start the app with fixed settings.
  await page.goto(url);
  if ((await page.evaluate("document.title")) !== TITLE) {
    throw new Error(`${url} does not serve the screenshot harness.`);
  }
  const storage = {
    "hangar.layout": { snapLayout: "pair", sidebarWidth: SIDEBAR_WIDTH, fontSize: 14 },
    "hangar.settings": { language: "en", theme: "system", cursorBlink: false },
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
          sessions.length === 4 &&
          text.includes("asks") &&
          text.includes("working") &&
          version !== null &&
          version.textContent.includes(${JSON.stringify(shown)})
        );
      })()`),
    { what: `four sessions, Codex working, Aider asking and version ${shown}` },
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
  // Let the terminals settle, then move the pointer away and freeze the spinner.
  await sleep(1000);
  await page.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: WIDTH - 2, y: HEIGHT - 2 });
  await page.evaluate("window.__readmeShots.freeze = true");
  await sleep(400);
  return page.screenshot();
}

async function main() {
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
    for (const scheme of SCHEMES) {
      const raw = await render(page, vite.url, scheme, version);
      writeFileSync(path.join(WORK, `raw-${scheme}.png`), raw);
      const optimized = optimizePng(raw);
      const { width, height } = pngSize(optimized);
      if (width !== WIDTH || height !== HEIGHT) {
        throw new Error(`${scheme}: got ${width} x ${height}, expected ${WIDTH} x ${HEIGHT}.`);
      }
      if (optimized.length > MAX_BYTES) {
        throw new Error(
          `${scheme}: ${optimized.length} bytes, more than the ${MAX_BYTES} allowed.`,
        );
      }
      const target = path.join(OUT_DIR, `hangar-${scheme}.png`);
      writeFileSync(target, optimized);
      console.log(
        `${path.relative(REPO, target)}: ${width} x ${height}, ${Math.round(optimized.length / 1024)} KB` +
          ` (raw ${Math.round(raw.length / 1024)} KB)`,
      );
    }
    const problems = page.logs.filter((line) => /^(error|exception)|unmocked/i.test(line));
    if (problems.length > 0) console.warn(`Page reported:\n  ${problems.join("\n  ")}`);
  } finally {
    await page?.close().catch(() => undefined);
    await vite?.server.close().catch(() => undefined);
    rmSync(WORK, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 });
  }
}

main().catch((error) => {
  console.error(`readme-shots: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
});

/// <reference types="node" />
import react from "@vitejs/plugin-react";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

import { configDefaults, defineConfig } from "vitest/config";

// Build number = buildNumberBase from package.json + number of commits (ADR 0002). Release builds
// already carry it in the version (X.Y.Z+Build, set by tools/release.py); this way dev builds show
// it too.
function buildNumber(): number | null {
  try {
    const pkg: unknown = JSON.parse(readFileSync("package.json", "utf8"));
    const base =
      typeof pkg === "object" && pkg !== null && "buildNumberBase" in pkg
        ? Number(pkg.buildNumberBase)
        : 0;
    const count = Number(
      execFileSync("git", ["rev-list", "--count", "HEAD"], { encoding: "utf8" }).trim(),
    );
    return Number.isInteger(count) && count > 0 && Number.isInteger(base) && base >= 0
      ? base + count
      : null;
  } catch {
    return null;
  }
}

// Fixed values that Tauri expects (devUrl in src-tauri/tauri.conf.json).
export default defineConfig({
  plugins: [react()],
  define: { __HANGAR_BUILD__: JSON.stringify(buildNumber()) },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // .claude/worktrees holds agents' working copies; they do not belong in this dev server.
    watch: { ignored: ["**/src-tauri/**", "**/.claude/**"] },
  },
  build: {
    target: "es2022",
  },
  test: {
    exclude: [...configDefaults.exclude, "**/.claude/**", "output/**", "release/**"],
  },
});

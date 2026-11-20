/// <reference types="node" />
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { MAX_TOKEN_BODY, TEXT_KINDS, scanSecrets, secretKinds } from "./secretScan";

interface Vector {
  name: string;
  text: string;
  kinds: string[];
}

/** The vectors the backend checks too; each text is joined from its parts at run time. */
function vectors(list: "hits" | "misses"): Vector[] {
  const file: unknown = JSON.parse(
    readFileSync(
      new URL("../src-tauri/tests/data/secret-scan-vectors.json", import.meta.url),
      "utf8",
    ),
  );
  const entries: unknown = typeof file === "object" && file !== null ? Reflect.get(file, list) : [];
  if (!Array.isArray(entries)) throw new Error("vectors missing");
  return entries.map((entry: unknown) => {
    if (typeof entry !== "object" || entry === null) throw new Error("bad vector");
    const name: unknown = Reflect.get(entry, "name");
    const parts: unknown = Reflect.get(entry, "parts");
    const kinds: unknown = Reflect.get(entry, "kinds");
    if (typeof name !== "string" || !Array.isArray(parts)) throw new Error("bad vector");
    return {
      name,
      text: parts.map(String).join(""),
      kinds: Array.isArray(kinds) ? kinds.map(String) : [],
    };
  });
}

describe("scanSecrets", () => {
  it("finds every hit of the shared vectors with its kinds", () => {
    const hits = vectors("hits");
    expect(hits.length).toBeGreaterThanOrEqual(10);
    for (const vector of hits) {
      expect(scanSecrets(vector.text), vector.name).toEqual(vector.kinds);
    }
  });

  it("finds nothing in the shared misses", () => {
    const misses = vectors("misses");
    expect(misses.length).toBeGreaterThanOrEqual(10);
    for (const vector of misses) {
      expect(scanSecrets(vector.text), vector.name).toEqual([]);
    }
  });

  it("has a hit vector of its own for every kind", () => {
    const hits = vectors("hits");
    for (const kind of TEXT_KINDS) {
      expect(
        hits.some((vector) => vector.kinds.length === 1 && vector.kinds[0] === kind),
        kind,
      ).toBe(true);
    }
  });

  it("stays usable and quick on megabytes of one prefix or one long run", () => {
    const prefixes = ["-----BEGIN ", "AKIA", "ghp_", "github_pat_", "sk-", "sk-ant-", "sk-proj-"];
    prefixes.push("xoxb-", "sk_live_");
    for (const prefix of prefixes) {
      for (const filler of ["", "a", "A1"]) {
        const unit = prefix + filler;
        const repeated = unit.repeat(Math.floor((6 * 1024 * 1024) / unit.length));
        const long = prefix + "Fake0Marker1Value2".repeat(400_000);
        const started = Date.now();
        expect(scanSecrets(repeated), unit).not.toContain("unknown");
        expect(scanSecrets(long), prefix).toEqual([]);
        expect(Date.now() - started, unit).toBeLessThan(3000);
      }
    }
  });

  it("counts a body only up to the backend's bound", () => {
    const body = "Fake0Marker1Value2Test3Hangar4".repeat(20);
    expect(scanSecrets(["sk", "_live_", body.slice(0, MAX_TOKEN_BODY)].join(""))).toEqual([
      "stripeKey",
    ]);
    expect(scanSecrets(["sk", "_live_", body.slice(0, MAX_TOKEN_BODY + 1)].join(""))).toEqual([]);
  });

  it("reads only known kinds from the backend", () => {
    expect(secretKinds(["githubToken", "x", 3, "stripeKey"])).toEqual(["githubToken", "stripeKey"]);
    expect(secretKinds(["secretFile", "unknown"])).toEqual(["secretFile"]);
    expect(secretKinds("githubToken")).toEqual([]);
  });
});

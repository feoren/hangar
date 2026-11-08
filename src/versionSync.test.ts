import { describe, expect, it } from "vitest";

// Rule "the code always carries the upcoming version" (ADR 0002): all version files name the
// same X.Y.Z, and it is the highest version in the changelog. Otherwise the app shows a different
// version than What's New. Read as text, so no build step stands in between.
import packageText from "../package.json?raw";
import cargoText from "../src-tauri/Cargo.toml?raw";
import tauriConfText from "../src-tauri/tauri.conf.json?raw";
import { baseVersion, compareVersions } from "./changelog";
import changelogText from "./changelog.json?raw";

function topLevelVersion(json: string, file: string): string {
  const parsed: unknown = JSON.parse(json);
  if (typeof parsed === "object" && parsed !== null && "version" in parsed) {
    const { version } = parsed;
    if (typeof version === "string") return version;
  }
  throw new Error(`${file}: no version`);
}

function cargoVersion(toml: string): string {
  // Only [package], not the versions of the dependencies.
  const match = /^\[package\][^[]*?^version\s*=\s*"([^"]+)"/m.exec(toml);
  if (!match?.[1]) throw new Error("Cargo.toml: no version in [package]");
  return match[1];
}

function changelogVersions(json: string): string[] {
  const parsed: unknown = JSON.parse(json);
  if (!Array.isArray(parsed)) throw new Error("changelog.json: not a list");
  return parsed.flatMap((entry: unknown) =>
    typeof entry === "object" &&
    entry !== null &&
    "version" in entry &&
    typeof entry.version === "string"
      ? [entry.version]
      : [],
  );
}

describe("versions in sync", () => {
  const tauri = topLevelVersion(tauriConfText, "tauri.conf.json");

  it("tauri.conf.json, package.json and Cargo.toml name the same version", () => {
    expect(topLevelVersion(packageText, "package.json")).toBe(tauri);
    expect(cargoVersion(cargoText)).toBe(tauri);
  });

  it("the version of the files is the highest in the changelog", () => {
    const highest = changelogVersions(changelogText).reduce<string | null>(
      (best, version) => (best === null || compareVersions(version, best) > 0 ? version : best),
      null,
    );
    // After a release the files contain X.Y.Z+Build; X.Y.Z is compared.
    expect(baseVersion(tauri)).toBe(highest);
  });
});

import { describe, expect, it } from "vitest";

import type { Profile } from "./api";
import { BADGE_COLORS, badgeColor } from "./format";
import {
  arrangedProfiles,
  availableProfiles,
  DEFAULT_PROGRAM_PREFS,
  fitContain,
  iconChoice,
  iconFileProblem,
  iconMode,
  MAX_ICON_DATA_URL,
  MAX_ICON_FILE_BYTES,
  moveTo,
  normalizeLetters,
  parseProgramPrefs,
  resolveBadge,
  sortByOrder,
  startableProfiles,
  withHidden,
  withIcon,
  type ProgramPrefs,
} from "./programPrefs";

const PNG = "data:image/png;base64,iVBORw0KGgo=";

function profile(id: string, installed = true, source: Profile["source"] = "catalog"): Profile {
  return {
    id,
    name: id.toUpperCase(),
    command: id,
    args: [],
    cwd: null,
    icon: null,
    source,
    installed,
    homepage: null,
    installHint: null,
  };
}

const prefs = (patch: Partial<ProgramPrefs>): ProgramPrefs => ({
  ...DEFAULT_PROGRAM_PREFS,
  ...patch,
});

describe("parseProgramPrefs", () => {
  it("uses defaults for nonsense", () => {
    for (const value of [undefined, null, 42, "x", [], { order: "a" }]) {
      expect(parseProgramPrefs(value)).toEqual(DEFAULT_PROGRAM_PREFS);
    }
  });

  it("keeps unknown IDs and removes duplicates and invalid entries", () => {
    const parsed = parseProgramPrefs({
      order: ["claude", "uninstalled-program", "claude", 7, ""],
      hidden: ["pwsh", null],
    });
    expect(parsed.order).toEqual(["claude", "uninstalled-program"]);
    expect(parsed.hidden).toEqual(["pwsh"]);
  });

  it("validates icons individually", () => {
    const parsed = parseProgramPrefs({
      icons: {
        a: { kind: "letters", text: "  Xyz ", color: BADGE_COLORS[2] },
        b: { kind: "letters", text: "Q", color: "red" },
        c: { kind: "letters", text: "   " },
        d: { kind: "image", src: PNG },
        e: { kind: "image", src: "data:image/svg+xml;base64,PHN2Zz4=" },
        f: { kind: "image", src: `data:image/png;base64,${"A".repeat(MAX_ICON_DATA_URL)}` },
        g: { kind: "auto" },
        ["__proto__"]: { kind: "letters", text: "P" },
      },
    });
    expect(iconChoice(parsed, "a")).toEqual({
      kind: "letters",
      text: "Xy",
      color: BADGE_COLORS[2],
    });
    expect(iconChoice(parsed, "b")).toEqual({ kind: "letters", text: "Q", color: BADGE_COLORS[0] });
    expect(iconChoice(parsed, "c")).toBeUndefined();
    expect(iconChoice(parsed, "d")).toEqual({ kind: "image", src: PNG });
    expect(iconChoice(parsed, "e")).toBeUndefined();
    expect(iconChoice(parsed, "f")).toBeUndefined();
    expect(iconChoice(parsed, "g")).toBeUndefined();
    expect(iconChoice(parsed, "constructor")).toBeUndefined();
    expect(Object.getPrototypeOf(parsed.icons)).toBe(Object.prototype);
  });

  it("survives a round trip through JSON", () => {
    const original = withIcon(prefs({ order: ["b", "a"], hidden: ["c"] }), "a", {
      kind: "image",
      src: PNG,
    });
    expect(parseProgramPrefs(JSON.parse(JSON.stringify(original)))).toEqual(original);
  });
});

describe("normalizeLetters", () => {
  it("shortens to two characters and removes control characters", () => {
    expect(normalizeLetters("abc")).toBe("ab");
    expect(normalizeLetters(" k ")).toBe("k");
    expect(normalizeLetters("\u0007Z")).toBe("Z");
    expect(normalizeLetters("日本語")).toBe("日本");
    expect(normalizeLetters("")).toBe("");
  });
});

describe("order", () => {
  const items = ["a", "b", "c", "d"].map((id) => ({ id }));

  it("sorts by the saved order, new ones at the end", () => {
    expect(sortByOrder(items, ["c", "x", "a"]).map((i) => i.id)).toEqual(["c", "a", "b", "d"]);
    expect(sortByOrder(items, []).map((i) => i.id)).toEqual(["a", "b", "c", "d"]);
  });

  it("moves and keeps IDs that are not shown", () => {
    const shown = ["a", "b", "c"];
    expect(moveTo(shown, ["x", "a"], "c", 0)).toEqual(["c", "a", "b", "x"]);
    expect(moveTo(shown, [], "a", 2)).toEqual(["b", "c", "a"]);
    expect(moveTo(shown, [], "a", 99)).toEqual(["b", "c", "a"]);
    expect(moveTo(shown, [], "b", -3)).toEqual(["b", "a", "c"]);
    expect(moveTo(shown, ["q"], "missing", 0)).toEqual(["q"]);
  });

  it("orders installed profiles and leaves out hidden ones", () => {
    const profiles = [profile("a"), profile("b", false), profile("c"), profile("d")];
    const p = prefs({ order: ["d", "a"], hidden: ["a"] });
    expect(arrangedProfiles(profiles, p).map((x) => x.id)).toEqual(["d", "a", "c"]);
    expect(startableProfiles(profiles, p).map((x) => x.id)).toEqual(["d", "c"]);
  });

  it("lists only catalog agents that are not installed as available", () => {
    const profiles = [
      profile("zeta", false),
      profile("alpha", false),
      profile("own", false, "user"),
      profile("present"),
    ];
    expect(availableProfiles(profiles).map((x) => x.id)).toEqual(["alpha", "zeta"]);
  });
});

describe("visibility and icons", () => {
  it("toggles visibility without duplicates", () => {
    const hidden = withHidden(withHidden(DEFAULT_PROGRAM_PREFS, "a", true), "a", true);
    expect(hidden.hidden).toEqual(["a"]);
    expect(withHidden(hidden, "a", false).hidden).toEqual([]);
  });

  it("sets and removes icons", () => {
    const set = withIcon(DEFAULT_PROGRAM_PREFS, "a", {
      kind: "letters",
      text: "A",
      color: "#287a70",
    });
    expect(iconMode(set, "a")).toBe("letters");
    expect(iconMode(withIcon(set, "a", null), "a")).toBe("auto");
  });

  it("resolves the badge: preference, then agent icon, then program icon, then letters", () => {
    const p = profile("claude");
    const AGENT = "/assets/claude.svg";
    expect(
      resolveBadge("claude", p, { agent: AGENT, program: PNG }, DEFAULT_PROGRAM_PREFS),
    ).toEqual({ kind: "image", src: AGENT });
    expect(resolveBadge("claude", p, { program: PNG }, DEFAULT_PROGRAM_PREFS)).toEqual({
      kind: "image",
      src: PNG,
    });
    expect(resolveBadge("claude", p, {}, DEFAULT_PROGRAM_PREFS)).toEqual({
      kind: "letters",
      text: "C",
      color: badgeColor("claude"),
    });
    const letters = withIcon(DEFAULT_PROGRAM_PREFS, "claude", {
      kind: "letters",
      text: "Cl",
      color: "#8a5cc2",
    });
    expect(resolveBadge("claude", p, { agent: AGENT, program: PNG }, letters)).toEqual({
      kind: "letters",
      text: "Cl",
      color: "#8a5cc2",
    });
    const own = "data:image/png;base64,AAAA";
    const image = withIcon(DEFAULT_PROGRAM_PREFS, "claude", { kind: "image", src: own });
    expect(resolveBadge("claude", undefined, { agent: AGENT }, image)).toEqual({
      kind: "image",
      src: own,
    });
  });
});

describe("image files", () => {
  it("fits images centered", () => {
    expect(fitContain(64, 32, 32)).toEqual({ x: 0, y: 8, width: 32, height: 16 });
    expect(fitContain(10, 20, 32)).toEqual({ x: 8, y: 0, width: 16, height: 32 });
    expect(fitContain(0, 0, 32)).toEqual({ x: 0, y: 0, width: 32, height: 32 });
  });

  it("accepts only PNG and SVG up to the size limit", () => {
    expect(iconFileProblem("image/png", 2000)).toBeNull();
    expect(iconFileProblem("image/svg+xml", 2000)).toBeNull();
    expect(iconFileProblem("image/jpeg", 2000)).toBe("type");
    expect(iconFileProblem("image/png", MAX_ICON_FILE_BYTES + 1)).toBe("size");
    expect(iconFileProblem("image/png", 0)).toBe("size");
  });
});

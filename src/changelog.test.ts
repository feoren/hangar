import { describe, expect, it } from "vitest";

import {
  baseVersion,
  changelogProblems,
  compareVersions,
  decideWhatsNew,
  entriesSince,
  entryHeading,
  formatReleaseDate,
  initiallyOpenVersion,
  itemText,
  parseChangelog,
  toggleVersion,
  type ChangelogEntry,
} from "./changelog";
import changelogText from "./changelog.json?raw";

const item = (de: string, en = `${de} (en)`) => ({ de, en });

const ENTRIES: ChangelogEntry[] = [
  { version: "0.3.0", date: null, items: [item("Drei")] },
  { version: "0.2.0", date: "2026-12-01", items: [item("Zwei")] },
  { version: "0.1.0", date: "2026-11-07", items: [item("Eins")] },
];

describe("src/changelog.json", () => {
  const raw: unknown = JSON.parse(changelogText);

  it("follows the schema (required languages, versions, order, texts)", () => {
    expect(changelogProblems(raw)).toEqual([]);
  });

  it("is read completely (except a freshly created version without items)", () => {
    const entries = parseChangelog(changelogText);
    const withItems = Array.isArray(raw)
      ? raw.filter(
          (entry: unknown) =>
            typeof entry === "object" &&
            entry !== null &&
            "items" in entry &&
            Array.isArray(entry.items) &&
            entry.items.length > 0,
        )
      : [];
    expect(entries).toHaveLength(withItems.length);
    expect(entries[entries.length - 1]?.version).toBe("0.1.0");
  });

  it("has all 12 languages in every item", () => {
    for (const entry of parseChangelog(changelogText)) {
      for (const point of entry.items) {
        expect(Object.keys(point).sort()).toHaveLength(12);
      }
    }
  });
});

describe("changelogProblems", () => {
  it("reports missing required languages, empty texts and dashes", () => {
    const problems = changelogProblems([
      { version: "1.0.0", date: "2026-01-01", items: [{ de: "Gut" }, { de: " ", en: "x" }] },
      { version: "0.9.0", date: "2026-01-01", items: [{ de: "a – b", en: "ok", xx: "?" }] },
    ]);
    expect(problems.some((p) => p.includes("language en missing"))).toBe(true);
    expect(problems.some((p) => p.includes(".de: empty"))).toBe(true);
    expect(problems.some((p) => p.includes("en or em dash"))).toBe(true);
    expect(problems.some((p) => p.includes("unknown language"))).toBe(true);
  });

  it("reports invalid versions, dates and wrong order", () => {
    const problems = changelogProblems([
      { version: "0.1.0", date: "2026-02-30", items: [item("a")] },
      { version: "0.2.0", date: null, items: [item("b")] },
      { version: "v1", date: "2026-01-01", items: [] },
    ]);
    expect(problems.some((p) => p.includes("date must be"))).toBe(true);
    expect(problems.some((p) => p.includes("not in descending order"))).toBe(true);
    expect(problems.some((p) => p.includes("may have no date"))).toBe(true);
    expect(problems.some((p) => p.includes("X.Y.Z"))).toBe(true);
    expect(problems.some((p) => p.includes("no items"))).toBe(true);
  });

  it("allows only the newest, still undated version without items", () => {
    expect(
      changelogProblems([
        { version: "0.3.0", date: null, items: [] },
        { version: "0.2.0", date: "2026-11-08", items: [item("a")] },
      ]),
    ).toEqual([]);
    expect(changelogProblems([{ version: "0.3.0", date: "2026-11-08", items: [] }])).toEqual([
      "entry 0: no items",
    ]);
    expect(parseChangelog(JSON.stringify([{ version: "0.3.0", date: null, items: [] }]))).toEqual(
      [],
    );
  });

  it("reports items that are too long and a wrong root", () => {
    expect(changelogProblems({})).toEqual(["root is not a list"]);
    const long = "word ".repeat(40).trim();
    expect(
      changelogProblems([{ version: "1.0.0", date: null, items: [item(long)] }]).length,
    ).toBeGreaterThan(0);
  });
});

describe("parseChangelog", () => {
  it("survives broken JSON and drops invalid entries", () => {
    expect(parseChangelog("{broken")).toEqual([]);
    expect(parseChangelog("{}")).toEqual([]);
    const entries = parseChangelog(
      JSON.stringify([
        { version: "0.1.0", date: "2026-11-07", items: [item("Eins"), { de: "de only" }] },
        { version: "x", date: null, items: [item("gone")] },
        { version: "0.2.0", date: "no date", items: [item("Zwei")] },
      ]),
    );
    expect(entries.map((e) => e.version)).toEqual(["0.2.0", "0.1.0"]);
    expect(entries[0]?.date).toBeNull();
    expect(entries[1]?.items).toHaveLength(1);
  });
});

describe("versions", () => {
  it("compares only X.Y.Z", () => {
    expect(compareVersions("0.2.0", "0.1.9")).toBeGreaterThan(0);
    expect(compareVersions("0.10.0", "0.9.0")).toBeGreaterThan(0);
    expect(compareVersions("0.2.0+50", "0.2.0")).toBe(0);
    expect(compareVersions("1.0.0", "1.0.1")).toBeLessThan(0);
    expect(compareVersions("broken", "0.0.1")).toBeLessThan(0);
  });

  it("strips the build number", () => {
    expect(baseVersion("0.2.0+57")).toBe("0.2.0");
    expect(baseVersion("0.2.0-57")).toBe("0.2.0");
    expect(baseVersion("x")).toBeNull();
  });
});

describe("itemText and formatReleaseDate", () => {
  it("falls back to English", () => {
    const point = { de: "Hallo", en: "Hello", fr: "Salut" };
    expect(itemText(point, "fr")).toBe("Salut");
    expect(itemText(point, "ja")).toBe("Hello");
    expect(itemText(point, "de")).toBe("Hallo");
  });

  it("formats the date in the language, without a time zone offset", () => {
    expect(formatReleaseDate("2026-11-07", "de")).toBe("7. November 2026");
    expect(formatReleaseDate("2026-11-07", "en")).toBe("November 7, 2026");
    expect(formatReleaseDate("nonsense", "de")).toBe("nonsense");
  });
});

describe("entriesSince", () => {
  it("takes only versions after the seen one up to the running one", () => {
    expect(entriesSince(ENTRIES, "0.1.0", "0.2.0+9").map((e) => e.version)).toEqual(["0.2.0"]);
    expect(entriesSince(ENTRIES, "0.0.0", "0.3.0").map((e) => e.version)).toEqual([
      "0.3.0",
      "0.2.0",
      "0.1.0",
    ]);
    expect(entriesSince(ENTRIES, "0.2.0", "0.2.0")).toEqual([]);
  });
});

describe("initiallyOpenVersion", () => {
  it("opens the highest changelog version, not the installed one", () => {
    expect(initiallyOpenVersion(ENTRIES, ENTRIES)).toBe("0.3.0");
    // The order of the list does not matter.
    expect(initiallyOpenVersion(ENTRIES, [...ENTRIES].reverse())).toBe("0.3.0");
  });

  it("opens the highest shown version when the newest is not shown", () => {
    const shown = entriesSince(ENTRIES, "0.0.0", "0.2.0");
    expect(initiallyOpenVersion(ENTRIES, shown)).toBe("0.2.0");
  });

  it("expands nothing for at most one version", () => {
    expect(initiallyOpenVersion(ENTRIES, ENTRIES.slice(0, 1))).toBeNull();
    expect(initiallyOpenVersion(ENTRIES, [])).toBeNull();
  });
});

describe("entryHeading", () => {
  const upcoming = ENTRIES[0];
  const released = ENTRIES[2];
  if (upcoming === undefined || released === undefined) throw new Error("ENTRIES incomplete");

  it("shows the full version with build number for the running version", () => {
    expect(entryHeading(upcoming, "0.3.0+57", "de")).toEqual({
      version: "0.3.0+57",
      date: null,
      current: true,
    });
    // Release builds report the build number themselves, dev builds via withBuild; both count.
    expect(entryHeading(released, "0.1.0-12", "de").current).toBe(true);
  });

  it("shows only X.Y.Z without a build number", () => {
    expect(entryHeading(upcoming, "0.3.0", "de")).toEqual({
      version: "0.3.0",
      date: null,
      current: true,
    });
  });

  it("keeps other versions at X.Y.Z and formats the date", () => {
    expect(entryHeading(released, "0.3.0+57", "de")).toEqual({
      version: "0.1.0",
      date: "7. November 2026",
      current: false,
    });
  });

  it("marks nothing while the running version is unknown or unreadable", () => {
    expect(entryHeading(upcoming, null, "en").current).toBe(false);
    expect(entryHeading(upcoming, "broken", "en")).toEqual({
      version: "0.3.0",
      date: null,
      current: false,
    });
  });
});

describe("toggleVersion", () => {
  it("expands and collapses without changing the old set", () => {
    const start: ReadonlySet<string> = new Set(["0.3.0"]);
    const opened = toggleVersion(start, "0.1.0");
    expect([...opened].sort()).toEqual(["0.1.0", "0.3.0"]);
    expect([...toggleVersion(opened, "0.3.0")]).toEqual(["0.1.0"]);
    expect([...start]).toEqual(["0.3.0"]);
  });
});

describe("decideWhatsNew", () => {
  it("shows nothing on the very first start and remembers the version", () => {
    expect(decideWhatsNew(ENTRIES, "0.2.0+12", null, false)).toEqual({
      show: false,
      since: null,
      remember: "0.2.0",
    });
  });

  it("shows the new entries after an update", () => {
    expect(decideWhatsNew(ENTRIES, "0.2.0+12", "0.1.0", true)).toEqual({
      show: true,
      since: "0.1.0",
      remember: "0.2.0",
    });
  });

  it("shows existing users without a remembered version everything after the oldest", () => {
    expect(decideWhatsNew(ENTRIES, "0.2.0", null, true)).toEqual({
      show: true,
      since: "0.1.0",
      remember: "0.2.0",
    });
  });

  it("shows nothing more for the same version", () => {
    expect(decideWhatsNew(ENTRIES, "0.2.0+13", "0.2.0", true)).toEqual({
      show: false,
      since: null,
      remember: null,
    });
  });

  it("remembers an update even without entries but shows nothing", () => {
    const only = ENTRIES.slice(2);
    expect(decideWhatsNew(only, "0.2.0", "0.1.0", true)).toEqual({
      show: false,
      since: "0.1.0",
      remember: "0.2.0",
    });
  });

  it("does not overwrite the higher version on a downgrade", () => {
    expect(decideWhatsNew(ENTRIES, "0.1.0", "0.2.0", true).remember).toBeNull();
  });

  it("does nothing for an unreadable app version and survives unreadable stored data", () => {
    expect(decideWhatsNew(ENTRIES, "dev", "0.1.0", true)).toEqual({
      show: false,
      since: null,
      remember: null,
    });
    expect(decideWhatsNew(ENTRIES, "0.2.0", "garbage", false).remember).toBe("0.2.0");
  });
});

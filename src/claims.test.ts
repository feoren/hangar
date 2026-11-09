import { describe, expect, it } from "vitest";

import {
  claimLines,
  EMPTY_CLAIMS,
  formatLines,
  freshOverlaps,
  markerText,
  overlapKey,
  overlapNotice,
  overlapsOf,
  overlapText,
  parseClaimsSnapshot,
  type ClaimsSnapshot,
  type Overlap,
} from "./claims";
import { createTranslator } from "./i18n";

const t = createTranslator("en");
const names = new Map([
  [1, "Claude"],
  [2, "Codex"],
  [3, "Gemini"],
]);
const nameOf = (id: number) => names.get(id) ?? `#${String(id)}`;

/** As the backend serializes it (claims.rs, `snapshot_serializes_camel_case`). */
const raw = {
  enabled: true,
  sessions: [
    {
      sessionId: 1,
      state: { kind: "ready" },
      root: "C:\\work\\wt-a",
      base: "0123456789abcdef0123456789abcdef01234567",
      sharedWith: [3],
      claims: [
        {
          path: "src/x.ts",
          kind: "modified",
          wholeFile: false,
          ranges: [{ start: 10, end: 12 }],
          area: "src",
        },
        { path: "README.md", kind: "untracked", wholeFile: true, ranges: [], area: null },
      ],
      truncated: false,
      level: "overlappingLines",
    },
    {
      sessionId: 2,
      state: { kind: "failed", message: "git was not found on PATH" },
      root: null,
      base: null,
      sharedWith: [],
      claims: [],
      truncated: false,
      level: "none",
    },
  ],
  overlaps: [
    {
      sessionId: 1,
      otherId: 2,
      level: "overlappingLines",
      path: "src/x.ts",
      area: null,
      lines: [{ start: 10, end: 15 }],
    },
    {
      sessionId: 1,
      otherId: 2,
      level: "sameFile",
      path: "src/y.ts",
      area: null,
      lines: [],
    },
    { sessionId: 1, otherId: 2, level: "sameArea", path: null, area: "docs", lines: [] },
    {
      sessionId: 2,
      otherId: 1,
      level: "overlappingLines",
      path: "src/x.ts",
      area: null,
      lines: [{ start: 10, end: 15 }],
    },
  ],
};

function snapshot(): ClaimsSnapshot {
  const parsed = parseClaimsSnapshot(raw);
  if (parsed === null) throw new Error("fixture does not parse");
  return parsed;
}

describe("parseClaimsSnapshot", () => {
  it("reads the backend's snapshot", () => {
    const parsed = snapshot();
    expect(parsed.enabled).toBe(true);
    expect(parsed.sessions[0]?.claims[0]?.ranges).toEqual([{ start: 10, end: 12 }]);
    expect(parsed.sessions[1]?.state).toEqual({
      kind: "failed",
      message: "git was not found on PATH",
    });
    expect(parsed.overlaps).toHaveLength(4);
    expect(parseClaimsSnapshot({ enabled: false, sessions: [], overlaps: [] })).toEqual(
      EMPTY_CLAIMS,
    );
  });

  it("refuses malformed payloads", () => {
    const broken = [
      null,
      "x",
      { ...raw, enabled: "yes" },
      { ...raw, overlaps: [{ ...raw.overlaps[0], level: "worst" }] },
      { ...raw, overlaps: [{ ...raw.overlaps[0], lines: [{ start: 9, end: 2 }] }] },
      { ...raw, sessions: [{ ...raw.sessions[0], state: { kind: "failed" } }] },
      { ...raw, sessions: [{ ...raw.sessions[0], sharedWith: [-1] }] },
      {
        ...raw,
        sessions: [{ ...raw.sessions[0], claims: [{ path: "a", kind: "moved", wholeFile: true }] }],
      },
    ];
    for (const value of broken) expect(parseClaimsSnapshot(value)).toBeNull();
  });
});

describe("texts", () => {
  it("formats line ranges", () => {
    expect(formatLines([{ start: 4, end: 4 }], t)).toBe("line 4");
    expect(
      formatLines(
        [
          { start: 4, end: 9 },
          { start: 20, end: 20 },
        ],
        t,
      ),
    ).toBe("lines 4 to 9, line 20");
  });

  it("describes each level", () => {
    const [lines, file, area] = overlapsOf(snapshot(), 1);
    if (!lines || !file || !area) throw new Error("fixture");
    expect(overlapText(lines, "Codex", t)).toBe("Same lines as Codex: src/x.ts, lines 10 to 15");
    expect(overlapText(file, "Codex", t)).toBe("Same file as Codex: src/y.ts");
    expect(overlapText(area, "Codex", t)).toBe("Same area as Codex: docs");
  });

  it("marks only sessions that collide on a file", () => {
    const parsed = snapshot();
    expect(markerText(overlapsOf(parsed, 1), nameOf, t)).toBe(
      "Same lines as Codex: src/x.ts, lines 10 to 15 (+1)",
    );
    expect(markerText(overlapsOf(parsed, 2), nameOf, t)).toBe(
      "Same lines as Claude: src/x.ts, lines 10 to 15",
    );
    const areaOnly = parsed.overlaps.filter((o) => o.level === "sameArea");
    expect(markerText(areaOnly, nameOf, t)).toBeNull();
    expect(markerText([], nameOf, t)).toBeNull();
  });

  it("shows ranges or the whole file per claim", () => {
    const [ranged, whole] = snapshot().sessions[0]?.claims ?? [];
    if (!ranged || !whole) throw new Error("fixture");
    expect(claimLines(ranged, t)).toBe("lines 10 to 12");
    expect(claimLines(whole, t)).toBe("whole file");
  });
});

describe("fresh overlaps", () => {
  it("announces each file collision once, from the lower session id", () => {
    const parsed = snapshot();
    const fresh = freshOverlaps(new Set(), parsed);
    expect(fresh.map((o) => [o.sessionId, o.otherId, o.path])).toEqual([
      [1, 2, "src/x.ts"],
      [1, 2, "src/y.ts"],
    ]);
    const seen = new Set(fresh.map(overlapKey));
    expect(freshOverlaps(seen, parsed)).toEqual([]);
  });

  it("uses one key for both sides", () => {
    const a: Overlap = {
      sessionId: 1,
      otherId: 2,
      level: "sameFile",
      path: "p",
      area: null,
      lines: [],
    };
    expect(overlapKey(a)).toBe(overlapKey({ ...a, sessionId: 2, otherId: 1 }));
    expect(overlapKey(a)).not.toBe(overlapKey({ ...a, path: "q" }));
  });

  it("builds the notice text", () => {
    const [first, second] = freshOverlaps(new Set(), snapshot());
    if (!first || !second) throw new Error("fixture");
    expect(overlapNotice(first, nameOf, t)).toEqual({
      title: "Claude and Codex change the same file",
      body: "src/x.ts, lines 10 to 15",
    });
    expect(overlapNotice(second, nameOf, t).body).toBe("src/y.ts");
  });
});

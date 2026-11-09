import { describe, expect, it } from "vitest";

import type { AttentionReason, AttentionState } from "./attention";
import {
  decisionText,
  decisionToShow,
  EMPTY_GATE,
  freshDecisions,
  gateHint,
  gateSetup,
  parseGateSnapshot,
  waitingSessions,
  withGate,
  type PendingDecision,
} from "./gate";
import { createTranslator } from "./i18n";

const t = createTranslator("en");
const names = new Map([
  [1, "Claude Code"],
  [2, "Codex · feature"],
  [3, "Gemini"],
]);
const nameOf = (id: number) => names.get(id) ?? `#${String(id)}`;

const NO_TEAMMATES: string[] = [];

/** As the backend serializes it (camelCase). */
const rawDecision = {
  id: 7,
  sessionId: 1,
  tool: "Edit",
  path: "src/x.ts",
  otherSessions: [2],
  teammates: NO_TEAMMATES,
  level: "overlappingLines",
  lines: [{ start: 10, end: 14 }],
  createdAt: 1_762_700_000_000,
};

function decision(overrides: Partial<PendingDecision> = {}): PendingDecision {
  return { ...rawDecision, level: "overlappingLines", ...overrides };
}

describe("parseGateSnapshot", () => {
  it("accepts the backend's snapshot", () => {
    expect(parseGateSnapshot({ enabled: true, pending: [rawDecision] })).toEqual({
      enabled: true,
      pending: [decision()],
    });
    expect(parseGateSnapshot({ enabled: false, pending: [] })).toEqual(EMPTY_GATE);
  });

  it("keeps any tool name and empty lines (whole file or unknown)", () => {
    const parsed = parseGateSnapshot({
      enabled: true,
      pending: [{ ...rawDecision, tool: "SomeFutureTool", level: "sameFile", lines: [] }],
    });
    expect(parsed?.pending[0]?.tool).toBe("SomeFutureTool");
    expect(parsed?.pending[0]?.lines).toEqual([]);
  });

  it("sorts the queue by id, oldest first", () => {
    const parsed = parseGateSnapshot({
      enabled: true,
      pending: [
        { ...rawDecision, id: 9 },
        { ...rawDecision, id: 3 },
      ],
    });
    expect(parsed?.pending.map((d) => d.id)).toEqual([3, 9]);
  });

  it("rejects malformed snapshots as a whole", () => {
    const bad: unknown[] = [
      null,
      undefined,
      "snapshot",
      42,
      [],
      { pending: [] },
      { enabled: "yes", pending: [] },
      { enabled: true, pending: {} },
      { enabled: true, pending: [null] },
      { enabled: true, pending: [{ ...rawDecision, id: -1 }] },
      { enabled: true, pending: [{ ...rawDecision, id: 1.5 }] },
      { enabled: true, pending: [{ ...rawDecision, id: "7" }] },
      { enabled: true, pending: [{ ...rawDecision, sessionId: null }] },
      { enabled: true, pending: [{ ...rawDecision, tool: 1 }] },
      { enabled: true, pending: [{ ...rawDecision, path: "" }] },
      { enabled: true, pending: [{ ...rawDecision, path: ["src/x.ts"] }] },
      { enabled: true, pending: [{ ...rawDecision, otherSessions: [] }] },
      { enabled: true, pending: [{ ...rawDecision, otherSessions: [2, "3"] }] },
      { enabled: true, pending: [{ ...rawDecision, teammates: [""] }] },
      { enabled: true, pending: [{ ...rawDecision, teammates: [1] }] },
      { enabled: true, pending: [{ ...rawDecision, level: "sameArea" }] },
      { enabled: true, pending: [{ ...rawDecision, level: "none" }] },
      { enabled: true, pending: [{ ...rawDecision, lines: [{ start: 5, end: 2 }] }] },
      { enabled: true, pending: [{ ...rawDecision, lines: [{ start: 1 }] }] },
      { enabled: true, pending: [{ ...rawDecision, lines: null }] },
      { enabled: true, pending: [{ ...rawDecision, createdAt: Number.NaN }] },
      { enabled: true, pending: [{ ...rawDecision, createdAt: -5 }] },
      { enabled: true, pending: [{ ...rawDecision, createdAt: "now" }] },
      { enabled: true, pending: [rawDecision, rawDecision] },
    ];
    for (const value of bad) expect(parseGateSnapshot(value), JSON.stringify(value)).toBeNull();
  });

  it("keeps hostile path characters for display to reveal", () => {
    const path = "src/‮gnp.ts\u0007";
    const parsed = parseGateSnapshot({ enabled: true, pending: [{ ...rawDecision, path }] });
    expect(parsed?.pending[0]?.path).toBe(path);
  });
});

describe("decisionText", () => {
  it("names the waiting session, the file, the other session and its lines", () => {
    expect(decisionText(decision(), nameOf, t)).toEqual({
      title: "Claude Code wants to change src/x.ts",
      detail: "Also being changed by Codex · feature: lines 10 to 14",
    });
  });

  it("writes a single line and several ranges", () => {
    const one = decision({ lines: [{ start: 3, end: 3 }] });
    expect(decisionText(one, nameOf, t).detail).toBe(
      "Also being changed by Codex · feature: line 3",
    );
    const several = decision({
      lines: [
        { start: 3, end: 3 },
        { start: 10, end: 14 },
      ],
    });
    expect(decisionText(several, nameOf, t).detail).toBe(
      "Also being changed by Codex · feature: line 3, lines 10 to 14",
    );
  });

  it("leaves the lines out at file level or when unknown", () => {
    expect(decisionText(decision({ level: "sameFile" }), nameOf, t).detail).toBe(
      "Also being changed by Codex · feature",
    );
    expect(decisionText(decision({ lines: [] }), nameOf, t).detail).toBe(
      "Also being changed by Codex · feature",
    );
  });

  it("waits for a teammate alone, and names teammates after the own sessions", () => {
    const parsed = parseGateSnapshot({
      enabled: true,
      pending: [{ ...rawDecision, otherSessions: [], teammates: ["Anna (Claude Code)"] }],
    });
    expect(parsed?.pending[0]?.teammates).toEqual(["Anna (Claude Code)"]);
    const text = decisionText(
      decision({ otherSessions: [2], teammates: ["Anna (Claude Code)"], level: "sameFile" }),
      nameOf,
      t,
    );
    expect(text.detail).toBe("Also being changed by Codex · feature, Anna (Claude Code)");
    // An older backend sends no teammates: none.
    const older = Object.fromEntries(
      Object.entries(rawDecision).filter(([key]) => key !== "teammates"),
    );
    expect(parseGateSnapshot({ enabled: true, pending: [older] })?.pending[0]?.teammates).toEqual(
      [],
    );
  });

  it("lists every other session and numbers unknown ones", () => {
    const text = decisionText(decision({ otherSessions: [2, 3, 8], level: "sameFile" }), nameOf, t);
    expect(text.detail).toBe("Also being changed by Codex · feature, Gemini, #8");
  });

  it("shows invisible and control characters in the path as placeholders", () => {
    const text = decisionText(decision({ path: "src/‮gnp.ts" }), nameOf, t);
    expect(text.title).toBe("Claude Code wants to change src/<U+202E>gnp.ts");
  });

  it("speaks the user's language", () => {
    const de = createTranslator("de");
    expect(decisionText(decision({ level: "sameFile" }), nameOf, de)).toEqual({
      title: "Claude Code will src/x.ts ändern",
      detail: "Wird gerade auch von Codex · feature geändert",
    });
  });
});

describe("decisionToShow", () => {
  const pending = [
    decision({ id: 1, sessionId: 5 }),
    decision({ id: 2, sessionId: 1 }),
    decision({ id: 3, sessionId: 1 }),
    decision({ id: 4, sessionId: 6 }),
  ];

  it("shows the active session's oldest decision first", () => {
    expect(decisionToShow(pending, 1)?.id).toBe(2);
  });

  it("then the oldest of a session on screen, then the oldest overall", () => {
    expect(decisionToShow(pending, 9, new Set([6]))?.id).toBe(4);
    expect(decisionToShow(pending, 9)?.id).toBe(1);
    expect(decisionToShow(pending, null)?.id).toBe(1);
  });

  it("shows nothing while nothing waits", () => {
    expect(decisionToShow([], 1)).toBeNull();
  });
});

describe("freshDecisions", () => {
  it("reports each decision once", () => {
    const seen = new Set<number>();
    const first = { enabled: true, pending: [decision({ id: 1 })] };
    const fresh = freshDecisions(seen, first);
    expect(fresh.map((d) => d.id)).toEqual([1]);
    for (const d of fresh) seen.add(d.id);
    const second = { enabled: true, pending: [decision({ id: 1 }), decision({ id: 2 })] };
    expect(freshDecisions(seen, second).map((d) => d.id)).toEqual([2]);
    expect(freshDecisions(seen, first)).toEqual([]);
  });
});

describe("withGate", () => {
  const attention = new Map<number, AttentionState>([
    [1, "working"],
    [2, "attention"],
  ]);
  const reasons = new Map<number, AttentionReason>([[2, "bell"]]);

  it("marks sessions with a pending decision as waiting with the reason gate", () => {
    const waiting = waitingSessions({ enabled: true, pending: [decision({ sessionId: 1 })] });
    const view = withGate({ attention, reasons }, waiting);
    expect(view.attention.get(1)).toBe("attention");
    expect(view.reasons.get(1)).toBe("gate");
    expect(view.reasons.get(2)).toBe("bell");
    // The tracker's own maps stay untouched.
    expect(attention.get(1)).toBe("working");
  });

  it("returns the tracker's maps unchanged while nothing waits", () => {
    const view = { attention, reasons };
    expect(withGate(view, waitingSessions(EMPTY_GATE))).toBe(view);
  });
});

describe("gateHint", () => {
  it("names why a session's edits are not paused, and stays quiet otherwise", () => {
    expect(gateHint("noEndpoint", t)).toBe(t("gate.noEndpoint"));
    expect(gateHint("launcher", t)).toBe(t("gate.launcher"));
    expect(gateHint("profileSettings", t)).toBe(t("gate.profileSettings"));
    expect(gateHint("active", t)).toBeNull();
    expect(gateHint("none", t)).toBeNull();
    expect(gateHint(undefined, t)).toBeNull();
  });

  it("promises no protection in the setting's help", () => {
    const help = t("settings.gateHelp");
    expect(help).toContain("no protection");
    expect(help).toContain("WSL");
  });
});

describe("gateSetup", () => {
  const never = () => new Promise<void>(() => undefined);

  it("lets the restore start only after the first setting reached the backend", async () => {
    const order: string[] = [];
    let answer: () => void = () => undefined;
    const setup = gateSetup(
      () =>
        new Promise<void>((resolve) => {
          answer = () => {
            order.push("configured");
            resolve();
          };
        }),
      never,
    );
    const restore = setup.ready.then(() => {
      order.push("restore");
    });
    const configuring = setup.configure(true);
    await Promise.resolve();
    expect(order).toEqual([]);
    answer();
    await Promise.all([configuring, restore]);
    expect(order).toEqual(["configured", "restore"]);
  });

  it("does not hold the restore when the backend refuses or never answers", async () => {
    const refused = gateSetup(() => Promise.reject(new Error("no backend")), never);
    await expect(refused.configure(true)).rejects.toThrow("no backend");
    await refused.ready;

    let elapse: () => void = () => undefined;
    const silent = gateSetup(
      () => new Promise(() => undefined),
      () =>
        new Promise<void>((resolve) => {
          elapse = resolve;
        }),
    );
    void silent.configure(true);
    elapse();
    await silent.ready;
  });

  it("passes every later setting on", async () => {
    const sent: boolean[] = [];
    const setup = gateSetup(async (enabled) => {
      sent.push(enabled);
      await Promise.resolve();
    }, never);
    await setup.configure(true);
    await setup.configure(false);
    expect(sent).toEqual([true, false]);
  });
});

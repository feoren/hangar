import { describe, expect, it } from "vitest";

import {
  DEFAULT_ROW_LAYOUT,
  findElement,
  hiddenElements,
  hideElement,
  loadSummary,
  matchingPreset,
  parseRowLayout,
  placeElement,
  planRow,
  presetLayout,
  ROW_ELEMENTS,
  ROW_LINES,
  ROW_PRESETS,
  rowAttributes,
  setFormByName,
  showElement,
  stepElement,
  type RowElement,
  type RowLayout,
} from "./rowLayout";

/** A layout's lines as "a b | c" strings, empty lines as "|". */
function lines(value: RowLayout): string[] {
  return value.lines.map((line) => `${line.left.join(" ")} | ${line.right.join(" ")}`.trim());
}

/** Every placed element, once each. */
function placed(value: RowLayout): RowElement[] {
  return value.lines.flatMap((line) => [...line.left, ...line.right]);
}

describe("the standard row", () => {
  it("is today's four lines with every element in its long-standing form", () => {
    expect(lines(DEFAULT_ROW_LAYOUT)).toEqual([
      "program signs title | state",
      "folder | branch",
      "load |",
      "runtime context | tokens",
    ]);
    expect(DEFAULT_ROW_LAYOUT.forms).toEqual({
      state: "word",
      program: "icon",
      folder: "text",
      branch: "text",
      load: "text",
      context: "bar",
    });
    expect(DEFAULT_ROW_LAYOUT.frame).toBe("flat");
    expect(DEFAULT_ROW_LAYOUT.density).toBe("normal");
    expect(hiddenElements(DEFAULT_ROW_LAYOUT)).toEqual([]);
    expect(presetLayout("standard")).toBe(DEFAULT_ROW_LAYOUT);
  });
});

describe("parsing a stored layout", () => {
  it("reads back every preset unchanged", () => {
    for (const preset of ROW_PRESETS) {
      const stored: unknown = JSON.parse(JSON.stringify(presetLayout(preset)));
      expect(parseRowLayout(stored)).toEqual(presetLayout(preset));
    }
  });

  it("gives the standard row for anything that is no layout", () => {
    for (const value of [undefined, null, "standard", 3, [], true]) {
      expect(parseRowLayout(value)).toEqual(DEFAULT_ROW_LAYOUT);
    }
  });

  it("drops unknown elements and shows each element only once, at its first place", () => {
    const value = parseRowLayout({
      lines: [
        { left: ["title", "price", 7, null, "state"], right: ["state", "branch"] },
        { left: ["branch", "folder"], right: "load" },
      ],
    });
    expect(lines(value)).toEqual(["title state | branch", "folder |", "|", "|"]);
    expect(new Set(placed(value)).size).toBe(placed(value).length);
  });

  it("keeps four lines: more are dropped, missing ones are empty", () => {
    const many = Array.from({ length: 9 }, (_, index) => ({
      left: [ROW_ELEMENTS[index] ?? "title"],
      right: [],
    }));
    const value = parseRowLayout({ lines: many });
    expect(value.lines).toHaveLength(ROW_LINES);
    expect(placed(value)).toEqual(["title", "state", "program", "signs"]);
    expect(parseRowLayout({ lines: [{ left: ["title"] }] }).lines).toHaveLength(ROW_LINES);
    expect(parseRowLayout({ lines: "none" }).lines).toHaveLength(ROW_LINES);
  });

  it("always shows the title", () => {
    expect(lines(parseRowLayout({ lines: [{ left: ["state"], right: [] }] }))[0]).toBe(
      "state title |",
    );
    expect(isTitled(parseRowLayout({ lines: [] }))).toBe(true);
    expect(isTitled(parseRowLayout({}))).toBe(true);
  });

  it("falls back to each default for unknown forms, frame and density", () => {
    const value = parseRowLayout({
      lines: [{ left: ["title"], right: [] }],
      forms: { state: "glow", context: "ring", load: 3, program: null },
      frame: "glass",
      density: "compact",
    });
    expect(value.forms).toEqual({ ...DEFAULT_ROW_LAYOUT.forms, context: "ring" });
    expect(value.frame).toBe("flat");
    expect(value.density).toBe("compact");
    expect(parseRowLayout({ lines: [], forms: "pill" }).forms).toEqual(DEFAULT_ROW_LAYOUT.forms);
  });

  it("does not read more than a bounded number of entries per part", () => {
    const value = parseRowLayout({
      lines: [{ left: [...Array<string>(100).fill("nothing"), "state"], right: [] }],
    });
    expect(isPlaced(value, "state")).toBe(false);
  });
});

function isTitled(value: RowLayout): boolean {
  return findElement(value, "title") !== null;
}

function isPlaced(value: RowLayout, element: RowElement): boolean {
  return findElement(value, element) !== null;
}

describe("moving a stored row style to its layout once", () => {
  it("turns each of the fourteen styles into its preset", () => {
    for (const preset of ROW_PRESETS) {
      expect(parseRowLayout(undefined, preset)).toEqual(presetLayout(preset));
    }
  });

  it("prefers a stored layout over the old style", () => {
    const own = placeElement(DEFAULT_ROW_LAYOUT, "tokens", { line: 0, side: "right", index: 0 });
    const stored: unknown = JSON.parse(JSON.stringify(own));
    expect(parseRowLayout(stored, "ring")).toEqual(own);
  });

  it("gives the standard row for an unknown style", () => {
    expect(parseRowLayout(undefined, "fancy")).toEqual(DEFAULT_ROW_LAYOUT);
  });

  it("builds every preset as the old style showed it, as close as elements allow", () => {
    expect(lines(presetLayout("ring"))).toEqual([
      "context program signs title | state",
      "folder | branch",
      "load | runtime",
      "|",
    ]);
    expect(presetLayout("ring").forms.context).toBe("ring");
    expect(lines(presetLayout("compact"))).toEqual([
      "program signs title | state",
      "folder | branch context",
      "|",
      "|",
    ]);
    expect(lines(presetLayout("accordion"))).toEqual([
      "program signs title | context state",
      "|",
      "|",
      "|",
    ]);
    expect(lines(presetLayout("counters"))[0]).toBe("program signs state title | runtime");
    expect(lines(presetLayout("columns"))[0]).toBe("program signs state title | context runtime");
    expect(presetLayout("columns").forms).toMatchObject({ state: "symbol", context: "number" });
    expect(presetLayout("card")).toMatchObject({
      frame: "card",
      forms: { state: "pill", program: "avatar", context: "ring" },
    });
    expect(presetLayout("tags").forms).toMatchObject({
      folder: "tag",
      branch: "tag",
      load: "pills",
    });
    for (const [preset, state] of [
      ["pill", "pill"],
      ["edge", "edge"],
      ["band", "band"],
    ] as const) {
      expect(presetLayout(preset).forms.state).toBe(state);
      expect(presetLayout(preset).lines).toEqual(DEFAULT_ROW_LAYOUT.lines);
    }
    expect(presetLayout("tiles").forms.load).toBe("tiles");
    expect(presetLayout("segments").forms.context).toBe("segments");
    expect(presetLayout("avatar").forms.program).toBe("avatar");
  });

  it("gives every preset its own layout, each element at most once and the title always", () => {
    const seen = new Set<string>();
    for (const preset of ROW_PRESETS) {
      const value = presetLayout(preset);
      seen.add(JSON.stringify(value));
      expect(new Set(placed(value)).size).toBe(placed(value).length);
      expect(isTitled(value)).toBe(true);
      expect(value.lines).toHaveLength(ROW_LINES);
      expect(matchingPreset(value)).toBe(preset);
    }
    expect(seen.size).toBe(ROW_PRESETS.length);
  });
});

describe("moving elements", () => {
  it("puts an element before the one at a place, also within its own part", () => {
    const moved = placeElement(DEFAULT_ROW_LAYOUT, "title", { line: 0, side: "left", index: 0 });
    expect(lines(moved)[0]).toBe("title program signs | state");
    // At the part's end: back where it was.
    const back = placeElement(moved, "title", { line: 0, side: "left", index: 3 });
    expect(lines(back)[0]).toBe("program signs title | state");
  });

  it("moves an element to another line and part", () => {
    const moved = placeElement(DEFAULT_ROW_LAYOUT, "tokens", { line: 1, side: "left", index: 1 });
    expect(lines(moved)).toEqual([
      "program signs title | state",
      "folder tokens | branch",
      "load |",
      "runtime context |",
    ]);
  });

  it("places a hidden element, and clamps a place out of range", () => {
    const hidden = hideElement(DEFAULT_ROW_LAYOUT, "load");
    const moved = placeElement(hidden, "load", { line: 9, side: "right", index: 99 });
    expect(lines(moved)[3]).toBe("runtime context | tokens load");
    expect(lines(placeElement(hidden, "load", { line: -2, side: "left", index: -1 }))[0]).toBe(
      "load program signs title | state",
    );
  });

  it("hides an element but never the title", () => {
    const hidden = hideElement(DEFAULT_ROW_LAYOUT, "branch");
    expect(lines(hidden)[1]).toBe("folder |");
    expect(hiddenElements(hidden)).toEqual(["branch"]);
    expect(hideElement(DEFAULT_ROW_LAYOUT, "title")).toBe(DEFAULT_ROW_LAYOUT);
    expect(hideElement(hidden, "branch")).toBe(hidden);
  });

  it("shows a hidden element where the standard row has it, at the end of that part", () => {
    const value = hideElement(hideElement(DEFAULT_ROW_LAYOUT, "signs"), "state");
    expect(lines(showElement(value, "signs"))[0]).toBe("program title signs |");
    expect(lines(showElement(value, "state"))[0]).toBe("program title | state");
    expect(showElement(DEFAULT_ROW_LAYOUT, "state")).toBe(DEFAULT_ROW_LAYOUT);
  });

  it("steps left and right through the line, across from one part into the other", () => {
    const right = stepElement(DEFAULT_ROW_LAYOUT, "signs", "right");
    expect(lines(right)[0]).toBe("program title signs | state");
    const across = stepElement(right, "signs", "right");
    expect(lines(across)[0]).toBe("program title | signs state");
    expect(lines(stepElement(across, "signs", "right"))[0]).toBe("program title | state signs");
    const end = stepElement(stepElement(across, "signs", "right"), "signs", "right");
    expect(lines(end)[0]).toBe("program title | state signs");
    expect(lines(stepElement(across, "signs", "left"))[0]).toBe("program title signs | state");
    expect(stepElement(DEFAULT_ROW_LAYOUT, "program", "left")).toBe(DEFAULT_ROW_LAYOUT);
  });

  it("steps up and down to the same part, at the same place where there is one", () => {
    const down = stepElement(DEFAULT_ROW_LAYOUT, "state", "down");
    expect(lines(down).slice(0, 2)).toEqual(["program signs title |", "folder | state branch"]);
    const up = stepElement(DEFAULT_ROW_LAYOUT, "context", "up");
    expect(lines(up).slice(2)).toEqual(["load context |", "runtime | tokens"]);
    expect(stepElement(DEFAULT_ROW_LAYOUT, "title", "up")).toBe(DEFAULT_ROW_LAYOUT);
    expect(stepElement(DEFAULT_ROW_LAYOUT, "tokens", "down")).toBe(DEFAULT_ROW_LAYOUT);
    expect(stepElement(hideElement(DEFAULT_ROW_LAYOUT, "load"), "load", "up")).toEqual(
      hideElement(DEFAULT_ROW_LAYOUT, "load"),
    );
  });

  it("changes a form by its name and ignores unknown names", () => {
    expect(setFormByName(DEFAULT_ROW_LAYOUT, "context", "ring").forms.context).toBe("ring");
    expect(setFormByName(DEFAULT_ROW_LAYOUT, "branch", "tag").forms).toEqual({
      ...DEFAULT_ROW_LAYOUT.forms,
      branch: "tag",
    });
    expect(setFormByName(DEFAULT_ROW_LAYOUT, "state", "ring").forms.state).toBe("word");
  });

  it("knows the preset a layout equals, and none for one's own", () => {
    expect(matchingPreset(DEFAULT_ROW_LAYOUT)).toBe("standard");
    expect(matchingPreset(stepElement(DEFAULT_ROW_LAYOUT, "signs", "right"))).toBeNull();
  });
});

describe("the plan for drawing", () => {
  it("leaves out empty lines and finds the title", () => {
    const plan = planRow(presetLayout("compact"));
    expect(plan.lines).toHaveLength(2);
    expect(plan.lines[0]).toMatchObject({ title: true, titleLeft: true, beside: null });
    expect(plan.lines[1]).toMatchObject({ title: false, titleLeft: false });
    expect(plan).toMatchObject({ lead: null, trail: null, state: "word", avatar: false });
  });

  it("takes an avatar or ring at the first line's ends beside the first two lines", () => {
    const ring = planRow(presetLayout("ring"));
    expect(ring.lead).toBe("ring");
    expect(ring.lines[0]?.left).toEqual(["program", "signs", "title"]);
    expect(ring.lines.map((line) => line.beside)).toEqual([1, 2, null]);
    const card = planRow(presetLayout("card"));
    expect(card).toMatchObject({ lead: "avatar", trail: "ring", avatar: true, state: "pill" });
    expect(card.lines[0]?.right).toEqual(["state"]);
    // A ring in the middle of a line stays where it is.
    const middle = planRow(setFormByName(DEFAULT_ROW_LAYOUT, "context", "ring"));
    expect(middle.lead).toBeNull();
    expect(middle.lines[3]?.left).toEqual(["runtime", "context"]);
  });

  it("asks the peek for what the row leaves out", () => {
    expect(planRow(DEFAULT_ROW_LAYOUT)).toMatchObject({ peekLoad: false, peekContext: false });
    expect(planRow(presetLayout("compact"))).toMatchObject({ peekLoad: true, peekContext: true });
    expect(planRow(hideElement(DEFAULT_ROW_LAYOUT, "context"))).toMatchObject({
      peekLoad: false,
      peekContext: true,
    });
  });

  it("names the row-wide looks as attributes", () => {
    expect(rowAttributes(planRow(DEFAULT_ROW_LAYOUT))).toEqual({
      "data-row-state": "word",
      "data-row-frame": "flat",
      "data-row-density": "normal",
    });
    expect(rowAttributes(planRow(presetLayout("card")))["data-row-frame"]).toBe("card");
    expect(rowAttributes(planRow(hideElement(DEFAULT_ROW_LAYOUT, "state")))).toMatchObject({
      "data-row-state": "none",
    });
  });
});

describe("load for the peek", () => {
  const names = { cpu: "CPU", memory: "RAM", gpu: "GPU" };

  it("names each value that is there", () => {
    expect(
      loadSummary(
        { cpu: "12%", memory: "476 MB", gpu: "30% · 1.2 GB", gpuLoad: "30%", gpuMemory: "1.2 GB" },
        names,
      ),
    ).toBe("CPU 12% · RAM 476 MB · GPU 30% · 1.2 GB");
    expect(
      loadSummary({ cpu: null, memory: "64 MB", gpu: null, gpuLoad: null, gpuMemory: null }, names),
    ).toBe("RAM 64 MB");
  });
});

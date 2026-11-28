import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { CONTEXT_SEGMENTS } from "./ContextLine";
import { createTranslator, I18nContext } from "./i18n";
import type { ResourceParts } from "./resources";
import {
  DEFAULT_ROW_LAYOUT,
  hideElement,
  parseRowLayout,
  placeElement,
  planRow,
  presetLayout,
  ROW_PRESETS,
  setFormByName,
  showElement,
  type RowLayout,
} from "./rowLayout";
import { RowLayoutField, RowLayoutPreview, RowLayoutSetting } from "./RowLayoutSettings";
import { SessionRowBody, type RowBodyProps } from "./SessionRow";
import { SessionLoad } from "./SessionLoad";

const english = createTranslator("en");

function render(element: ReactElement): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={english}>{element}</I18nContext.Provider>,
  );
}

/** How many elements carry `name` among their classes. */
function count(html: string, name: string): number {
  return [...html.matchAll(/class="([^"]*)"/g)].filter((match) =>
    (match[1] ?? "").split(" ").includes(name),
  ).length;
}

const USAGE: ResourceParts = {
  cpu: "12%",
  memory: "476 MB",
  gpu: null,
  gpuLoad: null,
  gpuMemory: null,
};

const PROGRAM = { kind: "letters", text: "C", color: "#d97757" } as const;

function row(value: RowLayout, extra: Partial<RowBodyProps> = {}): string {
  return render(
    <SessionRowBody
      plan={planRow(value)}
      name="hangar"
      badge={null}
      program={PROGRAM}
      marker={undefined}
      line={{ word: "working", full: "Claude Code is working" }}
      waiting={false}
      chip="working"
      signs={<span className="session-signs">sign</span>}
      place="hangar"
      branch="main"
      team={false}
      usage={USAGE}
      gpuColumn={false}
      runtime="1 hr"
      context={{ used: 215_500, window: 1_000_000 }}
      language="en"
      {...extra}
    />,
  );
}

/** The class lists of the row's lines, top to bottom. */
function lineClasses(html: string): string[] {
  return [...html.matchAll(/class="(row-line[^"]*)"/g)].map((match) => match[1] ?? "");
}

describe("the standard row", () => {
  const html = row(DEFAULT_ROW_LAYOUT);

  it("keeps its four lines: title and state, folder and branch, load, run time and context", () => {
    expect(lineClasses(html)).toEqual([
      "row-line session-title-row",
      "row-line",
      "row-line",
      "row-line",
    ]);
    for (const name of [
      "session-title",
      "session-path",
      "session-branch",
      "session-load",
      "session-runtime",
      "session-context-bar",
      "session-context-tokens",
      "state-dot",
      "session-status",
    ]) {
      expect(count(html, name)).toBe(1);
    }
  });

  it("says its state in words and adds none of the other forms' parts", () => {
    expect(html).toContain('<span class="session-status">working</span>');
    for (const name of [
      "status-chip",
      "session-avatar",
      "session-ring",
      "session-context-mini",
      "session-context-segments",
      "session-shape",
      "session-resource-label",
      "tag",
      "has-lead",
    ]) {
      expect(count(html, name)).toBe(0);
    }
  });

  it("keeps title, state and signs in that order for screen readers, the signs drawn first", () => {
    const title = html.indexOf('class="session-title"');
    const full = html.indexOf("Claude Code is working");
    const state = html.indexOf('class="session-state state-word"');
    const signs = html.indexOf('class="row-before-title"');
    expect(title).toBeGreaterThan(0);
    expect(full).toBeGreaterThan(title);
    expect(state).toBeGreaterThan(full);
    expect(signs).toBeGreaterThan(state);
    expect(html).toContain('<span class="row-before-title"><span class="session-signs">sign');
  });

  it("pushes the right part to the edge with one spacer per line that has one", () => {
    expect(count(html, "row-push")).toBe(3);
  });

  it("shows the program's small icon only where the group does not name the program", () => {
    expect(count(html, "badge")).toBe(0);
    expect(count(row(DEFAULT_ROW_LAYOUT, { badge: PROGRAM }), "badge")).toBe(1);
  });

  it("drops a line without anything to show", () => {
    const bare = row(DEFAULT_ROW_LAYOUT, { place: null, branch: null, usage: null });
    expect(lineClasses(bare)).toHaveLength(2);
    const quiet = row(DEFAULT_ROW_LAYOUT, { runtime: null, context: undefined, usage: null });
    expect(count(quiet, "session-runtime")).toBe(0);
  });

  it("says the context's numbers once, also when the row hides the context", () => {
    expect(html.match(/Context 22% full/g)).toHaveLength(1);
    const hidden = row(hideElement(DEFAULT_ROW_LAYOUT, "context"));
    expect(count(hidden, "session-context-bar")).toBe(0);
    expect(hidden).toContain("Context 22% full");
  });
});

describe("the forms", () => {
  it("state: a tinted pill, a solid band pill, a tint without the dot, or the bare symbol", () => {
    const pill = row(presetLayout("pill"));
    expect(pill).toContain('class="session-state state-pill"');
    expect(count(pill, "state-dot")).toBe(1);
    const band = row(presetLayout("band"));
    expect(band).toContain('class="session-state state-band"');
    expect(count(band, "state-dot")).toBe(0);
    expect(count(row(presetLayout("edge")), "state-dot")).toBe(0);
    const symbol = row(setFormByName(DEFAULT_ROW_LAYOUT, "state", "symbol"));
    expect(symbol).toContain('class="status-chip working"');
    expect(count(symbol, "session-status")).toBe(0);
    expect(symbol).toContain("Claude Code is working");
    expect(row(setFormByName(DEFAULT_ROW_LAYOUT, "state", "symbol"), { chip: null })).toContain(
      '<span class="session-shape" aria-hidden="true"><span class="state-dot"></span></span>',
    );
  });

  it("program: an avatar with the state dot on its corner, beside the first two lines", () => {
    const html = row(presetLayout("avatar"));
    expect(html).toMatch(
      /class="row-span row-lead"><span class="session-avatar"[^>]*><span class="badge"[^>]*>C<\/span><span class="state-dot">/,
    );
    expect(html).toContain('class="session-text has-lead"');
    expect(lineClasses(html)).toEqual([
      "row-line session-title-row beside beside-1",
      "row-line beside beside-2",
      "row-line",
      "row-line",
    ]);
    // The corner dot says the state; the word drops its own.
    expect(count(html, "state-dot")).toBe(1);
  });

  it("context: a ring beside the lines, a faint empty one without numbers", () => {
    const html = row(presetLayout("ring"));
    expect(html).toContain('<span class="session-ring-value">22</span>');
    expect(html).toMatch(/class="row-span row-lead"><span class="session-ring/);
    expect(row(presetLayout("ring"), { context: undefined })).toContain(
      'class="session-ring empty"',
    );
  });

  it("card: avatar at the start, ring at the end, the run time on the load line", () => {
    const html = row(presetLayout("card"));
    expect(html).toContain('class="session-text has-lead has-trail"');
    expect(html.indexOf("row-trail")).toBeGreaterThan(html.indexOf("session-branch"));
    expect(html).toMatch(/class="session-load row-fact".*class="row-push".*session-runtime/);
  });

  it("context: ten segments, a small bar or the share alone", () => {
    const segments = row(presetLayout("segments"));
    expect(count(segments, "session-context-segments")).toBe(1);
    expect(count(segments, "session-context-bar")).toBe(0);
    expect(count(segments, "segment-on")).toBe(2);
    const html = render(
      <SessionRowBody
        {...rowProps()}
        plan={planRow(presetLayout("segments"))}
        context={{ used: 215_500, window: 1_000_000 }}
      />,
    );
    expect((html.match(/<span( class="segment-on")?><\/span>/g) ?? []).length).toBe(
      CONTEXT_SEGMENTS,
    );
    const compact = row(presetLayout("compact"));
    expect(lineClasses(compact)).toHaveLength(2);
    expect(compact).toMatch(/class="session-branch row-where".*class="session-context-mini"/);
    expect(compact).toContain('<span class="session-context-share">22%</span>');
    const number = row(presetLayout("columns"));
    expect(number).toContain('class="session-context row-fact"');
    expect(count(number, "session-context-bar")).toBe(0);
  });

  it("load: outlined pills or tiles with names; folder and branch as tags", () => {
    const tags = row(presetLayout("tags"));
    expect(tags).toContain('class="session-load row-fact pills"');
    expect(tags).toContain('class="session-path row-where tag"');
    expect(tags).toContain('class="session-branch row-where tag"');
    const tiles = row(presetLayout("tiles"));
    expect(tiles).toContain('class="session-load row-fact tiles"');
    expect(count(tiles, "session-resource-label")).toBe(2);
  });

  it("counters and columns: the state symbol before the title, the run time at its end", () => {
    for (const preset of ["counters", "columns"] as const) {
      const html = row(presetLayout(preset));
      expect(html).toContain('class="session-shape"');
      expect(html).toMatch(/class="row-push".*class="session-runtime row-fact"/);
      expect(count(html, "session-runtime")).toBe(1);
    }
  });

  it("puts what stands before the title in the title's part only", () => {
    // The title at the right edge: everything keeps its order, nothing moves in front.
    const value = parseRowLayout({ lines: [{ left: ["state"], right: ["signs", "title"] }] });
    const html = row(value);
    expect(count(html, "row-before-title")).toBe(0);
    expect(html.indexOf("session-state")).toBeLessThan(html.indexOf("session-signs"));
  });
});

function rowProps(): RowBodyProps {
  return {
    plan: planRow(DEFAULT_ROW_LAYOUT),
    name: "hangar",
    badge: null,
    program: PROGRAM,
    marker: undefined,
    line: { word: "working", full: "Claude Code is working" },
    waiting: false,
    chip: "working",
    signs: null,
    place: null,
    branch: null,
    team: false,
    usage: null,
    gpuColumn: false,
    runtime: null,
    context: undefined,
    language: "en",
  };
}

describe("tiles", () => {
  it("name each value", () => {
    const html = render(<SessionLoad usage={USAGE} gpuColumn form="tiles" />);
    expect(html).toContain('<span class="session-resource-label" aria-hidden="true">CPU</span>');
    expect(html).toContain('<span class="session-resource-label" aria-hidden="true">RAM</span>');
    expect(html).toContain('<span class="session-resource-label" aria-hidden="true">GPU</span>');
  });
});

describe("the settings' preview", () => {
  it("draws a group of two rows for every preset, inert and hidden from screen readers", () => {
    for (const preset of ROW_PRESETS) {
      const html = render(<RowLayoutPreview plan={planRow(presetLayout(preset))} language="en" />);
      expect(html).toContain('aria-hidden="true" inert=""');
      expect(count(html, "session")).toBe(2);
      expect(count(html, "agent-group")).toBe(1);
      expect(count(html, "tree-guide")).toBe(2);
    }
  });

  it("carries the row-wide looks as attributes", () => {
    const html = render(<RowLayoutPreview plan={planRow(presetLayout("card"))} language="en" />);
    expect(html).toContain('data-row-frame="card"');
    expect(html).toContain('data-row-state="pill"');
    expect(html).toContain('data-row-density="normal"');
  });
});

describe("the builder", () => {
  const html = render(
    <RowLayoutField value={DEFAULT_ROW_LAYOUT} language="en" onChange={() => undefined} />,
  );

  it("shows four lines with a left and a right part each", () => {
    expect(count(html, "row-builder-line")).toBe(4);
    expect(count(html, "row-builder-slot")).toBe(8);
    expect(html).toContain('aria-label="Line 1, left"');
    expect(html).toContain('aria-label="Line 4, right"');
  });

  it("gives every element a chip with its form and a way to hide it, the title none", () => {
    // Twelve placed elements and the cost, which no template shows, among the hidden ones.
    expect(count(html, "row-chip")).toBe(13);
    expect(html).toContain('aria-label="Form of Context"');
    expect(html).toContain('aria-label="Hide State"');
    expect(html).not.toContain('aria-label="Hide Title"');
    expect(html).toContain('aria-label="Show Cost"');
    const everything = render(
      <RowLayoutField
        value={showElement(DEFAULT_ROW_LAYOUT, "cost")}
        language="en"
        onChange={() => undefined}
      />,
    );
    expect(everything).toContain("Everything is shown.");
  });

  it("offers hidden elements back and names the preset in use", () => {
    const hidden = render(
      <RowLayoutField
        value={hideElement(DEFAULT_ROW_LAYOUT, "tokens")}
        language="en"
        onChange={() => undefined}
      />,
    );
    expect(hidden).toContain('aria-label="Show Tokens"');
    expect(hidden).toContain('<option value="" selected="">Your own layout</option>');
    expect(html).toContain('<option value="standard" selected="">Standard</option>');
    expect(html).toMatch(/<button type="button" class="secondary-button" disabled="">Reset/);
  });
});

describe("the builder's line in the settings", () => {
  it("stays closed and names the template in use, or your own layout", () => {
    const standard = render(
      <RowLayoutSetting value={DEFAULT_ROW_LAYOUT} language="en" onChange={() => undefined} />,
    );
    expect(standard).toContain('aria-expanded="false"');
    expect(standard).toContain("Standard");
    expect(count(standard, "row-builder-line")).toBe(0);
    const own = render(
      <RowLayoutSetting
        value={hideElement(DEFAULT_ROW_LAYOUT, "tokens")}
        language="en"
        onChange={() => undefined}
      />,
    );
    expect(own).toContain("Your own layout");
  });
});

describe("a session running as a sub-agent", () => {
  const session = { subAgent: "hangar-backend", cwd: "C:\\src\\hangar" };

  it("shows the figure with the program's badge on its corner in the avatar form", () => {
    const html = row(presetLayout("avatar"), { session, programName: "Claude Code" });
    expect(html).toMatch(
      /class="session-avatar"[^>]*><span class="session-figure full" title="Claude Code as hangar-backend" data-agent="hangar-backend"><span class="agent-figure full"/,
    );
    expect(html).toMatch(/class="figure-corner" aria-hidden="true"><span class="badge"[^>]*>C</);
  });

  it("shows the figure in a program group too, where the badge itself stays away", () => {
    const plain = row(DEFAULT_ROW_LAYOUT);
    const figured = row(DEFAULT_ROW_LAYOUT, { session });
    expect(count(plain, "session-figure")).toBe(0);
    expect(count(plain, "badge")).toBe(0);
    expect(count(figured, "session-figure")).toBe(1);
    expect(count(figured, "figure-corner")).toBe(1);
  });

  it("leaves a session without one as it was", () => {
    const html = row(presetLayout("avatar"), { session: { subAgent: null, cwd: "C:\\src" } });
    expect(count(html, "session-figure")).toBe(0);
    expect(html).toMatch(/class="session-avatar"[^>]*><span class="badge"/);
  });
});

describe("the cost on the row", () => {
  const withCost = showElement(DEFAULT_ROW_LAYOUT, "cost");

  it("shows the estimated cost as the cost overview writes it, with its name for screen readers", () => {
    const html = row(withCost, { cost: "~$0.42+" });
    expect(count(html, "session-cost")).toBe(1);
    expect(html).toContain("~$0.42+");
    expect(html).toMatch(/class="sr-only">Cost <\/span>/);
  });

  it("shows nothing for a program without usage, and nothing while it is hidden", () => {
    expect(count(row(withCost, { cost: null }), "session-cost")).toBe(0);
    expect(count(row(withCost), "session-cost")).toBe(0);
    expect(count(row(DEFAULT_ROW_LAYOUT, { cost: "~$0.42" }), "session-cost")).toBe(0);
  });

  it("alone does not keep a line that shows nothing else", () => {
    const only = placeElement(DEFAULT_ROW_LAYOUT, "cost", { line: 2, side: "right", index: 0 });
    const html = row(only, { usage: null, cost: null });
    expect(lineClasses(html)).toHaveLength(3);
    expect(lineClasses(row(only, { usage: null, cost: "~$1.00" }))).toHaveLength(4);
  });

  it("shows in the settings' preview once placed", () => {
    const html = render(<RowLayoutPreview plan={planRow(withCost)} language="en" />);
    expect(count(html, "session-cost")).toBe(2);
    expect(html).toContain("~$3.42");
  });
});

describe("the output rate on the row", () => {
  it("shows tokens per second on the title line, the sentence for screen readers", () => {
    const html = row(DEFAULT_ROW_LAYOUT, { speed: { value: 1234, live: true } });
    expect(count(html, "session-speed")).toBe(1);
    expect(count(html, "settled")).toBe(0);
    expect(html).toContain(">1,234 tok/s<");
    expect(html).toContain("Claude Code is writing 1,234 tokens per second");
    // On the title's line, just before the state.
    const title = /class="row-line session-title-row">([\s\S]*?)<\/span><span class="row-line/.exec(
      html,
    );
    expect(title?.[1]).toMatch(/session-speed[\s\S]*session-state/);
  });

  it("keeps the last response's rate, quieter, once it is no longer live", () => {
    const html = row(DEFAULT_ROW_LAYOUT, { speed: { value: 84, live: false } });
    expect(html).toContain('class="session-speed row-fact settled"');
    expect(html).toContain(">84 tok/s<");
    expect(html).toContain("Last response: 84 tokens per second");
    expect(html).not.toContain("is writing");
  });

  it("shows nothing before the first measured response, and nothing while it is hidden", () => {
    expect(count(row(DEFAULT_ROW_LAYOUT, { speed: null }), "session-speed")).toBe(0);
    expect(count(row(DEFAULT_ROW_LAYOUT), "session-speed")).toBe(0);
    expect(row(DEFAULT_ROW_LAYOUT)).not.toContain("tok/s");
    const hidden = hideElement(DEFAULT_ROW_LAYOUT, "speed");
    expect(count(row(hidden, { speed: { value: 84, live: true } }), "session-speed")).toBe(0);
  });

  it("shows in the settings' preview for the working session only", () => {
    const html = render(<RowLayoutPreview plan={planRow(DEFAULT_ROW_LAYOUT)} language="en" />);
    expect(count(html, "session-speed")).toBe(1);
    expect(html).toContain(">84 tok/s<");
  });
});

describe("the sub-agent count on the badge", () => {
  const dot = <span className="subagent-count badge-count">2</span>;

  it("sits on the program's badge, outside the lines' flow", () => {
    const html = row(DEFAULT_ROW_LAYOUT, { badge: PROGRAM, badgeCount: dot });
    expect(count(html, "badge-anchor")).toBe(1);
    expect(html).toMatch(
      /class="badge-anchor"><span class="badge"[^>]*>C<\/span><span class="subagent-count badge-count">2<\/span><\/span>/,
    );
    const avatar = row(presetLayout("avatar"), { badgeCount: dot });
    expect(avatar).toMatch(/class="session-avatar"[^>]*><span class="badge-anchor">/);
  });

  it("is left out where the row shows no badge, and adds nothing without a count", () => {
    expect(count(row(DEFAULT_ROW_LAYOUT, { badgeCount: dot }), "badge-count")).toBe(0);
    const none = row(DEFAULT_ROW_LAYOUT, { badge: PROGRAM, badgeCount: null });
    expect(count(none, "badge-anchor")).toBe(0);
  });
});

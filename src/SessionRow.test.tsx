import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { ContextLineCells, CONTEXT_SEGMENTS } from "./ContextLine";
import { createTranslator, I18nContext } from "./i18n";
import type { ResourceParts } from "./resources";
import { ROW_STYLES, rowParts, type RowStyle } from "./rowStyle";
import { RowStylePreview } from "./RowStyleSettings";
import { GroupStateCounts, RowExpander, SessionRowBody, type RowBodyProps } from "./SessionRow";
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

function row(style: RowStyle, extra: Partial<RowBodyProps> = {}): string {
  return render(
    <SessionRowBody
      parts={rowParts(style)}
      name="hangar"
      badge={null}
      program={{ kind: "letters", text: "C", color: "#d97757" }}
      marker={undefined}
      line={{ word: "working", full: "Claude Code is working" }}
      waiting={false}
      chip="working"
      signs={null}
      place="hangar"
      branch="main"
      team={false}
      usage={USAGE}
      gpuColumn={false}
      runtime="1 hr"
      context={{ used: 215_500, window: 1_000_000 }}
      language="en"
      moreId="more-1"
      {...extra}
    />,
  );
}

describe("the standard row", () => {
  const html = row("standard");

  it("keeps its four lines: title and state, folder and branch, load, run time and context", () => {
    for (const name of [
      "session-title-row",
      "session-meta",
      "session-detail",
      "session-context",
      "session-context-bar",
      "state-dot",
      "session-status",
    ]) {
      expect(count(html, name)).toBe(1);
    }
  });

  it("says its state in words and adds none of the other styles' parts", () => {
    expect(html).toContain('<span class="session-status" aria-hidden="true">working</span>');
    for (const name of [
      "status-chip",
      "session-avatar",
      "session-ring",
      "session-more",
      "session-context-mini",
      "session-context-segments",
      "session-columns",
      "session-shape",
      "session-resource-label",
    ]) {
      expect(count(html, name)).toBe(0);
    }
  });

  it("is the same markup in the styles that are pure CSS", () => {
    for (const style of ["pill", "tags", "edge", "band"] as const) {
      expect(row(style)).toBe(html);
    }
  });
});

describe("the parts of each style", () => {
  it("avatar: the program's badge with the state dot on its corner", () => {
    const html = row("avatar");
    expect(count(html, "session-avatar")).toBe(1);
    expect(html).toMatch(
      /class="session-avatar"[^>]*><span class="badge"[^>]*>C<\/span><span class="state-dot">/,
    );
  });

  it("accordion: the details in a folding wrapper with the chevron's id, a small bar on top", () => {
    const html = row("accordion");
    expect(html).toContain(
      '<span class="session-more" id="more-1"><span class="session-more-inner">',
    );
    expect(count(html, "session-context-mini")).toBe(1);
    expect(html.indexOf("session-context-mini")).toBeLessThan(html.indexOf("session-more"));
  });

  it("ring: a ring with the share, the run time on the load line, no fourth line", () => {
    const html = row("ring");
    expect(count(html, "session-ring")).toBe(1);
    expect(html).toContain('<span class="session-ring-value">22</span>');
    expect(html).toMatch(/class="session-detail">.*class="session-runtime"/);
    expect(count(html, "session-context")).toBe(0);
    // The context still reaches screen readers.
    expect(html).toContain("Context 22% full");
  });

  it("ring: a faint empty ring without numbers", () => {
    const html = row("ring", { context: undefined });
    expect(html).toContain('class="session-ring empty"');
    expect(count(html, "session-ring-value")).toBe(0);
  });

  it("card: avatar without a dot, the ring at the end", () => {
    const html = row("card");
    expect(count(html, "session-avatar")).toBe(1);
    expect(html).not.toMatch(/class="session-avatar"[^>]*>.*?<span class="state-dot">/);
    expect(html.lastIndexOf("session-ring")).toBeGreaterThan(html.indexOf("session-meta"));
  });

  it("tiles: the load with names", () => {
    const html = row("tiles");
    expect(html).toContain('class="session-load tiles"');
    expect(count(html, "session-resource-label")).toBe(2);
  });

  it("segments: ten cells instead of the striped bar", () => {
    const html = row("segments");
    expect(count(html, "session-context-segments")).toBe(1);
    expect(count(html, "session-context-bar")).toBe(0);
    expect(count(html, "segment-on")).toBe(2);
  });

  it("compact: two lines, the small bar with its share on the folder line", () => {
    const html = row("compact");
    expect(count(html, "session-detail")).toBe(0);
    expect(count(html, "session-context")).toBe(0);
    expect(html).toMatch(/class="session-meta">.*class="session-context-mini"/);
    expect(html).toContain('<span class="session-context-share">22%</span>');
  });

  it("counters: the state as its shape, the run time where the word was", () => {
    const html = row("counters");
    expect(count(html, "session-status")).toBe(0);
    expect(html).toContain('<span class="session-state-runtime" aria-hidden="true">1 hr</span>');
    expect(html).toContain('class="session-shape"');
    expect(html).toContain('class="status-chip working"');
    // The run time is not repeated on the fourth line.
    expect(count(html, "session-runtime")).toBe(0);
    expect(html).toContain("Claude Code is working");
  });

  it("columns: CPU, share and run time in cells, no load or context line", () => {
    const html = row("columns");
    expect(html).toContain(
      '<span class="session-column session-column-cpu" aria-hidden="true">12%</span>',
    );
    expect(html).toContain(
      '<span class="session-column session-column-context" aria-hidden="true">22%</span>',
    );
    expect(html).toContain(
      '<span class="session-column session-column-runtime" aria-hidden="true">1 hr</span>',
    );
    expect(count(html, "session-detail")).toBe(0);
    expect(count(html, "session-context")).toBe(0);
  });

  it("shows a starting session's dot where no chip exists", () => {
    expect(row("columns", { chip: null })).toContain(
      '<span class="session-shape" aria-hidden="true"><span class="state-dot"></span></span>',
    );
  });
});

describe("the chevron of a folding row", () => {
  it("is a button that says whether the details show and which ones", () => {
    const open = render(<RowExpander expanded controls="more-1" onToggle={() => undefined} />);
    expect(open).toContain('aria-expanded="true"');
    expect(open).toContain('aria-controls="more-1"');
    expect(open).toContain('aria-label="Show or hide details"');
    expect(open).toMatch(/^<button type="button" class="session-expander"/);
    const folded = render(
      <RowExpander expanded={false} controls="more-1" onToggle={() => undefined} />,
    );
    expect(folded).toContain('aria-expanded="false"');
  });
});

describe("the state counts on a group header", () => {
  it("shows each state's shape with its number and says them in words", () => {
    const html = render(
      <GroupStateCounts
        counts={[
          { kind: "working", count: 2 },
          { kind: "asks", count: 1 },
        ]}
      />,
    );
    expect(count(html, "agent-group-state")).toBe(2);
    expect(html).toContain('class="status-chip working"');
    expect(html).toContain('class="status-chip asks"');
    expect(html).toContain('<span class="agent-group-state-count">2</span>');
    expect(html).toContain('<span class="sr-only">working: 2, question: 1</span>');
  });

  it("is gone without counts", () => {
    expect(render(<GroupStateCounts counts={[]} />)).toBe("");
  });
});

describe("the fourth line's meters", () => {
  const context = { used: 215_500, window: 1_000_000 };

  it("lights one segment per tenth", () => {
    const html = render(
      <ContextLineCells runtime="1 hr" context={context} language="en" meter="segments" />,
    );
    expect(count(html, "session-context-segments")).toBe(1);
    expect((html.match(/<span( class="segment-on")?><\/span>/g) ?? []).length).toBe(
      CONTEXT_SEGMENTS,
    );
  });

  it("keeps only the words when another part shows the context and the run time", () => {
    const html = render(
      <ContextLineCells
        runtime="1 hr"
        context={context}
        language="en"
        meter="none"
        runtimeCell={false}
      />,
    );
    expect(html).toMatch(/^<span class="sr-only">Context 22% full[^<]*<\/span>$/);
  });
});

describe("tiles", () => {
  it("name each value", () => {
    const html = render(<SessionLoad usage={USAGE} gpuColumn tiles />);
    expect(html).toContain('<span class="session-resource-label" aria-hidden="true">CPU</span>');
    expect(html).toContain('<span class="session-resource-label" aria-hidden="true">RAM</span>');
    expect(html).toContain('<span class="session-resource-label" aria-hidden="true">GPU</span>');
  });
});

describe("the settings' preview", () => {
  it("draws a group of two rows in every style, inert and hidden from screen readers", () => {
    for (const style of ROW_STYLES) {
      const html = render(<RowStylePreview style={style} language="en" />);
      expect(html).toContain(`data-rows="${style}"`);
      expect(html).toContain('aria-hidden="true" inert=""');
      expect(count(html, "session")).toBe(2);
      expect(count(html, "agent-group")).toBe(1);
      expect(count(html, "tree-guide")).toBe(2);
    }
  });

  it("counts the states on the header only in the counters style", () => {
    expect(
      count(render(<RowStylePreview style="counters" language="en" />), "agent-group-state"),
    ).toBe(2);
    expect(
      count(render(<RowStylePreview style="standard" language="en" />), "agent-group-state"),
    ).toBe(0);
  });

  it("names the columns once above the list in the columns style", () => {
    expect(
      count(render(<RowStylePreview style="columns" language="en" />), "session-columns-head"),
    ).toBe(1);
    expect(
      count(render(<RowStylePreview style="pill" language="en" />), "session-columns-head"),
    ).toBe(0);
  });

  it("folds all but the active row in the accordion style", () => {
    const html = render(<RowStylePreview style="accordion" language="en" />);
    expect(html).toMatch(/class="session active[^"]* expanded"/);
    expect(html).toMatch(/class="session attention[^"]* folded"/);
    expect(count(html, "session-expander")).toBe(2);
  });
});

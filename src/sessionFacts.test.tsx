import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { SessionContext } from "./contextUsage";
import { createTranslator, I18nContext } from "./i18n";
import type { ResourceParts } from "./resources";
import { DEFAULT_ROW_LAYOUT, planRow } from "./rowLayout";
import { SessionLoad } from "./SessionLoad";
import { SessionRowBody } from "./SessionRow";

const BUSY: ResourceParts = {
  cpu: "12%",
  memory: "340 MB",
  gpu: "30% 1.2 GB",
  gpuLoad: "30%",
  gpuMemory: "1.2 GB",
};
const NO_GPU: ResourceParts = { ...BUSY, gpu: null, gpuLoad: null, gpuMemory: null };

const english = createTranslator("en");

function render(element: ReactElement): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={english}>{element}</I18nContext.Provider>,
  );
}

/** The class lists of every element carrying `name` in its classes. */
function cells(html: string, name: string): string[] {
  return [...html.matchAll(/class="([^"]*)"/g)]
    .map((match) => match[1] ?? "")
    .filter((classes) => classes.split(" ").includes(name));
}

describe("the load group of the third line", () => {
  it("puts each value in its own fixed cell, the GPU's two with a dot between", () => {
    const html = render(<SessionLoad usage={BUSY} gpuColumn />);
    expect(html).toContain(
      '<span class="session-value session-value-percent" aria-hidden="true">30%</span><span class="session-value-sep" aria-hidden="true">·</span><span class="session-value session-value-memory" aria-hidden="true">1.2 GB</span>',
    );
    expect(cells(html, "session-value-percent")).toHaveLength(2);
    expect(cells(html, "session-value-memory")).toHaveLength(2);
    expect(html).toContain('<span class="sr-only">CPU 12%</span>');
    expect(html).toContain('<span class="sr-only">GPU 30% 1.2 GB</span>');
  });

  it("shows icons instead of the names; only screen readers hear the names", () => {
    const html = render(<SessionLoad usage={BUSY} gpuColumn />);
    expect(cells(html, "session-resource-icon")).toHaveLength(3);
    expect(html).not.toContain("session-resource-name");
    const visible = html.replace(/<span class="sr-only">[^<]*<\/span>/g, "");
    for (const name of ["CPU", "RAM", "GPU"]) expect(visible).not.toContain(name);
  });

  it("drops the GPU's dot when only one of its values is there", () => {
    const html = render(
      <SessionLoad usage={{ ...NO_GPU, gpu: "64 MB", gpuMemory: "64 MB" }} gpuColumn />,
    );
    expect(html).toContain('<span class="session-value-sep" aria-hidden="true"></span>');
  });

  it("keeps an empty GPU column while the machine gives GPU numbers", () => {
    const html = render(<SessionLoad usage={NO_GPU} gpuColumn />);
    expect(cells(html, "session-resource-gpu")).toEqual([
      "session-resource session-resource-gpu vacant",
    ]);
    expect(html).toContain(
      'class="session-resource session-resource-gpu vacant" aria-hidden="true"',
    );
    expect(html).not.toContain('sr-only">GPU');
  });

  it("has no GPU column where the machine gives no GPU numbers", () => {
    const html = render(<SessionLoad usage={NO_GPU} gpuColumn={false} />);
    expect(cells(html, "session-resource-gpu")).toEqual([]);
  });

  it("shows a session's GPU use even before the machine's first GPU reading", () => {
    const html = render(<SessionLoad usage={BUSY} gpuColumn={false} />);
    expect(cells(html, "session-resource-gpu")).toEqual(["session-resource session-resource-gpu"]);
  });

  it("keeps the CPU column's place before the second reading", () => {
    const html = render(<SessionLoad usage={{ ...NO_GPU, cpu: null }} gpuColumn />);
    expect(cells(html, "session-resource-cpu")).toEqual([
      "session-resource session-resource-cpu vacant",
    ]);
    expect(html).not.toContain('sr-only">CPU');
  });
});

describe("the fourth line: run time and context", () => {
  const context = { used: 215_500, window: 1_000_000 };
  const plan = planRow(DEFAULT_ROW_LAYOUT);

  /** The standard row's fourth line, or "" when the row leaves it out. */
  function fourth(runtime: string | null, shown: SessionContext | undefined): string {
    const html = render(
      <SessionRowBody
        plan={plan}
        name="hangar"
        badge={null}
        program={{ kind: "letters", text: "C", color: "#d97757" }}
        marker={undefined}
        line={{ word: "working", full: "Claude Code is working" }}
        waiting={false}
        chip="working"
        signs={null}
        place={null}
        branch={null}
        team={false}
        usage={null}
        gpuColumn={false}
        runtime={runtime}
        context={shown}
        language="en"
      />,
    );
    const lines = html.split('<span class="row-line').slice(1);
    return lines.find((line) => line.includes("session-runtime")) ?? "";
  }

  it("is gone without a run time and context numbers, so no empty line", () => {
    expect(fourth(null, undefined)).toBe("");
  });

  it("shows the run time, bar, share and tokens in their cells", () => {
    const html = fourth("1 hr", context);
    for (const name of [
      "session-runtime",
      "session-context-bar",
      "session-context-fill",
      "session-context-share",
      "session-context-tokens",
    ]) {
      expect(cells(html, name)).toHaveLength(1);
    }
    expect(html).toContain('<span class="session-context-share" aria-hidden="true">22%</span>');
    expect(html).toContain(
      '<span class="session-context-tokens row-fact" aria-hidden="true">215.5k / 1M</span>',
    );
    expect(html).toContain('style="width:22%"');
    expect(html).toContain("running for 1 hr");
  });

  it("keeps the run time alone, in the same place, before the agent answered", () => {
    const html = fourth("3 min", undefined);
    expect(cells(html, "session-runtime")).toHaveLength(1);
    expect(cells(html, "session-context-bar")).toEqual([]);
  });

  it("keeps the run time's cell empty below a minute, so the bar starts in the same place", () => {
    expect(fourth(null, context)).toContain('<span class="session-runtime row-fact"></span>');
  });

  it("warns from a high share", () => {
    const html = fourth("1 hr", { used: 900_000, window: 1_000_000 });
    expect(cells(html, "session-context")).toEqual(["session-context row-fact grows high"]);
    expect(cells(html, "session-context-tokens")).toEqual(["session-context-tokens row-fact high"]);
  });
});

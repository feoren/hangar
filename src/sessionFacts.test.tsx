import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import type { ResourceParts } from "./resources";
import { SessionLoad } from "./SessionLoad";
import { SessionRunCells } from "./SessionRun";

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
  it("puts each value in its own fixed cell, without separators", () => {
    const html = render(<SessionLoad usage={BUSY} gpuColumn />);
    expect(html).not.toContain("·");
    expect(cells(html, "session-value-percent")).toHaveLength(2);
    expect(cells(html, "session-value-memory")).toHaveLength(2);
    expect(html).toContain('<span class="sr-only">CPU 12%</span>');
    expect(html).toContain('<span class="sr-only">GPU 30% 1.2 GB</span>');
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

describe("the session group of the third line", () => {
  const context = { used: 215_500, window: 1_000_000 };

  it("is gone without a run time and context numbers", () => {
    expect(render(<SessionRunCells runtime={null} context={undefined} language="en" />)).toBe("");
  });

  it("shows run time, bar, share and tokens in their cells", () => {
    const html = render(<SessionRunCells runtime="1 hr" context={context} language="en" />);
    expect(html).not.toContain("·");
    for (const name of [
      "session-runtime",
      "session-context-bar",
      "session-context-share",
      "session-context-tokens",
    ]) {
      expect(cells(html, name)).toHaveLength(1);
    }
    expect(html).toContain('<span class="session-context-share" aria-hidden="true">22%</span>');
    expect(html).toContain(
      '<span class="session-context-tokens" aria-hidden="true">215.5k / 1M</span>',
    );
    expect(html).toContain("running for 1 hr");
  });

  it("keeps the run time's cell empty below a minute, so the context columns stay in place", () => {
    const html = render(<SessionRunCells runtime={null} context={context} language="en" />);
    expect(html).toContain('<span class="session-runtime"></span>');
  });

  it("shows the run time alone before the agent answered", () => {
    const html = render(<SessionRunCells runtime="3 min" context={undefined} language="en" />);
    expect(cells(html, "session-runtime")).toHaveLength(1);
    expect(cells(html, "session-context")).toEqual([]);
  });

  it("warns from a high share", () => {
    const html = render(
      <SessionRunCells
        runtime="1 hr"
        context={{ used: 900_000, window: 1_000_000 }}
        language="en"
      />,
    );
    expect(cells(html, "session-context")).toEqual(["session-context high"]);
  });
});

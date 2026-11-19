import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import type { ResourceParts } from "./resources";
import { DEFAULT_ROW_LAYOUT, planRow } from "./rowLayout";
import { SessionRowBody } from "./SessionRow";
import type { SubAgent, SubAgentStatus } from "./subAgents";
import { SubAgentItem } from "./SubAgentTree";
import { DEFAULT_TREE_FLOW, type TreeFlowPrefs } from "./treeFlow";
import { TreeFlowField } from "./TreeFlowSettings";
import { noteInput, noteOutput, sampleNow, setTreeFlow } from "./treeFlowStore";
import { litRows, TreeFlowElbow, TreeFlowRate, TreeFlowTrunk } from "./TreeFlowView";

const ON: TreeFlowPrefs = { enabled: true, style: "stream", showRate: true };

function render(element: ReactElement, language: "de" | "en" = "de"): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={createTranslator(language)}>{element}</I18nContext.Provider>,
  );
}

afterEach(() => {
  // Stops the store's timer and forgets every count.
  setTreeFlow(DEFAULT_TREE_FLOW, true);
});

describe("litRows", () => {
  it("lights only the working sessions and ends the trunk at the last of them", () => {
    const rows = [
      { id: 1, chip: "working" },
      { id: 2, chip: "paused" },
      { id: 3, chip: "working" },
      { id: 4, chip: "asks" },
    ];
    const flow = litRows(
      "test:a",
      rows,
      (row) => row.id,
      (row) => row.chip === "working",
    );
    expect(flow).toEqual({ live: [1, 3], end: 3 });
  });

  it("keeps the end while the last working session stops, then lets go", () => {
    const working = (chips: string[]) =>
      litRows(
        "test:b",
        chips.map((chip, id) => ({ id, chip })),
        (row) => row.id,
        (row) => row.chip === "working",
      );
    expect(working(["paused", "working"]).end).toBe(1);
    expect(working(["paused", "paused"])).toEqual({ live: [], end: 1 });
    expect(working(["paused"])).toEqual({ live: [], end: null });
  });
});

describe("the flow's parts", () => {
  const parts = (
    <>
      <TreeFlowTrunk live={[1]} />
      <TreeFlowElbow sessionId={1} live end />
      <TreeFlowRate sessionId={1} live language="de" />
    </>
  );

  it("render nothing while the setting is off", () => {
    setTreeFlow(DEFAULT_TREE_FLOW, true);
    expect(render(parts)).toBe("");
  });

  it("render nothing where the WebView cannot draw the flow", () => {
    setTreeFlow(ON, false);
    expect(render(parts)).toBe("");
  });

  it("light a working session's way in the chosen style", () => {
    setTreeFlow({ ...ON, style: "pulse" }, true);
    const html = render(parts);
    expect(html).toContain(
      '<li class="tree-flow-line group flow-pulse live" data-flow-level="1" role="presentation" aria-hidden="true">',
    );
    expect(html).toContain('class="tree-flow-elbow flow-pulse live end"');
    expect(html).toContain('class="tree-flow-rate live"');
  });

  it("keep a row's part faded out while the session does not work", () => {
    setTreeFlow(ON, true);
    const html = render(
      <>
        <TreeFlowTrunk live={[]} />
        <TreeFlowElbow sessionId={2} live={false} end={false} />
        <TreeFlowRate sessionId={2} live={false} language="de" />
      </>,
    );
    expect(html).toContain('class="tree-flow-line group flow-stream"');
    expect(html).toContain('class="tree-flow-elbow flow-stream"');
    expect(html).toContain('class="tree-flow-rate"');
    expect(html).not.toContain("live");
  });

  it("leave out the rate when it is turned off", () => {
    setTreeFlow({ ...ON, showRate: false }, true);
    expect(render(<TreeFlowRate sessionId={1} live language="de" />)).toBe("");
  });
});

describe("the measured rate", () => {
  it("counts nothing while the flow is off", () => {
    setTreeFlow(DEFAULT_TREE_FLOW, true);
    noteOutput(5, 50_000);
    setTreeFlow(ON, true);
    sampleNow(1000);
    expect(render(<TreeFlowRate sessionId={5} live language="de" />)).toBe(
      '<span class="tree-flow-rate live" aria-hidden="true"></span>',
    );
  });

  it("shows output and input per second in the row's language and sets the speed", () => {
    setTreeFlow(ON, true);
    noteOutput(5, 100_000);
    noteInput(5, "ä".repeat(100));
    sampleNow(1000);
    const html = render(
      <>
        <TreeFlowRate sessionId={5} live language="de" />
        <TreeFlowElbow sessionId={5} live end={false} />
      </>,
    );
    // Half of the first second, as the smoothing starts from zero; "ä" is two bytes.
    expect(html).toContain('<span class="tree-flow-rate-down">↓ 50 KB/s</span>');
    expect(html).toContain('<span class="tree-flow-rate-up">↑ 0,1 KB/s</span>');
    expect(html).toContain('data-flow-level="3"');
  });

  it("ignores output of a terminal without a session yet", () => {
    setTreeFlow(ON, true);
    noteOutput(null, 100_000);
    sampleNow(1000);
    expect(render(<TreeFlowElbow sessionId={0} live end={false} />)).toContain(
      'data-flow-level="1"',
    );
  });
});

const USAGE: ResourceParts = {
  cpu: "12%",
  memory: "476 MB",
  gpu: null,
  gpuLoad: null,
  gpuMemory: null,
};

function row(rate: ReactElement | null, usage: ResourceParts | null = USAGE): string {
  return render(
    <SessionRowBody
      plan={planRow(DEFAULT_ROW_LAYOUT)}
      name="hangar"
      badge={null}
      program={{ kind: "letters", text: "C", color: "#d97757" }}
      marker={undefined}
      line={{ word: "arbeitet", full: "Claude Code arbeitet" }}
      waiting={false}
      chip="working"
      signs={null}
      place="hangar"
      branch="main"
      team={false}
      usage={usage}
      gpuColumn={false}
      runtime="1 Std."
      context={{ used: 215_500, window: 1_000_000 }}
      language="de"
      rate={rate}
    />,
  );
}

describe("the rate in a session row", () => {
  const cell = <span className="rate-cell" />;

  it("sits at the end of the load's line", () => {
    const html = row(cell);
    // The load's line: its values, then the push to the right edge, then the cell.
    expect(html).toMatch(
      /session-resource-memory(?:(?!row-line)[\s\S])*<span class="row-push"[^>]*><\/span><span class="rate-cell">/,
    );
    expect(html.match(/rate-cell/g)).toHaveLength(1);
  });

  it("takes the last line without the load", () => {
    const html = row(cell, null);
    expect(html).toMatch(
      /session-context-tokens(?:(?!row-line)[\s\S])*<span class="rate-cell"><\/span><\/span><span class="sr-only">/,
    );
  });

  it("adds nothing without a rate", () => {
    expect(row(null)).toBe(row(null));
    expect(row(null)).not.toContain("rate-cell");
  });
});

function agent(status: SubAgentStatus): SubAgent {
  return {
    id: "a1",
    description: "Find the parser",
    agentType: "Explore",
    spawnDepth: 1,
    status,
    startedAt: 0,
    lastActivity: 60_000,
    tool: "Grep",
    target: "parseLayout",
    note: null,
    tokens: 12_300,
    excerpt: null,
  };
}

function subAgent(status: SubAgentStatus): string {
  return render(
    <SubAgentItem
      agent={agent(status)}
      cwd={null}
      now={60_000}
      picked={false}
      onShow={() => undefined}
      onClose={() => undefined}
      flow={{ sessionId: 1, live: status === "running", end: true }}
    />,
  );
}

describe("the sub-agent branch", () => {
  it("lights the elbow of a running sub-agent only", () => {
    setTreeFlow(ON, true);
    expect(subAgent("running")).toContain('class="tree-flow-elbow flow-stream live end"');
    expect(subAgent("finished")).toContain('class="tree-flow-elbow flow-stream end"');
  });

  it("has no flow while the setting is off", () => {
    expect(subAgent("running")).not.toContain("tree-flow");
  });
});

describe("TreeFlowField", () => {
  const field = (value: TreeFlowPrefs, supported = true) =>
    render(<TreeFlowField value={value} supported={supported} onChange={() => undefined} />);
  const t = createTranslator("de");

  it("shows only the switch while the flow is off", () => {
    const html = field(DEFAULT_TREE_FLOW);
    expect(html).toContain(t("treeFlow.enabled"));
    expect(html).not.toContain(t("treeFlow.style"));
    expect(html).not.toContain(t("treeFlow.showRate"));
  });

  it("offers the style and the rate once the flow is on", () => {
    const html = field(ON);
    expect(html).toContain('role="radiogroup"');
    for (const key of [
      "treeFlow.styleStream",
      "treeFlow.stylePulse",
      "treeFlow.styleDots",
    ] as const) {
      expect(html).toContain(t(key));
    }
    expect(html).toMatch(/checked="" value="stream"/);
    expect(html).toContain(t("treeFlow.showRate"));
  });

  it("is left out where the flow cannot be drawn", () => {
    expect(field(ON, false)).toBe("");
  });
});

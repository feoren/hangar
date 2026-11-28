// @vitest-environment happy-dom

// Hangar Drive's logo in the sidebar (ADR 0024): the "+" hands the folder over once per click
// or key and never activates, drags or renames the session row it sits in; the check only says
// that Hangar Drive holds the folder; nothing shows without a mark. Also the logo's places: after
// the folder in the row, at the end of a project group's header, and the setting that picks one.

import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { DriveMarkView, HangarDriveLogo } from "./DriveMark";
import type { DriveMark } from "./hangarDrive";
import { createTranslator, I18nContext } from "./i18n";
import { DEFAULT_ROW_LAYOUT, planRow } from "./rowLayout";
import { SessionRowBody } from "./SessionRow";
import { DrivePlaceField } from "./SettingsDialog";
import { AgentGroupSection } from "./Sidebar";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const OFFER: DriveMark = { kind: "offer" };

let host: HTMLElement;
let root: Root | null = null;
let handed = 0;
let rowEvents: string[] = [];

function show(content: ReactNode) {
  act(() => {
    root?.render(
      <I18nContext.Provider value={createTranslator("en")}>{content}</I18nContext.Provider>,
    );
  });
}

const backUp = () => {
  handed += 1;
};

/** The logo inside a stand-in for the session row's button, which records what reaches it. */
function inRow(mark: DriveMark) {
  return (
    <button
      type="button"
      className="session-main"
      onClick={() => rowEvents.push("click")}
      onPointerDown={() => rowEvents.push("pointerdown")}
      onDoubleClick={() => rowEvents.push("dblclick")}
      onKeyDown={() => rowEvents.push("keydown")}
    >
      <DriveMarkView mark={mark} onBackUp={backUp} inRow className="in-card" />
    </button>
  );
}

function key(target: Element, name: string, repeat = false) {
  act(() => {
    target.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true, repeat }));
  });
}

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  handed = 0;
  rowEvents = [];
});

afterEach(() => {
  act(() => {
    root?.unmount();
  });
  root = null;
  host.remove();
});

describe("DriveMarkView in a session row", () => {
  it("hands the folder over once per click and never by itself", () => {
    show(inRow(OFFER));
    show(inRow({ ...OFFER }));
    expect(handed).toBe(0);

    const logo = host.querySelector('[role="button"].drive-mark.offer');
    expect(logo).not.toBeNull();
    expect(logo?.getAttribute("aria-label")).toBe("Back up with Hangar Drive …");
    expect(logo?.getAttribute("title")).toBe("Back up with Hangar Drive …");
    expect(logo?.getAttribute("tabindex")).toBe("0");
    expect(logo?.querySelector("svg")).not.toBeNull();

    act(() => {
      logo?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(handed).toBe(1);
  });

  it("keeps clicks, presses and double clicks from the row", () => {
    show(inRow(OFFER));
    const logo = host.querySelector(".drive-mark.offer");
    if (logo === null) throw new Error("no logo");
    act(() => {
      logo.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      logo.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      logo.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    });
    expect(rowEvents).toEqual([]);
    expect(handed).toBe(1);
  });

  it("backs up with Enter or Space, once per press, and lets other keys reach the row", () => {
    show(inRow(OFFER));
    const logo = host.querySelector(".drive-mark.offer");
    if (logo === null) throw new Error("no logo");
    key(logo, "Enter");
    key(logo, " ");
    key(logo, "Enter", true);
    expect(handed).toBe(2);
    expect(rowEvents).toEqual([]);
    key(logo, "ArrowDown");
    expect(rowEvents).toEqual(["keydown"]);
  });

  it("shows the check with its words, the state included, and nothing to press", () => {
    show(inRow({ kind: "held", state: "paused" }));
    const mark = host.querySelector(".drive-mark.held");
    expect(mark?.classList.contains("paused")).toBe(true);
    expect(mark?.getAttribute("title")).toBe("In Hangar Drive · paused");
    expect(mark?.textContent).toBe("In Hangar Drive · paused");
    expect(mark?.getAttribute("role")).toBeNull();
    expect(mark?.getAttribute("tabindex")).toBeNull();
    act(() => {
      mark?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(handed).toBe(0);
  });
});

describe("the logo and its badge", () => {
  it("draws Hangar Drive's own logo from the bundled file, in its colors", () => {
    show(<HangarDriveLogo size={16} />);
    const svg = host.querySelector("svg.hangar-drive-logo");
    expect(svg?.getAttribute("width")).toBe("16");
    expect(svg?.getAttribute("aria-hidden")).toBe("true");
    expect(svg?.querySelector("rect")?.getAttribute("fill")).toBe("#141a24");
    const fills = [...(svg?.querySelectorAll("path") ?? [])].map((path) =>
      path.getAttribute("fill"),
    );
    expect(fills).toEqual(["#4fd1a5", "#f2e3c6"]);
    expect(svg?.querySelector("path")?.getAttribute("fill-rule")).toBe("evenodd");
  });

  it("puts a plus on the way to back up, a check once held, a quiet sign while paused", () => {
    const badge = (mark: DriveMark) => {
      show(<DriveMarkView mark={mark} onBackUp={backUp} />);
      expect(host.querySelector(".drive-sign .hangar-drive-logo")).not.toBeNull();
      return host.querySelector(".drive-badge")?.className;
    };
    expect(badge(OFFER)).toBe("drive-badge add");
    expect(badge({ kind: "held", state: "active" })).toBe("drive-badge ok");
    expect(badge({ kind: "held", state: "paused" })).toBe("drive-badge quiet");
    expect(badge({ kind: "held", state: "stopped" })).toBe("drive-badge quiet");
  });
});

describe("DriveMarkView outside the row", () => {
  it("is a real icon button that hands over once per click", () => {
    show(<DriveMarkView mark={OFFER} onBackUp={backUp} className="beside-card" />);
    const button = host.querySelector("button.drive-mark.offer.beside-card");
    expect(button?.getAttribute("aria-label")).toBe("Back up with Hangar Drive …");
    act(() => {
      button?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(handed).toBe(1);
  });

  it("names a synced folder plainly", () => {
    show(<DriveMarkView mark={{ kind: "held", state: "active" }} onBackUp={backUp} />);
    expect(host.querySelector(".drive-mark.held")?.getAttribute("title")).toBe("In Hangar Drive");
    expect(host.querySelector("button")).toBeNull();
  });
});

describe("the logo after the folder in a session row", () => {
  function row(drive: ReactNode, place: string | null = "acme-shop") {
    show(
      <SessionRowBody
        plan={planRow(DEFAULT_ROW_LAYOUT)}
        name="acme-shop"
        badge={null}
        program={{ kind: "letters", text: "C", color: "#d97757" }}
        marker={undefined}
        line={{ word: "working", full: "Claude Code is working" }}
        waiting={false}
        chip="working"
        signs={null}
        place={place}
        branch="main"
        team={false}
        usage={null}
        gpuColumn={false}
        runtime={null}
        context={undefined}
        language="en"
        drive={drive}
      />,
    );
  }

  it("sits right after the folder's name, on the folder's line", () => {
    row(<DriveMarkView mark={OFFER} onBackUp={backUp} inRow className="in-card" />);
    const folder = host.querySelector(".session-path");
    expect(folder?.classList.contains("has-drive")).toBe(true);
    const parts = [...(folder?.children ?? [])].map((child) => child.className);
    expect(parts).toEqual(["session-path-icon", "session-path-name", "drive-mark offer in-card"]);
  });

  it("leaves the folder as it was without a logo, and shows none without a folder", () => {
    row(null);
    expect(host.querySelector(".drive-mark")).toBeNull();
    expect(host.querySelector(".session-path")?.classList.contains("has-drive")).toBe(false);
    row(<DriveMarkView mark={OFFER} onBackUp={backUp} inRow />, null);
    expect(host.querySelector(".drive-mark")).toBeNull();
  });
});

describe("the logo in a project group's header", () => {
  function header(drive: ReactNode) {
    show(
      <ul>
        <AgentGroupSection
          groupKey="project:acme"
          name="acme-shop"
          look={null}
          path="E:\work\acme-shop"
          count={3}
          open
          hasRows
          hiddenWaiting={0}
          holdsActive={false}
          onToggle={() => undefined}
          onMenu={() => undefined}
          drive={drive}
        >
          <li />
        </AgentGroupSection>
      </ul>,
    );
  }

  it("stands at the header's end, beside its toggle, and does not fold the group", () => {
    header(<DriveMarkView mark={OFFER} onBackUp={backUp} className="in-header" />);
    const title = host.querySelector("h2");
    expect(title?.classList.contains("has-drive")).toBe(true);
    const logo = title?.lastElementChild;
    expect(logo?.className).toBe("drive-mark offer icon-button in-header");
    expect(title?.querySelector(".agent-group-toggle")?.contains(logo ?? null)).toBe(false);
    act(() => {
      logo?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(handed).toBe(1);
  });

  it("shows nothing without a logo", () => {
    header(null);
    expect(host.querySelector("h2")?.classList.contains("has-drive")).toBe(false);
    expect(host.querySelector(".drive-mark")).toBeNull();
  });
});

describe("DrivePlaceField", () => {
  it("offers every place and reports the one picked", () => {
    const picked: string[] = [];
    show(
      <DrivePlaceField
        ready
        value="sessionCard"
        onChange={(place) => {
          picked.push(place);
        }}
      />,
    );
    const select = host.querySelector("select");
    if (select === null) throw new Error("no list");
    expect([...select.options].map((option) => option.textContent)).toEqual([
      "In the session row",
      "Beside the session row",
      "At the left edge",
      "In the project header",
      "Only in the session menu",
    ]);
    expect(select.value).toBe("sessionCard");
    act(() => {
      select.value = "leftGutter";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(picked).toEqual(["leftGutter"]);
  });

  it("is not offered while Hangar Drive is not set up", () => {
    show(<DrivePlaceField ready={false} value="sessionCard" onChange={() => undefined} />);
    expect(host.querySelector("select")).toBeNull();
    expect(host.textContent).toBe("");
  });
});

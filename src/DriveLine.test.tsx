// @vitest-environment happy-dom

// The card's "Back up with Hangar Drive" hands the folder over once per click and never on its
// own: not when it appears, not when the card renders again (ADR 0024).

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { DriveView } from "./hangarDrive";
import { createTranslator, I18nContext } from "./i18n";
import { DriveLine } from "./ProjectOverview";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const ready: DriveView = { ready: true, home: null, marks: new Map(), refused: new Set() };

let host: HTMLElement;
let root: Root | null = null;
let handed: string[] = [];

function show(drive: DriveView) {
  act(() => {
    root?.render(
      <I18nContext.Provider value={createTranslator("en")}>
        <DriveLine
          root="E:\work\proj"
          drive={drive}
          onBackUp={(folder) => {
            handed.push(folder);
          }}
        />
      </I18nContext.Provider>,
    );
  });
}

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  handed = [];
});

afterEach(() => {
  act(() => {
    root?.unmount();
  });
  root = null;
  host.remove();
});

describe("DriveLine", () => {
  it("hands the folder over once per click and never by itself", () => {
    show(ready);
    show({ ...ready });
    expect(handed).toEqual([]);

    const button = host.querySelector("button");
    expect(button).not.toBeNull();
    act(() => {
      button?.click();
    });
    expect(handed).toEqual(["E:\\work\\proj"]);
    show({ ...ready });
    expect(handed).toEqual(["E:\\work\\proj"]);
  });

  it("shows no button while Hangar Drive cannot take a folder", () => {
    show({ ...ready, ready: false });
    expect(host.querySelector("button")).toBeNull();
  });
});

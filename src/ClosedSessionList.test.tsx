// @vitest-environment happy-dom

// "Recently closed" in the New session area: nothing while empty, one row per entry with folder
// and title, the whole title and path in the tooltip, and a click reopens that entry.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ClosedSessionList } from "./ClosedSessionList";
import { createTranslator, I18nContext } from "./i18n";
import type { BadgeLook } from "./programPrefs";
import type { ClosedSession } from "./recentlyClosed";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const letters: BadgeLook = { kind: "letters", text: "CC", color: "#4a8fe7" };

const entry: ClosedSession = {
  profileId: "claude",
  cwd: "C:\\work\\shop",
  title: "Claude Code",
  color: null,
  emoji: null,
  cliSessionId: "0f8fad5b-d9cb-469f-a165-70867728950e",
  closedAt: 1,
};

describe("ClosedSessionList", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    host.remove();
  });

  function render(entries: readonly ClosedSession[], onReopen = vi.fn()) {
    act(() => {
      root.render(
        <I18nContext.Provider value={createTranslator("de")}>
          <ClosedSessionList entries={entries} badgeFor={() => letters} onReopen={onReopen} />
        </I18nContext.Provider>,
      );
    });
    return onReopen;
  }

  it("shows nothing while no session was closed", () => {
    render([]);
    expect(host.innerHTML).toBe("");
  });

  it("names folder and title and reopens the clicked entry", () => {
    const onReopen = render([entry]);
    expect(host.querySelector("h3")?.textContent).toBe("Zuletzt geschlossen");
    const button = host.querySelector<HTMLButtonElement>("button.closed-session");
    expect(button?.querySelector(".closed-session-folder")?.textContent).toBe("shop");
    expect(button?.querySelector(".closed-session-title")?.textContent).toBe("Claude Code");
    expect(button?.title).toBe("Claude Code\nC:\\work\\shop");
    expect(button?.getAttribute("aria-label")).toBe("Claude Code in shop wieder öffnen");
    act(() => {
      button?.click();
    });
    expect(onReopen).toHaveBeenCalledWith(entry);
  });
});

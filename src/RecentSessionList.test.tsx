// @vitest-environment happy-dom

// The rows of "Recently opened": nothing while empty, two lines per entry (folder; title or
// program and the age), the title, program and path in the tooltip, a click reopens and the ×
// removes that entry.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import type { BadgeLook } from "./programPrefs";
import { recentDetail, RecentSessionList } from "./RecentSessionList";
import { UNKNOWN_TIME, type RecentSession } from "./recentlyOpened";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const letters: BadgeLook = { kind: "letters", text: "CC", color: "#4a8fe7" };
const NOW = new Date(2026, 10, 9, 12, 0).getTime();

const named: RecentSession = {
  profileId: "claude",
  cwd: "C:\\work\\shop",
  title: "Checkout",
  color: null,
  emoji: null,
  cliSessionId: "0f8fad5b-d9cb-469f-a165-70867728950e",
  closedAt: NOW - 5 * 60_000,
};

const seeded: RecentSession = {
  profileId: "claude",
  cwd: "C:\\work\\blog",
  title: "blog",
  color: null,
  emoji: null,
  cliSessionId: null,
  closedAt: UNKNOWN_TIME,
};

describe("recentDetail", () => {
  it("names the session's own title, else the program", () => {
    expect(recentDetail(named, "Claude Code")).toBe("Checkout");
    expect(recentDetail(seeded, "Claude Code")).toBe("Claude Code");
  });
});

describe("RecentSessionList", () => {
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

  function render(
    entries: readonly RecentSession[],
    handlers: { onReopen?: () => void; onRemove?: (entry: RecentSession) => void } = {},
  ) {
    const onReopen = vi.fn(handlers.onReopen);
    const onRemove = vi.fn(handlers.onRemove);
    act(() => {
      root.render(
        <I18nContext.Provider value={createTranslator("de")}>
          <RecentSessionList
            entries={entries}
            badgeFor={() => letters}
            programName={() => "Claude Code"}
            language="de"
            now={NOW}
            onReopen={onReopen}
            onRemove={onRemove}
          />
        </I18nContext.Provider>,
      );
    });
    return { onReopen, onRemove };
  }

  it("shows nothing while the list is empty", () => {
    render([]);
    expect(host.innerHTML).toBe("");
  });

  it("names folder, title and age and reopens the clicked entry", () => {
    const { onReopen } = render([named]);
    const button = host.querySelector<HTMLButtonElement>("button.recent-open");
    expect(button?.querySelector(".recent-folder")?.textContent).toBe("shop");
    expect(button?.querySelector(".recent-name")?.textContent).toBe("Checkout");
    expect(button?.querySelector(".recent-age")?.textContent).toBe(" · vor 5 Min.");
    expect(button?.title).toBe("Checkout\nClaude Code\nC:\\work\\shop");
    expect(button?.getAttribute("aria-label")).toBe("Checkout in shop öffnen");
    act(() => {
      button?.click();
    });
    expect(onReopen).toHaveBeenCalledWith(named);
  });

  it("names the program and no age for an entry from the recent folders", () => {
    render([seeded]);
    const button = host.querySelector<HTMLButtonElement>("button.recent-open");
    expect(button?.querySelector(".recent-name")?.textContent).toBe("Claude Code");
    expect(button?.querySelector(".recent-age")).toBeNull();
    expect(button?.title).toBe("Claude Code\nC:\\work\\blog");
  });

  it("removes an entry with its × and moves the focus to the row in its place", () => {
    let entries: readonly RecentSession[] = [named, seeded];
    const { onRemove } = render(entries, {
      onRemove: (entry) => {
        entries = entries.filter((other) => other !== entry);
      },
    });
    const remove = host.querySelector<HTMLButtonElement>("button.recent-remove");
    expect(remove?.getAttribute("aria-label")).toBe("shop aus der Liste entfernen");
    act(() => {
      remove?.click();
    });
    expect(onRemove).toHaveBeenCalledWith(named);
    render(entries);
    expect(
      document.activeElement?.closest("li")?.querySelector(".recent-folder")?.textContent,
    ).toBe("blog");
  });
});

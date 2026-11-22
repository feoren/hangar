// @vitest-environment happy-dom

// The sidebar footer's support link: next to Settings, in the wide footer and in the rail, it opens
// the project's Buy Me a Coffee page through the backend's plain web link command.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import { SidebarFooter, SUPPORT_URL } from "./SidebarFooter";

const openWebUrl = vi.hoisted(() => vi.fn<(url: string) => Promise<null>>());

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return { ...actual, api: { ...actual.api, openWebUrl } };
});

vi.mock("./useAccount", () => ({
  useAccount: () => ({ view: null, projects: null }),
  useAccountAvatar: () => null,
}));

vi.mock("./useTeam", () => ({
  useTeam: () => ({ pending: [], activities: [], statuses: new Map() }),
  loadTeamStatuses: () => undefined,
}));

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");
let root: Root;
let host: HTMLDivElement;

beforeEach(() => {
  openWebUrl.mockResolvedValue(null);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => {
    root.unmount();
  });
  host.remove();
  vi.clearAllMocks();
});

function render(collapsed: boolean) {
  act(() => {
    root.render(
      <I18nContext.Provider value={t}>
        <SidebarFooter
          collapsed={collapsed}
          teamOpen={false}
          onToggleTeam={() => undefined}
          onOpenAccount={() => undefined}
          onOpenSettings={() => undefined}
          theme="dark"
          onToggleTheme={() => undefined}
        />
      </I18nContext.Provider>,
    );
  });
}

describe("SidebarFooter support link", () => {
  it.each([false, true])(
    "sits right before Settings and opens the page (collapsed: %s)",
    (collapsed) => {
      render(collapsed);
      const coffee = host.querySelector<HTMLButtonElement>(".footer-coffee");
      expect(coffee?.getAttribute("aria-label")).toBe("Buy me a coffee");
      expect(coffee?.nextElementSibling?.classList.contains("footer-settings")).toBe(true);

      act(() => {
        coffee?.click();
      });
      expect(openWebUrl).toHaveBeenCalledWith(SUPPORT_URL);
    },
  );

  it("points to the project's page over https", () => {
    expect(SUPPORT_URL).toBe("https://buymeacoffee.com/feoren");
  });
});

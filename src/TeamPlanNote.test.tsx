// @vitest-environment happy-dom

// The note that stands in for team functions on Free (teamPlan.ts): one sentence about the Team
// plan, "See plans" opening the pricing page through the backend, and, signed out, a sign-in.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import { PRICING_URL } from "./teamPlan";
import { TeamPlanNote } from "./TeamPlanNote";

const openWebUrl = vi.hoisted(() => vi.fn<(url: string) => Promise<null>>());

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return { ...actual, api: { ...actual.api, openWebUrl } };
});

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

function render(onSignIn?: () => void) {
  act(() => {
    root.render(
      <I18nContext.Provider value={t}>
        <TeamPlanNote onSignIn={onSignIn} />
      </I18nContext.Provider>,
    );
  });
}

const button = (text: string) =>
  [...host.querySelectorAll("button")].find((element) => element.textContent === text);

describe("TeamPlanNote", () => {
  it("explains the Team plan and opens the pricing page", async () => {
    render();
    expect(host.textContent).toContain(t("teamPlan.text"));
    expect(host.textContent).not.toContain(t("teamPlan.signedOut"));
    expect(button(t("account.signIn"))).toBeUndefined();
    await act(async () => {
      button(t("teamPlan.seePlans"))?.click();
      await Promise.resolve();
    });
    expect(openWebUrl).toHaveBeenCalledWith(PRICING_URL);
    expect(host.querySelector('[role="alert"]')).toBeNull();
  });

  it("offers the sign-in when signed out: the person may belong to a paid team", () => {
    const onSignIn = vi.fn();
    render(onSignIn);
    expect(host.textContent).toContain(t("teamPlan.signedOut"));
    act(() => {
      button(t("account.signIn"))?.click();
    });
    expect(onSignIn).toHaveBeenCalledOnce();
  });

  it("says so when the browser could not be opened", async () => {
    openWebUrl.mockRejectedValue(new Error("not focused"));
    render();
    await act(async () => {
      button(t("teamPlan.seePlans"))?.click();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(host.querySelector('[role="alert"]')?.textContent).toBe(t("account.error.open"));
  });

  it("uses no greyed-out controls", () => {
    render(vi.fn());
    expect(host.querySelectorAll("button:disabled").length).toBe(0);
  });
});

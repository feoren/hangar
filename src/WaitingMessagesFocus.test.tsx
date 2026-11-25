// @vitest-environment happy-dom

// Where the keyboard focus goes when a card's buttons swap: to the head that folded the card once
// a send starts (Enter opens it, "Cancel" is inside), back
// to "Send" when it ended and the card stays. Without it, the focused button vanished and the
// focus fell to the page.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import type { CardSend } from "./messageCard";
import type { WaitingMessage } from "./partnerInbox";
import { ARM_DELAY_MS } from "./useArmDelay";
import { WaitingMessages } from "./WaitingMessages";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");
const message: WaitingMessage = { id: 1, from: "app", to: "web", text: "hello", at: 1 };

let host: HTMLElement;
let root: Root | null = null;
let sends: ReadonlyMap<number, CardSend> = new Map();

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date", "performance"] });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  sends = new Map();
});

afterEach(() => {
  act(() => {
    root?.unmount();
  });
  root = null;
  host.remove();
  vi.useRealTimers();
});

function show() {
  act(() => {
    root?.render(
      <I18nContext.Provider value={t}>
        <WaitingMessages
          messages={[message]}
          sends={sends}
          generation={1}
          language="en"
          onSend={(id, submit) => {
            sends = new Map([[id, { phase: "waiting", submit, target: "web" }]]);
          }}
          onCancel={() => undefined}
          onDiscard={() => undefined}
        />
      </I18nContext.Provider>,
    );
  });
}

function button(text: string): HTMLButtonElement | undefined {
  return [...host.querySelectorAll("button")].find((entry) => entry.textContent === text);
}

describe("WaitingMessages focus", () => {
  it("moves to the folded head when a send starts and back to Send when it ends", () => {
    show();
    act(() => {
      vi.advanceTimersByTime(ARM_DELAY_MS);
    });
    const send = button("Send");
    act(() => {
      send?.focus();
      send?.click();
    });
    show();
    const head = host.querySelector<HTMLButtonElement>(".waiting-message-fold");
    expect(document.activeElement).toBe(head);
    expect(head?.getAttribute("aria-expanded")).toBe("false");
    // The status line was there before and now speaks.
    expect(host.querySelector('[role="status"]')?.textContent).toBe(
      "Sends when the session is ready",
    );
    // Cancelled or failed: the card stays, and the focus is on Send again.
    sends = new Map();
    show();
    expect(document.activeElement).toBe(button("Send"));
    expect(host.querySelector('[role="status"]')?.textContent).toBe("");
  });

  it("leaves the focus where the person moved it", () => {
    const outside = document.createElement("button");
    document.body.append(outside);
    show();
    act(() => {
      vi.advanceTimersByTime(ARM_DELAY_MS);
    });
    act(() => {
      button("Send")?.focus();
      outside.focus();
    });
    sends = new Map([[1, { phase: "waiting", submit: true, target: "web" }]]);
    show();
    expect(document.activeElement).toBe(outside);
    outside.remove();
  });
});

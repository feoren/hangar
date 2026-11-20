// @vitest-environment happy-dom

// The pause before a waiting message's buttons take clicks: a new message arms every card anew,
// since it can move the card under the mouse.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import type { WaitingMessage } from "./partnerInbox";
import { ARM_DELAY_MS } from "./useArmDelay";
import { WaitingMessages } from "./WaitingMessages";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");
const first: WaitingMessage = { id: 1, from: "app", to: "web", text: "first", at: 1 };
const second: WaitingMessage = { id: 2, from: "app", to: "web", text: "second", at: 2 };

let host: HTMLElement;
let root: Root | null = null;
const pasted: number[] = [];

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date", "performance"] });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  pasted.length = 0;
});

afterEach(() => {
  act(() => {
    root?.unmount();
  });
  root = null;
  host.remove();
  vi.useRealTimers();
});

function show(messages: WaitingMessage[], generation: number) {
  act(() => {
    root?.render(
      <I18nContext.Provider value={t}>
        <WaitingMessages
          messages={messages}
          sends={new Map()}
          generation={generation}
          language="en"
          onSend={(id) => {
            pasted.push(id);
          }}
          onCancel={() => undefined}
          onDiscard={() => undefined}
        />
      </I18nContext.Provider>,
    );
  });
}

/** Clicks "Send" on the card of message `id`. */
function paste(id: number) {
  const card = host.querySelectorAll(".waiting-message")[id - 1];
  const button = card?.querySelector<HTMLButtonElement>(".waiting-message-send");
  act(() => {
    button?.click();
  });
}

function wait(ms: number) {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
}

describe("WaitingMessages arm delay", () => {
  it("takes a click only after the pause", () => {
    show([first], 1);
    paste(1);
    expect(pasted).toEqual([]);
    wait(ARM_DELAY_MS);
    paste(1);
    expect(pasted).toEqual([1]);
  });

  it("arms the first card anew when a second message arrives", () => {
    show([first], 1);
    wait(ARM_DELAY_MS);
    show([first, second], 2);
    // The first card moved or a new one appeared: its click is ignored for the pause.
    paste(1);
    expect(pasted).toEqual([]);
    wait(ARM_DELAY_MS - 1);
    paste(1);
    expect(pasted).toEqual([]);
    wait(1);
    paste(1);
    paste(2);
    expect(pasted).toEqual([1, 2]);
  });
});

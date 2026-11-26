// @vitest-environment happy-dom

// The message stack below a row (ADR 0014): two or more link messages in one line with "Send
// all" and "Discard all", which arm like a card's buttons, and the single cards on request.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator, I18nContext, type Language } from "./i18n";
import type { CardSend } from "./messageCard";
import type { WaitingMessage } from "./partnerInbox";
import { ARM_DELAY_MS } from "./useArmDelay";
import { WaitingMessages } from "./WaitingMessages";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const first: WaitingMessage = { id: 1, from: "web", to: "1", text: "first", at: 1 };
const second: WaitingMessage = { id: 2, from: "docs", to: "1", text: "second", at: 2 };
const third: WaitingMessage = { id: 3, from: "web", to: "1", text: "third", at: 3 };

let host: HTMLElement;
let root: Root | null = null;
const calls: string[] = [];

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date", "performance"] });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  calls.length = 0;
});

afterEach(() => {
  act(() => {
    root?.unmount();
  });
  root = null;
  host.remove();
  vi.useRealTimers();
});

function show(
  messages: WaitingMessage[],
  options: {
    generation?: number;
    stack?: boolean;
    pasteOnly?: boolean;
    sends?: ReadonlyMap<number, CardSend>;
    language?: Language;
  } = {},
) {
  act(() => {
    root?.render(
      <I18nContext.Provider value={createTranslator(options.language ?? "en")}>
        <WaitingMessages
          messages={messages}
          sends={options.sends ?? new Map()}
          generation={options.generation ?? 1}
          language="en"
          session={1}
          {...(options.pasteOnly === undefined ? {} : { pasteOnly: options.pasteOnly })}
          {...(options.stack === false
            ? {}
            : {
                stack: {
                  onSendAll: (ids) => calls.push(`send ${ids.join(",")}`),
                  onDiscardAll: (ids) => calls.push(`discard ${ids.join(",")}`),
                },
              })}
          onSend={(id, submit) => calls.push(`one ${String(id)} ${String(submit)}`)}
          onCancel={(id) => calls.push(`cancel ${String(id)}`)}
          onDiscard={(id) => calls.push(`drop ${String(id)}`)}
        />
      </I18nContext.Provider>,
    );
  });
}

function button(label: string): HTMLButtonElement | undefined {
  return [...host.querySelectorAll("button")].find((entry) => entry.textContent === label);
}

function click(label: string) {
  act(() => {
    button(label)?.click();
  });
}

function wait(ms: number) {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
}

describe("the message stack", () => {
  it("sums up two or more messages in one line, naming who sent them", () => {
    show([first, second, third]);
    const stack = host.querySelector(".waiting-stack");
    expect(stack?.getAttribute("aria-label")).toBe("3 messages from the group");
    expect(stack?.textContent).toContain("From web, docs");
    expect(button("Send all")?.className).toBe("primary-button");
    expect(button("Discard all")).toBeDefined();
    // The single cards only on request.
    expect(host.querySelectorAll(".waiting-message").length).toBe(1);
    expect(host.textContent).not.toContain("first");
  });

  it("opens to the single cards and closes again", () => {
    show([first, second]);
    const toggle = button("Show each");
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    click("Show each");
    expect(button("Hide each")?.getAttribute("aria-expanded")).toBe("true");
    const controls = button("Hide each")?.getAttribute("aria-controls") ?? "";
    expect(host.querySelector(`[id="${controls}"]`)?.textContent).toContain("first");
    expect(host.querySelectorAll(".waiting-message").length).toBe(3);
    click("Hide each");
    expect(host.querySelectorAll(".waiting-message").length).toBe(1);
  });

  it("takes Send all and Discard all only after the pause, and arms anew on a change", () => {
    show([first, second]);
    click("Send all");
    click("Discard all");
    expect(calls).toEqual([]);
    wait(ARM_DELAY_MS);
    click("Send all");
    expect(calls).toEqual(["send 1,2"]);
    // A new message arrives: the stack's buttons wait again.
    show([first, second, third], { generation: 2 });
    click("Discard all");
    expect(calls).toEqual(["send 1,2"]);
    wait(ARM_DELAY_MS);
    click("Discard all");
    expect(calls).toEqual(["send 1,2", "discard 1,2,3"]);
  });

  it("shows a secret hint of any message in the stack line", () => {
    show([first, { ...second, secrets: ["githubToken"] }, { ...third, secrets: ["privateKey"] }]);
    const hint = host.querySelector(".waiting-stack .waiting-message-secret");
    expect(hint?.textContent).toContain(
      "Probably contains a secret key (GitHub token, private key).",
    );
    expect(hint?.getAttribute("role")).toBe("note");
  });

  it("says that a send of the stack waits, with Cancel instead of its buttons", () => {
    const sends = new Map([[2, { phase: "waiting", submit: true, target: "app", to: 1 } as const]]);
    show([first, second], { sends });
    const status = host.querySelector('.waiting-stack [role="status"]');
    expect(status?.textContent).toBe("Sends when the session is ready");
    expect(button("Send all")).toBeUndefined();
    click("Cancel");
    expect(calls).toEqual(["cancel 2"]);
  });

  it("announces a send once, in the stack's line, even with the single cards open", () => {
    const sends = new Map([[2, { phase: "waiting", submit: true, target: "app", to: 1 } as const]]);
    show([first, second], { sends });
    click("Show each");
    expect(host.querySelectorAll('[role="status"]').length).toBe(1);
    expect(host.querySelector('.waiting-stack [role="status"]')).not.toBeNull();
  });

  it("keeps the keyboard focus in the line while Send all runs and after it", () => {
    show([first, second]);
    wait(ARM_DELAY_MS);
    act(() => {
      button("Send all")?.focus();
    });
    const waiting = new Map([
      [1, { phase: "waiting", submit: true, target: "app", to: 1 } as const],
    ]);
    show([first, second], { sends: waiting });
    expect(document.activeElement?.textContent).toBe("Cancel");
    const sending = new Map([
      [1, { phase: "sending", submit: true, target: "app", to: 1 } as const],
    ]);
    show([first, second], { sends: sending });
    expect(document.activeElement?.textContent).toBe("Show each");
    show([first, second]);
    expect(document.activeElement?.textContent).toBe("Send all");
  });

  it("stays a single card below two, and without a stack for partner messages", () => {
    show([first]);
    expect(host.querySelector(".waiting-stack")).toBeNull();
    show([first, second], { stack: false });
    expect(host.querySelector(".waiting-stack")).toBeNull();
    expect(host.querySelectorAll(".waiting-message").length).toBe(2);
  });

  it("reads in German", () => {
    show([first, second], { language: "de" });
    expect(host.querySelector(".waiting-stack")?.getAttribute("aria-label")).toBe(
      "2 Nachrichten aus der Gruppe",
    );
    expect(button("Alle senden")).toBeDefined();
    expect(button("Alle verwerfen")).toBeDefined();
    expect(button("Einzeln anzeigen")).toBeDefined();
  });
});

describe("Paste only", () => {
  it("is offered for partner messages, not for link messages", () => {
    show([first], { stack: false });
    expect(button("Paste only")).toBeDefined();
    show([first], { pasteOnly: false });
    expect(button("Paste only")).toBeUndefined();
    expect(button("Send")).toBeDefined();
  });
});

// @vitest-environment happy-dom

// Component test for "Send to partner": which target it shows, when sending is possible and
// what each button hands over. Where the message goes is decided in partnerMessage.ts.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import { MESSAGE_MAX_CHARS } from "./partnerMessage";
import { PartnerMessageDialog, type PartnerSend, type TargetView } from "./PartnerMessageDialog";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("de");
const PARTNERS = [
  { name: "web", folder: "C:\\work\\web" },
  { name: "relay", folder: "C:\\work\\relay" },
];

let host: HTMLElement;
let root: Root | null = null;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
});

afterEach(() => {
  act(() => {
    root?.unmount();
  });
  root = null;
  host.remove();
});

async function mount(
  resolve: (partner: string) => Promise<TargetView>,
  onSend: (send: PartnerSend) => void,
  partners = PARTNERS,
) {
  root = createRoot(host);
  await act(async () => {
    root?.render(
      <I18nContext.Provider value={t}>
        <PartnerMessageDialog
          title="Fix login"
          partners={partners}
          resolve={resolve}
          onSend={onSend}
          onClose={() => undefined}
        />
      </I18nContext.Provider>,
    );
    await Promise.resolve();
  });
}

function type(text: string) {
  const area = host.querySelector("textarea");
  if (area === null) throw new Error("no text field");
  act(() => {
    // Through the prototype's setter, so React's value tracker sees a change.
    Reflect.set(HTMLTextAreaElement.prototype, "value", text, area);
    area.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

function button(label: string): HTMLButtonElement {
  const found = Array.from(host.querySelectorAll("button")).find(
    (element) => element.textContent === label,
  );
  if (found === undefined) throw new Error(`no button ${label}`);
  return found;
}

describe("PartnerMessageDialog", () => {
  it("pastes into the one running session, with or without Enter", async () => {
    const onSend = vi.fn();
    await mount(() => Promise.resolve({ kind: "one", id: 7, title: "Codex web" }), onSend);
    expect(host.textContent).toContain(t("partner.toOne", { session: "Codex web" }));
    expect(button(t("partner.pasteSend")).disabled).toBe(true);
    type("Show the new server text");
    act(() => {
      button(t("partner.paste")).click();
    });
    act(() => {
      button(t("partner.pasteSend")).click();
    });
    expect(onSend.mock.calls).toEqual([
      [
        {
          partner: "web",
          text: "Show the new server text",
          submit: false,
          target: { kind: "session", id: 7 },
        },
      ],
      [
        {
          partner: "web",
          text: "Show the new server text",
          submit: true,
          target: { kind: "session", id: 7 },
        },
      ],
    ]);
  });

  it("asks which session when several run and sends only after the choice", async () => {
    const onSend = vi.fn();
    await mount(
      () =>
        Promise.resolve({
          kind: "choose",
          sessions: [
            { id: 3, title: "Claude web" },
            { id: 4, title: "Codex web" },
          ],
        }),
      onSend,
    );
    type("x");
    expect(button(t("partner.paste")).disabled).toBe(true);
    const radios = host.querySelectorAll<HTMLInputElement>('input[type="radio"]');
    expect(radios).toHaveLength(2);
    act(() => {
      radios[1]?.click();
    });
    act(() => {
      button(t("partner.paste")).click();
    });
    expect(onSend).toHaveBeenCalledWith({
      partner: "web",
      text: "x",
      submit: false,
      target: { kind: "session", id: 4 },
    });
  });

  it("offers to start the last program and asks again for another partner", async () => {
    const resolve = vi.fn((partner: string) =>
      Promise.resolve<TargetView>(
        partner === "web"
          ? { kind: "start", profileId: "claude", program: "Claude Code", folder: "C:\\work\\web" }
          : { kind: "none" },
      ),
    );
    const onSend = vi.fn();
    await mount(resolve, onSend);
    expect(host.textContent).toContain(
      t("partner.start", { program: "Claude Code", folder: "C:\\work\\web" }),
    );
    type("hi");
    act(() => {
      button(t("partner.pasteSend")).click();
    });
    expect(onSend).toHaveBeenLastCalledWith({
      partner: "web",
      text: "hi",
      submit: true,
      target: { kind: "start", profileId: "claude" },
    });
    const select = host.querySelector("select");
    if (select === null) throw new Error("no partner choice");
    await act(async () => {
      select.value = "relay";
      select.dispatchEvent(new Event("change", { bubbles: true }));
      await Promise.resolve();
    });
    expect(resolve).toHaveBeenLastCalledWith("relay");
    expect(host.textContent).toContain(t("partner.noProgram"));
    expect(button(t("partner.pasteSend")).disabled).toBe(true);
  });

  it("names the only partner and refuses a text over the limit", async () => {
    await mount(
      () => Promise.resolve({ kind: "one", id: 1, title: "web" }),
      () => undefined,
      [PARTNERS[0] ?? { name: "web", folder: "/w" }],
    );
    expect(host.querySelector("select")).toBeNull();
    type("x".repeat(MESSAGE_MAX_CHARS + 1));
    expect(host.textContent).toContain(t("partner.tooLong", { max: MESSAGE_MAX_CHARS }));
    expect(button(t("partner.paste")).disabled).toBe(true);
  });
});

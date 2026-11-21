// @vitest-environment happy-dom

// Dictation: one switch; the key, the language and the speech models only while it is on, and the
// status of a model at its own card.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { DictationStatus } from "./dictation";
import { DictationSettings } from "./DictationSettings";
import { createTranslator, I18nContext } from "./i18n";
import { defaultSettings, type Settings } from "./settings";
import { dictationKeyName } from "./shortcuts";

const mocks = vi.hoisted(() => {
  const status: DictationStatus = {
    state: { kind: "notInstalled" },
    models: [
      { id: "base", bytes: 1000, installed: false },
      { id: "small", bytes: 2000, installed: false },
    ],
    source: "example.org",
    microphone: true,
  };
  return {
    status,
    install: vi.fn(() => Promise.resolve()),
    remove: vi.fn(() => Promise.resolve()),
  };
});

vi.mock("./useDictation", () => ({ useDictationStatus: () => mocks.status }));
vi.mock("./api", () => ({
  api: { dictationInstallModel: mocks.install, dictationRemoveModel: mocks.remove },
  errorText: (error: unknown) => (error instanceof Error ? error.message : String(error)),
}));

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");
let root: Root;
let host: HTMLDivElement;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  mocks.install.mockClear();
});

afterEach(() => {
  act(() => {
    root.unmount();
  });
  host.remove();
});

function render(
  changes: Partial<Settings> = {},
  onChange: (patch: Partial<Settings>) => void = () => undefined,
) {
  act(() => {
    root.render(
      <I18nContext.Provider value={t}>
        <DictationSettings
          settings={{ ...defaultSettings("en"), ...changes }}
          onChange={onChange}
        />
      </I18nContext.Provider>,
    );
  });
}

const dictationSwitch = () => host.querySelector<HTMLInputElement>('input[role="switch"]');
const cards = () => [...host.querySelectorAll<HTMLElement>(".dictation-model")];
const statusOf = (card: HTMLElement | undefined) =>
  card?.querySelector(".dictation-model-status")?.textContent ?? null;

describe("DictationSettings", () => {
  it("shows one switch named Dictation, with the key in its help, and nothing else while off", () => {
    render({ dictationEnabled: false });
    const toggle = dictationSwitch();
    expect(host.querySelectorAll('input[role="switch"]')).toHaveLength(1);
    expect(toggle?.checked).toBe(false);
    const label = document.getElementById(toggle?.getAttribute("aria-labelledby") ?? "");
    expect(label?.textContent).toBe(t("dictation.enabled"));
    const help = document.getElementById(toggle?.getAttribute("aria-describedby") ?? "");
    const settings = defaultSettings("en");
    expect(help?.textContent).toBe(
      t("dictation.enabledHelp", { shortcut: dictationKeyName(settings.dictation.key, t, true) }),
    );
    expect(host.querySelector("#dictation-key")).toBeNull();
    expect(host.querySelector("#dictation-language")).toBeNull();
    expect(cards()).toHaveLength(0);
    expect(host.querySelectorAll("h4")).toHaveLength(0);
  });

  it("turns dictation on through the same setting", () => {
    const onChange = vi.fn();
    render({ dictationEnabled: false }, onChange);
    act(() => {
      dictationSwitch()?.click();
    });
    expect(onChange).toHaveBeenCalledWith({ dictationEnabled: true });
  });

  it("shows key, language and the model cards while on, each with a visible name", () => {
    render({ dictationEnabled: true });
    for (const [id, key] of [
      ["dictation-key", "dictation.key"],
      ["dictation-language", "dictation.language"],
    ] as const) {
      const select = host.querySelector(`#${id}`);
      const name = document.getElementById(select?.getAttribute("aria-labelledby") ?? "");
      expect(name?.textContent).toBe(t(key));
      expect(name?.classList.contains("sr-only")).toBe(false);
    }
    expect(cards()).toHaveLength(2);
    expect(host.querySelector(".setting-group-title")?.textContent).toBe(t("dictation.model"));
  });

  it("keeps the key and the language wired to the dictation preferences", () => {
    const onChange = vi.fn();
    render({ dictationEnabled: true }, onChange);
    const key = host.querySelector<HTMLSelectElement>("#dictation-key");
    act(() => {
      if (key) key.value = "f9";
      key?.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(onChange).toHaveBeenLastCalledWith({
      dictation: { ...defaultSettings("en").dictation, key: "f9" },
    });
    const language = host.querySelector<HTMLSelectElement>("#dictation-language");
    act(() => {
      if (language) language.value = "de";
      language?.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(onChange).toHaveBeenLastCalledWith({
      dictation: { ...defaultSettings("en").dictation, language: "de" },
    });
  });

  it("shows the state at the chosen model's card only", () => {
    render({ dictationEnabled: true });
    const [base, small] = cards();
    expect(base?.dataset.selected).toBe("true");
    expect(statusOf(base)).toBe(t("dictation.stateNotInstalled"));
    expect(statusOf(small)).toBe("");
    expect(host.querySelectorAll('[role="status"]')).toHaveLength(2);
  });

  it("asks at the card before a download and installs only after yes", () => {
    render({ dictationEnabled: true });
    const small = cards()[1];
    const download = [...(small?.querySelectorAll("button") ?? [])].find(
      (button) => button.textContent === t("dictation.download"),
    );
    act(() => {
      download?.click();
    });
    const question = small?.querySelector(".confirm-box");
    expect(question?.textContent).toContain("example.org");
    expect(mocks.install).not.toHaveBeenCalled();
    const yes = [...(question?.querySelectorAll("button") ?? [])].find(
      (button) => button.textContent === t("dictation.consentYes"),
    );
    act(() => {
      yes?.click();
    });
    expect(mocks.install).toHaveBeenCalledWith("small");
    // The button gives way to the progress, so the keyboard stays at the model's choice.
    expect(document.activeElement).toBe(small?.querySelector('input[type="radio"]'));
  });

  it("gives the focus back to the download button when the question is cancelled", () => {
    render({ dictationEnabled: true });
    const base = cards()[0];
    const download = () =>
      [...(base?.querySelectorAll("button") ?? [])].find(
        (button) => button.textContent === t("dictation.download"),
      );
    act(() => {
      download()?.click();
    });
    const cancel = [
      ...(base?.querySelectorAll<HTMLButtonElement>(".confirm-box button") ?? []),
    ].find((button) => button.textContent === t("common.cancel"));
    expect(document.activeElement).toBe(cancel);
    act(() => {
      cancel?.click();
    });
    expect(base?.querySelector(".confirm-box")).toBeNull();
    expect(document.activeElement).toBe(download());
  });

  it("shows a failed download at the card of that model, not at the chosen one", async () => {
    mocks.install.mockImplementationOnce(() => Promise.reject(new Error("download")));
    render({ dictationEnabled: true });
    const small = cards()[1];
    const download = [...(small?.querySelectorAll("button") ?? [])].find(
      (button) => button.textContent === t("dictation.download"),
    );
    act(() => {
      download?.click();
    });
    const yes = [...(small?.querySelectorAll<HTMLButtonElement>(".confirm-box button") ?? [])].find(
      (button) => button.textContent === t("dictation.consentYes"),
    );
    await act(async () => {
      yes?.click();
      await Promise.resolve();
    });
    const [base, after] = cards();
    expect(statusOf(after)).toBe(t("dictation.downloadFailed"));
    expect(after?.querySelector(".dictation-model-status")?.classList.contains("error-text")).toBe(
      true,
    );
    expect(statusOf(base)).toBe(t("dictation.stateNotInstalled"));
  });

  it("offers no download without a microphone and says so at the chosen card", () => {
    mocks.status.microphone = false;
    try {
      render({ dictationEnabled: true });
      const [base, small] = cards();
      const labels = [...host.querySelectorAll("button")].map((button) => button.textContent);
      expect(labels).not.toContain(t("dictation.download"));
      expect(statusOf(base)).toBe(t("dictation.noMicrophoneSetup"));
      expect(statusOf(small)).toBe("");
    } finally {
      mocks.status.microphone = true;
    }
  });
});

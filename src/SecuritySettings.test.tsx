// @vitest-environment happy-dom

// Security: each switch that builds on another shows only while that one is on, nothing is
// greyed out, and the rarely needed rules file waits under "Advanced" unless it has a problem.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import { SecuritySettings } from "./SecuritySettings";
import { defaultSettings, type Settings } from "./settings";

const backend = vi.hoisted(() => {
  const state: { guard: unknown; rules: unknown[] } = { guard: null, rules: [] };
  return state;
});

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => undefined)) }));
vi.mock("./api", () => ({
  GATE_CHANGED: "gate-changed",
  api: {
    commandGuardInfo: vi.fn(() => Promise.resolve(backend.guard)),
    gateRules: vi.fn(() => Promise.resolve(backend.rules)),
    gateSnapshot: vi.fn(() => Promise.resolve(null)),
    deleteGateRule: vi.fn(() => Promise.resolve()),
  },
}));

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");
const GUARD_OK = {
  enabled: true,
  file: "C:\\Users\\demo\\command-guard.toml",
  defaults: false,
  defaultCount: 0,
  rules: [],
  problems: [],
};

let root: Root;
let host: HTMLDivElement;

beforeEach(() => {
  backend.guard = GUARD_OK;
  backend.rules = [];
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

async function render(
  changes: Partial<Settings> = {},
  onChange: (patch: Partial<Settings>) => void = () => undefined,
) {
  await act(async () => {
    root.render(
      <I18nContext.Provider value={t}>
        <SecuritySettings settings={{ ...defaultSettings("en"), ...changes }} onChange={onChange} />
      </I18nContext.Provider>,
    );
    await Promise.resolve();
  });
}

function switchFor(label: string): HTMLInputElement | null {
  const name = [...host.querySelectorAll(".setting-label")].find(
    (element) => element.textContent === label,
  );
  if (name === undefined) return null;
  return host.querySelector<HTMLInputElement>(`input[role="switch"][aria-labelledby="${name.id}"]`);
}

describe("SecuritySettings", () => {
  it("shows the gate only with the overlap warning, the command guard only with the gate", async () => {
    await render({ claimsEnabled: false, gateEnabled: true, commandGuardEnabled: true });
    expect(switchFor(t("settings.warnMultilinePaste"))).not.toBeNull();
    expect(switchFor(t("settings.claims"))?.checked).toBe(false);
    expect(switchFor(t("settings.gate"))).toBeNull();
    expect(switchFor(t("settings.commandGuard"))).toBeNull();

    await render({ claimsEnabled: true, gateEnabled: false, commandGuardEnabled: true });
    expect(switchFor(t("settings.gate"))?.checked).toBe(false);
    expect(switchFor(t("settings.commandGuard"))).toBeNull();

    await render({ claimsEnabled: true, gateEnabled: true, commandGuardEnabled: true });
    expect(switchFor(t("settings.commandGuard"))?.checked).toBe(true);
  });

  it("greys nothing out", async () => {
    await render({ claimsEnabled: true, gateEnabled: true });
    expect(host.querySelectorAll("input:disabled")).toHaveLength(0);
  });

  it("saves a switch at once", async () => {
    const onChange = vi.fn();
    await render({ claimsEnabled: true, gateEnabled: false }, onChange);
    act(() => {
      switchFor(t("settings.gate"))?.click();
    });
    expect(onChange).toHaveBeenCalledWith({ gateEnabled: true });
  });

  it("keeps the rules file under a closed Advanced while it reads fine", async () => {
    await render({ claimsEnabled: true, gateEnabled: true, commandGuardEnabled: true });
    const advanced = host.querySelector("details");
    expect(advanced?.open).toBe(false);
    expect(advanced?.textContent).toContain("command-guard.toml");
  });

  it("opens Advanced when the rules file has a problem", async () => {
    backend.guard = {
      ...GUARD_OK,
      problems: [{ rule: null, line: 3, id: null, message: "expected a value" }],
    };
    await render({ claimsEnabled: true, gateEnabled: true, commandGuardEnabled: true });
    const advanced = host.querySelector("details");
    expect(advanced?.open).toBe(true);
    expect(advanced?.textContent).toContain("expected a value");
  });

  it("shows no rules file without the command guard", async () => {
    await render({ claimsEnabled: true, gateEnabled: true, commandGuardEnabled: false });
    expect(host.querySelector("details")).toBeNull();
  });

  it("lists remembered answers as soon as there is one", async () => {
    await render();
    expect(host.textContent).not.toContain(t("settings.gateRules"));
    backend.rules = [
      {
        id: 1,
        createdAt: 1,
        projectName: "demo",
        path: "src/main.ts",
        scope: "file",
        effect: "allow",
      },
    ];
    act(() => {
      root.unmount();
    });
    root = createRoot(host);
    await render();
    expect(host.querySelector(".setting-group-title")?.textContent).toBe(t("settings.gateRules"));
    expect(host.textContent).toContain("src/main.ts");
  });
});

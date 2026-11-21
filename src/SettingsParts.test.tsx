// @vitest-environment happy-dom

// The settings parts wire names, help and status to their controls, so every section that uses
// them is read out the same way, and dependent settings only show while their switch is on.

import { act, useState, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  ActionRow,
  Advanced,
  InlineConfirm,
  SegmentedField,
  SettingsGroup,
  SwitchRow,
} from "./SettingsParts";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

let root: Root;
let host: HTMLDivElement;

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

const render = (node: ReactNode) => {
  act(() => {
    root.render(node);
  });
};

const byId = (id: string | null) => (id === null ? null : document.getElementById(id));

function ToggleProbe({ initial }: { initial: boolean }) {
  const [on, setOn] = useState(initial);
  return (
    <SwitchRow label="Sound" help="Plays a short tone." checked={on} onChange={setOn}>
      <p className="probe-child">Volume</p>
    </SwitchRow>
  );
}

describe("SwitchRow", () => {
  it("names and describes the switch by the row's texts", () => {
    render(<ToggleProbe initial={false} />);
    const input = host.querySelector<HTMLInputElement>('input[role="switch"]');
    expect(input).not.toBeNull();
    expect(byId(input?.getAttribute("aria-labelledby") ?? null)?.textContent).toBe("Sound");
    expect(byId(input?.getAttribute("aria-describedby") ?? null)?.textContent).toBe(
      "Plays a short tone.",
    );
  });

  it("shows dependent settings only while the switch is on", () => {
    render(<ToggleProbe initial={false} />);
    expect(host.querySelector(".probe-child")).toBeNull();
    const input = host.querySelector<HTMLInputElement>('input[role="switch"]');
    act(() => {
      input?.click();
    });
    expect(input?.checked).toBe(true);
    expect(host.querySelector(".probe-child")).not.toBeNull();
  });

  it("toggles when the row's text is clicked", () => {
    render(<ToggleProbe initial={false} />);
    const label = host.querySelector<HTMLElement>(".setting-label");
    act(() => {
      label?.click();
    });
    expect(host.querySelector<HTMLInputElement>('input[role="switch"]')?.checked).toBe(true);
  });

  it("leaves out the description when there is no help", () => {
    render(<SwitchRow label="Plain" checked onChange={() => undefined} />);
    const input = host.querySelector('input[role="switch"]');
    expect(input?.hasAttribute("aria-describedby")).toBe(false);
  });
});

describe("ActionRow", () => {
  it("keeps the status as a live region and runs the action", () => {
    const onAction = vi.fn();
    render(
      <ActionRow
        label="Hangar 1.0"
        status="Not checked yet."
        action="Check now"
        onAction={onAction}
      />,
    );
    const status = host.querySelector('[role="status"]');
    expect(status?.textContent).toBe("Not checked yet.");
    const button = host.querySelector("button");
    expect(byId(button?.getAttribute("aria-describedby") ?? null)?.textContent).toBe("Hangar 1.0");
    act(() => {
      button?.click();
    });
    expect(onAction).toHaveBeenCalledTimes(1);
  });

  it("marks an error status and describes the button by it without a label", () => {
    render(<ActionRow status="Failed" error action="Check now" onAction={() => undefined} />);
    expect(host.querySelector('[role="status"]')?.classList.contains("error-text")).toBe(true);
    const button = host.querySelector("button");
    expect(byId(button?.getAttribute("aria-describedby") ?? null)?.textContent).toBe("Failed");
  });
});

describe("SegmentedField", () => {
  it("is a radio group named by its label that reports the choice", () => {
    const onChange = vi.fn();
    render(
      <SegmentedField
        label="Cursor"
        value="bar"
        options={[
          { value: "block", label: "Block" },
          { value: "bar", label: "Bar" },
        ]}
        onChange={onChange}
      />,
    );
    const group = host.querySelector('[role="radiogroup"]');
    expect(byId(group?.getAttribute("aria-labelledby") ?? null)?.textContent).toBe("Cursor");
    const radios = host.querySelectorAll<HTMLInputElement>('input[type="radio"]');
    expect(radios).toHaveLength(2);
    expect(radios[1]?.checked).toBe(true);
    expect(radios[0]?.name).toBe(radios[1]?.name);
    act(() => {
      radios[0]?.click();
    });
    expect(onChange).toHaveBeenCalledWith("block");
  });
});

describe("SettingsGroup", () => {
  it("is a section labelled by its heading", () => {
    render(
      <SettingsGroup title="Hangar" description="Your app">
        <p>Inside</p>
      </SettingsGroup>,
    );
    const section = host.querySelector("section");
    expect(byId(section?.getAttribute("aria-labelledby") ?? null)?.tagName).toBe("H4");
    expect(host.querySelector(".setting-card")?.textContent).toBe("Inside");
    expect(host.querySelector(".setting-group-desc")?.textContent).toBe("Your app");
  });
});

describe("InlineConfirm", () => {
  it("focuses Cancel first and reports both answers", () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <InlineConfirm
        text="Delete all?"
        confirm="Delete"
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );
    const group = host.querySelector('[role="group"]');
    expect(byId(group?.getAttribute("aria-labelledby") ?? null)?.textContent).toBe("Delete all?");
    const [yes, cancel] = [...host.querySelectorAll("button")];
    expect(document.activeElement).toBe(cancel);
    act(() => {
      yes?.click();
      cancel?.click();
    });
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});

describe("Advanced", () => {
  it("starts collapsed", () => {
    render(
      <Advanced>
        <p>Rare</p>
      </Advanced>,
    );
    const details = host.querySelector("details");
    expect(details?.open).toBe(false);
    expect(details?.querySelector("summary")?.textContent).not.toBe("");
  });
});

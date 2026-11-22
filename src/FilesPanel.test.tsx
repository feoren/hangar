// @vitest-environment happy-dom

// Component test for the files filter: in a large index the search waits for a pause in typing,
// and Enter during that pause goes to the best match of what is typed now, not of the last pause.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { EMPTY_CLAIMS } from "./claims";
import { FILTER_DEBOUNCE_FROM, FILTER_DEBOUNCE_MS } from "./fileTree";
import { FilesPanel } from "./FilesPanel";
import { createTranslator, I18nContext } from "./i18n";

const backend = vi.hoisted(() => ({
  listSessionFolder: vi.fn<(id: number, relative: string) => Promise<unknown>>(),
  listSessionPaths: vi.fn<(id: number) => Promise<unknown>>(),
}));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return { ...actual, api: { ...actual.api, ...backend } };
});

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");

/** Large enough that the filter waits for a typing pause; two names to find among the rest. */
function bigIndex() {
  const entries = [
    { path: "notes", kind: "dir" },
    { path: "notes/alpha.md", kind: "file" },
    { path: "notes/beta.md", kind: "file" },
  ];
  for (let index = 0; entries.length < FILTER_DEBOUNCE_FROM; index += 1) {
    entries.push({ path: `src/part${String(index % 40)}/file${String(index)}.ts`, kind: "file" });
  }
  return { entries, truncated: false };
}

let host: HTMLElement;
let root: Root | null = null;

beforeEach(() => {
  vi.useFakeTimers();
  backend.listSessionFolder.mockResolvedValue({
    root: "C:/work/shop",
    entries: [
      { name: "notes", kind: "dir" },
      { name: "src", kind: "dir" },
    ],
  });
  backend.listSessionPaths.mockResolvedValue(bigIndex());
  host = document.createElement("div");
  document.body.append(host);
});

afterEach(() => {
  act(() => {
    root?.unmount();
  });
  root = null;
  host.remove();
  vi.useRealTimers();
});

async function settle(ms = 0) {
  await act(async () => {
    vi.advanceTimersByTime(ms);
    for (let step = 0; step < 5; step += 1) await Promise.resolve();
  });
}

async function mount() {
  root = createRoot(host);
  await act(async () => {
    root?.render(
      <I18nContext.Provider value={t}>
        <FilesPanel
          session={{ id: 1, cwd: "C:/work/shop" }}
          claims={EMPTY_CLAIMS}
          activities={new Map()}
          sessions={[]}
          nameOf={() => ""}
          focusToken={0}
          onInsertPath={() => undefined}
        />
      </I18nContext.Provider>,
    );
    await Promise.resolve();
  });
  await settle();
}

function field(): HTMLInputElement {
  const input = host.querySelector<HTMLInputElement>(".files-filter-input");
  if (!input) throw new Error("no filter field");
  return input;
}

async function type(text: string) {
  const input = field();
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(input, text);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await Promise.resolve();
  });
}

async function press(key: string) {
  await act(async () => {
    field().dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
    await Promise.resolve();
  });
}

function focusedName(): string | null {
  const active = document.activeElement;
  return active instanceof HTMLElement && active.getAttribute("role") === "treeitem"
    ? (active.querySelector(".tree-name")?.textContent ?? active.textContent)
    : null;
}

describe("FilesPanel filter", () => {
  it("goes to the best match of the typed text on Enter, also within the typing pause", async () => {
    await mount();
    const open = host.querySelector<HTMLButtonElement>(`button[aria-label="${t("files.filter")}"]`);
    await act(async () => {
      open?.click();
      await Promise.resolve();
    });
    await type("alpha");
    await settle(FILTER_DEBOUNCE_MS);
    // The index is in and searched: the large index now waits for pauses.
    expect(backend.listSessionPaths).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain("alpha.md");

    await type("beta");
    // Within the pause the list still shows the last result...
    expect(host.textContent).not.toContain("beta.md");
    await press("Enter");
    await settle();
    // ...but Enter searched "beta" at once and went to its match.
    expect(focusedName()).toContain("beta.md");
    expect(host.textContent).toContain("beta.md");
  });
});

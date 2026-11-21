// @vitest-environment happy-dom

// Wiring of the version retry in `usePrograms`: the rule itself lives in programs.test.ts
// (`retryVersions`); this file checks that the hook asks again a minute apart, stops after
// MAX_VERSION_RETRIES, asks nothing more once every version is there and leaves no timer behind.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { api } from "./api";
import { MAX_VERSION_RETRIES, VERSION_RETRY_MS } from "./programs";
import { usePrograms } from "./usePrograms";

vi.mock("./api", () => ({
  api: {
    detectVersions: vi.fn(),
    programIcons: vi.fn(() => Promise.resolve({})),
  },
}));

const detectVersions = vi.mocked(api.detectVersions);

function Probe({ installedKey }: { installedKey: string }) {
  usePrograms(installedKey, true, false);
  return null;
}

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

let root: Root;

async function render(installedKey: string) {
  await act(async () => {
    root.render(<Probe installedKey={installedKey} />);
    await vi.advanceTimersByTimeAsync(0);
  });
}

async function wait(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

/** Many retry periods, one `act` each so React runs the effects between them. */
async function waitLong() {
  for (let round = 0; round < 10; round += 1) await wait(VERSION_RETRY_MS);
}

describe("usePrograms version retry", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    detectVersions.mockReset();
    root = createRoot(document.createElement("div"));
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    vi.useRealTimers();
  });

  it("asks again a minute apart while a version is missing, then stops", async () => {
    detectVersions.mockResolvedValue({ claude: "2.1.330", opencode: null });
    await render("claude\nopencode");
    expect(detectVersions).toHaveBeenCalledTimes(1);

    await wait(VERSION_RETRY_MS - 1);
    expect(detectVersions).toHaveBeenCalledTimes(1);
    await wait(1);
    expect(detectVersions).toHaveBeenCalledTimes(2);

    await waitLong();
    expect(detectVersions).toHaveBeenCalledTimes(1 + MAX_VERSION_RETRIES);
  });

  it("stops asking as soon as the missing version arrives", async () => {
    detectVersions
      .mockResolvedValueOnce({ claude: "2.1.330", opencode: null })
      .mockResolvedValue({ claude: "2.1.330", opencode: "2.3.2" });
    await render("claude\nopencode");
    await waitLong();
    expect(detectVersions).toHaveBeenCalledTimes(2);
  });

  it("asks once when every installed program has a version", async () => {
    detectVersions.mockResolvedValue({ claude: "2.1.330" });
    await render("claude");
    await waitLong();
    expect(detectVersions).toHaveBeenCalledTimes(1);
  });

  it("starts counting again for a new program list", async () => {
    detectVersions.mockResolvedValue({});
    await render("claude");
    await waitLong();
    expect(detectVersions).toHaveBeenCalledTimes(1 + MAX_VERSION_RETRIES);
    await render("claude\nopencode");
    await waitLong();
    expect(detectVersions).toHaveBeenCalledTimes(2 * (1 + MAX_VERSION_RETRIES));
  });

  it("leaves no pending retry after unmounting", async () => {
    detectVersions.mockResolvedValue({});
    await render("claude");
    act(() => {
      root.unmount();
    });
    root = createRoot(document.createElement("div"));
    await waitLong();
    expect(detectVersions).toHaveBeenCalledTimes(1);
  });
});

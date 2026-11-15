import { describe, expect, it } from "vitest";

import { openLink, type LinkOpeners } from "./terminalLinkOpen";

/** Openers that record their calls; `fail` names the ones that reject. */
function openers(fail: (keyof LinkOpeners)[] = []) {
  const calls: string[] = [];
  const make =
    (name: keyof LinkOpeners) =>
    (value: string): Promise<unknown> => {
      calls.push(`${name} ${value}`);
      return fail.includes(name) ? Promise.reject(new Error(`${name} failed`)) : Promise.resolve();
    };
  const result: LinkOpeners = {
    openUrl: make("openUrl"),
    openPath: make("openPath"),
    openViewer: make("openViewer"),
  };
  return { calls, result };
}

const root = String.raw`E:\repo`;

describe("openLink", () => {
  it("opens an address in the browser, or shows it when that fails", async () => {
    const ok = openers();
    expect(await openLink({ kind: "url", url: "https://example.com" }, ok.result)).toEqual({
      kind: "opened",
    });
    expect(ok.calls).toEqual(["openUrl https://example.com"]);
    const failing = openers(["openUrl"]);
    expect(await openLink({ kind: "url", url: "https://example.com" }, failing.result)).toEqual({
      kind: "showUrl",
      url: "https://example.com",
    });
  });

  it("shows a file of the session's folder in the viewer", async () => {
    const { calls, result } = openers();
    const path = String.raw`E:\repo\docs\a.md`;
    await openLink({ kind: "path", path, pathKind: "file", root }, result);
    expect(calls).toEqual(["openViewer docs/a.md"]);
  });

  it("falls back to the default program when the viewer refuses", async () => {
    const { calls, result } = openers(["openViewer"]);
    const path = String.raw`E:\repo\docs\a.md`;
    expect(await openLink({ kind: "path", path, pathKind: "file", root }, result)).toEqual({
      kind: "opened",
    });
    expect(calls).toEqual(["openViewer docs/a.md", `openPath ${path}`]);
  });

  it("opens folders and outside files through the system", async () => {
    const { calls, result } = openers();
    await openLink({ kind: "path", path: String.raw`E:\repo\src`, pathKind: "dir", root }, result);
    await openLink({ kind: "path", path: String.raw`C:\x\a.pdf`, pathKind: "file", root }, result);
    expect(calls).toEqual([String.raw`openPath E:\repo\src`, String.raw`openPath C:\x\a.pdf`]);
  });

  it("reports a failure to open", async () => {
    const { result } = openers(["openPath"]);
    const outcome = await openLink(
      { kind: "path", path: String.raw`E:\repo\src`, pathKind: "dir", root },
      result,
    );
    expect(outcome.kind).toBe("failed");
  });
});

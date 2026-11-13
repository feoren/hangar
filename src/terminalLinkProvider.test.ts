import type { IDisposable, ILink, ILinkProvider } from "@xterm/xterm";
import { describe, expect, it } from "vitest";

import { PathChecker, type PathKind } from "./pathCheck";
import {
  TerminalLinks,
  type LinkLine,
  type LinkRequest,
  type LinkTerminal,
} from "./terminalLinkProvider";

/** A terminal with one cell per character; enough of xterm for the link provider. */
class FakeTerminal implements LinkTerminal {
  provider: ILinkProvider | null = null;
  rows = 24;
  selected = false;
  refreshed = 0;
  modes = { mouseTrackingMode: "none" };
  readonly buffer: LinkTerminal["buffer"];

  constructor(lines: readonly string[]) {
    const bufferLines: LinkLine[] = lines.map((text) => ({
      isWrapped: false,
      length: text.length,
      getCell: (x: number) => {
        const chars = text.charAt(x);
        return { getChars: () => chars, getWidth: () => 1 };
      },
    }));
    this.buffer = { active: { getLine: (y: number) => bufferLines[y] } };
  }

  registerLinkProvider(provider: ILinkProvider): IDisposable {
    this.provider = provider;
    return {
      dispose: () => {
        this.provider = null;
      },
    };
  }

  refresh(): void {
    this.refreshed += 1;
  }

  hasSelection(): boolean {
    return this.selected;
  }

  /** The links xterm would get for buffer line `line` (1 based). */
  links(line: number): Promise<ILink[] | undefined> {
    return new Promise((resolve) => {
      this.provider?.provideLinks(line, resolve);
    });
  }
}

/** A checker whose backend knows `existing`, answering at once. */
function checker(existing: Record<string, PathKind>): PathChecker {
  return new PathChecker((paths) => Promise.resolve(paths.map((path) => existing[path] ?? null)), {
    now: () => 0,
    later: (run) => {
      run();
    },
  });
}

/** Node has `Event` but no `MouseEvent`; xterm only passes the event on. */
function isMouse(event: Event): event is MouseEvent {
  return event.type === "mouseup";
}

function click(link: ILink | undefined): void {
  const event = new Event("mouseup", { cancelable: true });
  if (link === undefined || !isMouse(event)) throw new Error("no link");
  link.activate(event, link.text);
}

function setup(
  line: string,
  existing: Record<string, PathKind>,
  check: PathChecker = checker(existing),
) {
  const term = new FakeTerminal([line]);
  const opened: LinkRequest[] = [];
  const links = new TerminalLinks(term, {
    cwd: () => "/repo",
    checker: check,
    open: (request) => opened.push(request),
  });
  return { term, opened, links };
}

describe("TerminalLinks", () => {
  it("underlines an existing path and opens it on a click", async () => {
    const { term, opened } = setup("see docs/a.md here", { "/repo/docs/a.md": "file" });
    const links = await term.links(1);
    expect(links).toHaveLength(1);
    expect(links?.[0]?.range).toEqual({ start: { x: 5, y: 1 }, end: { x: 13, y: 1 } });
    expect(links?.[0]?.decorations).toEqual({ underline: true, pointerCursor: true });
    click(links?.[0]);
    expect(opened).toEqual([
      { kind: "path", path: "/repo/docs/a.md", pathKind: "file", root: "/repo" },
    ]);
  });

  it("offers nothing for a missing path", async () => {
    const { term } = setup("see docs/missing.md", {});
    expect(await term.links(1)).toBeUndefined();
  });

  it("drops a stale answer when xterm asked again meanwhile", async () => {
    let answer: (value: unknown) => void = () => undefined;
    const slow = new PathChecker(
      () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
      {
        now: () => 0,
        later: (run) => {
          run();
        },
      },
    );
    const { term } = setup("see docs/a.md", {}, slow);
    const answers: string[] = [];
    term.provider?.provideLinks(1, () => answers.push("first"));
    term.provider?.provideLinks(1, () => answers.push("second"));
    answer(["file"]);
    for (let round = 0; round < 10; round += 1) await Promise.resolve();
    expect(answers).toEqual(["second"]);
  });

  it("leaves plain clicks to a program that tracks the mouse", async () => {
    const { term, opened } = setup("see docs/a.md", { "/repo/docs/a.md": "file" });
    term.modes.mouseTrackingMode = "vt200";
    const links = await term.links(1);
    expect(links?.[0]?.decorations).toEqual({ underline: false, pointerCursor: false });
    click(links?.[0]);
    expect(opened).toEqual([]);
  });

  it("splits a path into segments with Shift; an inner one opens its folder", async () => {
    const { term, opened, links } = setup("see docs/wiki/a.md", {
      "/repo/docs/wiki/a.md": "file",
    });
    const element = new EventTarget();
    const host = new EventTarget();
    links.watch(element, host);
    const shift = new Event("keydown");
    Object.defineProperty(shift, "key", { value: "Shift" });
    host.dispatchEvent(shift);
    const parts = await term.links(1);
    expect(parts?.map((link) => link.text)).toEqual(["docs", "docs/wiki", "docs/wiki/a.md"]);
    expect(parts?.[1]?.range).toEqual({ start: { x: 10, y: 1 }, end: { x: 13, y: 1 } });
    click(parts?.[1]);
    expect(opened).toEqual([
      { kind: "path", path: "/repo/docs/wiki", pathKind: "dir", root: "/repo" },
    ]);
    // Shift also works where the program tracks the mouse: xterm then keeps the click.
    term.modes.mouseTrackingMode = "vt200";
    const tracked = await term.links(1);
    expect(tracked?.[0]?.decorations).toEqual({ underline: true, pointerCursor: true });
    links.dispose();
  });

  it("redraws a hovered link when Shift changes", async () => {
    const { term, links } = setup("see docs/a.md", { "/repo/docs/a.md": "file" });
    const host = new EventTarget();
    links.watch(new EventTarget(), host);
    const found = (await term.links(1))?.[0];
    const move = new Event("mouseup");
    if (found?.hover === undefined || !isMouse(move)) throw new Error("no link");
    found.hover(move, found.text);
    const shift = new Event("keydown");
    Object.defineProperty(shift, "key", { value: "Shift" });
    host.dispatchEvent(shift);
    expect(term.refreshed).toBe(1);
  });

  it("opens nothing while text is selected", async () => {
    const { term, opened } = setup("see docs/a.md", { "/repo/docs/a.md": "file" });
    term.selected = true;
    click((await term.links(1))?.[0]);
    expect(opened).toEqual([]);
  });

  it("links web addresses without asking the backend", async () => {
    let calls = 0;
    const counting = new PathChecker(
      () => {
        calls += 1;
        return Promise.resolve([]);
      },
      {
        now: () => 0,
        later: (run) => {
          run();
        },
      },
    );
    const { term, opened } = setup("open https://example.com/x now", {}, counting);
    click((await term.links(1))?.[0]);
    expect(opened).toEqual([{ kind: "url", url: "https://example.com/x" }]);
    expect(calls).toBe(0);
  });
});

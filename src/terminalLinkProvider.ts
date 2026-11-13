// Connects terminalLinks.ts to one xterm instance: web addresses and existing paths in the output
// become links. A plain click opens them, with Shift held a path splits into its segments.
//
// xterm asks for links when the mouse reaches a line (and again when that line is redrawn), never
// per mouse move. Paths are checked through the shared PathChecker, so a line's paths cost at most
// one backend call, and none while they are remembered.
//
// When the program tracks the mouse itself (full screen programs), a plain click is the
// program's: links are then inert and invisible. Shift bypasses mouse tracking in xterm, so
// Shift+click still opens a segment there.

import type { IDisposable, ILink, ILinkProvider } from "@xterm/xterm";

import type { PathChecker, PathKind } from "./pathCheck";
import {
  cellText,
  findLinks,
  linkSpans,
  pickLinks,
  resolvePath,
  type CellText,
  type LinkCell,
  type TextLink,
} from "./terminalLinks";

/** What a click asks the app to open. `path` is absolute, in the style of the session's folder. */
export type LinkRequest =
  { kind: "url"; url: string } | { kind: "path"; path: string; pathKind: PathKind; root: string };

export interface TerminalLinkOptions {
  /** The session's folder; relative paths resolve against it. `null` while unknown. */
  cwd: () => string | null;
  checker: PathChecker;
  open: (request: LinkRequest) => void;
}

/**
 * What the links need of xterm's `Terminal`; narrower, so tests can stand in for it. The real
 * terminal fits as it is.
 */
export interface LinkTerminal {
  registerLinkProvider(provider: ILinkProvider): IDisposable;
  refresh(start: number, end: number): void;
  readonly rows: number;
  hasSelection(): boolean;
  readonly modes: { readonly mouseTrackingMode: string };
  readonly buffer: { readonly active: { getLine(y: number): LinkLine | undefined } };
}

/** A buffer line as far as the links read it. */
export interface LinkLine {
  readonly isWrapped: boolean;
  readonly length: number;
  getCell(x: number): { getChars(): string; getWidth(): number } | undefined;
}

/** A soft wrapped line longer than this many rows is only searched in these rows. */
const MAX_ROWS = 16;

interface Candidate {
  link: TextLink;
  /** For paths: resolved against the session's folder; `null` for addresses. */
  absolute: string | null;
}

export class TerminalLinks implements IDisposable {
  private shift = false;
  /** A link of this terminal is under the mouse (xterm's hover and leave). */
  private hovered = false;
  /** The last click opened a link; the click must not also move the prompt's cursor. */
  private opened = false;
  private requests = 0;
  private readonly disposables: IDisposable[] = [];

  constructor(
    private readonly term: LinkTerminal,
    private readonly options: TerminalLinkOptions,
  ) {
    this.disposables.push(
      term.registerLinkProvider({
        provideLinks: (line, callback) => {
          this.provide(line, callback);
        },
      }),
    );
  }

  /**
   * Watches Shift on the terminal's element and the window (`host`, the window by default). Call
   * once the element exists.
   */
  watch(element: EventTarget, host: EventTarget = window): void {
    const onMove = (event: Event) => {
      this.setShift("shiftKey" in event && event.shiftKey === true);
    };
    const onKey = (event: Event) => {
      if ("key" in event && event.key === "Shift") this.setShift(event.type === "keydown");
    };
    const onBlur = () => {
      this.setShift(false);
    };
    // Capture: before xterm's own handler on the element asks for the link under the mouse.
    element.addEventListener("mousemove", onMove, { capture: true });
    host.addEventListener("keydown", onKey);
    host.addEventListener("keyup", onKey);
    host.addEventListener("blur", onBlur);
    this.disposables.push({
      dispose: () => {
        element.removeEventListener("mousemove", onMove, { capture: true });
        host.removeEventListener("keydown", onKey);
        host.removeEventListener("keyup", onKey);
        host.removeEventListener("blur", onBlur);
      },
    });
  }

  /** Whether the click that just ended opened a link; resets. */
  takeOpened(): boolean {
    const opened = this.opened;
    this.opened = false;
    return opened;
  }

  dispose(): void {
    for (const disposable of this.disposables) disposable.dispose();
    this.disposables.length = 0;
  }

  private setShift(shift: boolean): void {
    if (shift === this.shift) return;
    this.shift = shift;
    // xterm keeps the hovered link until the mouse leaves it. A redraw of the rows makes it ask
    // again, now with or without segments.
    if (this.hovered) this.term.refresh(0, this.term.rows - 1);
  }

  private provide(line: number, callback: (links: ILink[] | undefined) => void): void {
    this.requests += 1;
    const request = this.requests;
    const cwd = this.options.cwd();
    const cells = this.logicalLine(line);
    const candidates: Candidate[] = [];
    for (const link of findLinks(cells.text)) {
      const first = cells.from[link.start];
      const last = cells.to[link.end - 1];
      if (first === undefined || last === undefined || first.y > line || last.y < line) continue;
      if (link.target.kind === "url") {
        candidates.push({ link, absolute: null });
        continue;
      }
      const absolute = cwd === null ? null : resolvePath(cwd, link.target.path);
      if (absolute !== null) candidates.push({ link, absolute });
    }
    if (candidates.length === 0) {
      callback(undefined);
      return;
    }
    const finish = () => {
      // xterm asked again meanwhile (another line, a redraw): this answer is stale.
      if (request !== this.requests) return;
      callback(this.build(cells, candidates, cwd ?? ""));
    };
    const unknown = candidates
      .map((candidate) => candidate.absolute)
      .filter(
        (path): path is string => path !== null && this.options.checker.state(path) === undefined,
      );
    if (unknown.length === 0) finish();
    else void this.options.checker.lookup(unknown).then(finish);
  }

  private build(
    cells: CellText,
    candidates: readonly Candidate[],
    root: string,
  ): ILink[] | undefined {
    const checker = this.options.checker;
    const kindOf = (candidate: Candidate): PathKind | null => {
      if (candidate.absolute === null) return null;
      const state = checker.state(candidate.absolute);
      return state === "file" || state === "dir" ? state : null;
    };
    const live = pickLinks(
      candidates.map((candidate) => ({ ...candidate.link, candidate })),
      ({ candidate }) => candidate.absolute === null || kindOf(candidate) !== null,
    );
    const inert = this.term.modes.mouseTrackingMode !== "none" && !this.shift;
    const links: ILink[] = [];
    for (const { candidate } of live) {
      const kind = kindOf(candidate);
      for (const span of linkSpans(candidate.link, this.shift)) {
        const start = cells.from[span.start];
        const end = cells.to[span.end - 1];
        if (start === undefined || end === undefined) continue;
        const request = this.requestFor(span.opens, span.whole ? kind : "dir", root);
        if (request === null) continue;
        links.push({
          range: { start, end },
          text: span.opens,
          decorations: { underline: !inert, pointerCursor: !inert },
          activate: (event) => {
            if (inert || this.term.hasSelection()) return;
            event.preventDefault();
            this.opened = true;
            this.options.open(request);
          },
          hover: () => {
            this.hovered = true;
          },
          leave: () => {
            this.hovered = false;
          },
        });
      }
    }
    return links.length === 0 ? undefined : links;
  }

  private requestFor(opens: string, kind: PathKind | null, root: string): LinkRequest | null {
    if (kind === null) return { kind: "url", url: opens };
    const path = resolvePath(root, opens);
    return path === null ? null : { kind: "path", path, pathKind: kind, root };
  }

  /** The rows of the soft wrapped line that holds buffer line `line` (1 based). */
  private logicalLine(line: number): CellText {
    const buffer = this.term.buffer.active;
    let first = line - 1;
    while (first > 0 && line - 1 - first < MAX_ROWS && buffer.getLine(first)?.isWrapped === true) {
      first -= 1;
    }
    const rows: LinkCell[][] = [];
    for (let row = first; row < first + MAX_ROWS; row += 1) {
      const bufferLine = buffer.getLine(row);
      if (bufferLine === undefined || (row > first && !bufferLine.isWrapped)) break;
      const cells: LinkCell[] = [];
      for (let x = 0; x < bufferLine.length; x += 1) {
        const cell = bufferLine.getCell(x);
        cells.push({ chars: cell?.getChars() ?? "", width: cell?.getWidth() ?? 1 });
      }
      rows.push(cells);
    }
    return cellText(rows, first);
  }
}

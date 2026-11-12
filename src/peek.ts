// Session peek: resting the mouse on a session row in the sidebar shows the last lines of that
// session's terminal next to the sidebar. Pure: timing, placement and the comparison that keeps
// React from rendering unchanged lines are testable without a DOM.

/** How long the mouse rests on a row before the peek shows; shorter would flicker while moving. */
export const PEEK_DELAY_MS = 400;
/** How often a shown peek reads the terminal again. */
export const PEEK_REFRESH_MS = 500;
/** Lines of output a peek shows. */
export const PEEK_LINES = 12;
/** Space between the sidebar's edge and the peek, and between the peek and the window's edges. */
export const PEEK_GAP = 8;

/** Widest the peek gets; wider lines are cut at its edge. */
export const PEEK_MAX_WIDTH = 440;
/** Narrower than this, a peek shows too little to be worth covering the terminal. */
export const PEEK_MIN_WIDTH = 240;

/**
 * Width of the peek in the room right of the sidebar, or `null` when that room is too small:
 * a peek over the sidebar would cover the very row it describes.
 */
export function peekWidth(sidebarRight: number, viewportWidth: number): number | null {
  const room = viewportWidth - sidebarRight - 2 * PEEK_GAP;
  return room < PEEK_MIN_WIDTH ? null : Math.min(PEEK_MAX_WIDTH, room);
}

export interface PeekAnchor {
  /** The session whose row the mouse rests on. */
  id: number;
  /** The row's top and bottom edge in window coordinates. */
  top: number;
  bottom: number;
  /** Right edge of the sidebar; the peek goes beside it. */
  sidebarRight: number;
  /** Width of the peek (peekWidth). */
  width: number;
}

export interface PeekBox {
  width: number;
  height: number;
}

/**
 * Where the peek goes: right of the sidebar, its top level with the row's top. Near the bottom
 * of the window it moves up so it stays whole; a window too small for it keeps its top edge
 * visible first.
 */
export function peekPosition(
  anchor: Pick<PeekAnchor, "top" | "sidebarRight">,
  box: PeekBox,
  viewport: PeekBox,
): { left: number; top: number } {
  const { sidebarRight } = anchor;
  const left = Math.max(
    PEEK_GAP,
    Math.min(sidebarRight + PEEK_GAP, viewport.width - box.width - PEEK_GAP),
  );
  const lowest = viewport.height - box.height - PEEK_GAP;
  const top = Math.max(PEEK_GAP, Math.min(anchor.top, lowest));
  return { left, top };
}

/**
 * What a row only hints at with a small mark, spelled out: session rows have no tooltip, so the
 * peek is where these are read.
 */
export interface PeekNoteSources {
  /** Another session or a teammate changes the same files (claims.ts marker text). */
  overlap?: string | null | undefined;
  /** Why the gate is off for this session (gate.ts gateHint). */
  gateOff?: string | null | undefined;
  /** Exit code of a session that failed (format.ts exitDetail). */
  exit?: string | null | undefined;
  /** "CPU 12 % · 340 MB" (resources.ts resourceText). */
  resources?: string | null | undefined;
  /** Why the session is marked as an outlier. */
  outlier?: string | null | undefined;
  /** Address of the dev server the session runs. */
  devServer?: string | null | undefined;
  /** The session's program and its version ("PowerShell 7 · 7.6.9"). */
  program?: string | null | undefined;
  /** The session's working folder. */
  cwd?: string | null | undefined;
}

/**
 * The muted lines under the peek's title: what needs action first (overlap, gate, exit code),
 * then the load, the dev server and the program, then the working folder. Empty and repeated texts are left
 * out.
 */
export function peekNotes(sources: PeekNoteSources): string[] {
  const notes: string[] = [];
  const ordered = [
    sources.overlap,
    sources.gateOff,
    sources.exit,
    sources.outlier,
    sources.resources,
    sources.devServer,
    sources.program,
    sources.cwd,
  ];
  for (const text of ordered) {
    const line = text?.trim() ?? "";
    if (line !== "" && !notes.includes(line)) notes.push(line);
  }
  return notes;
}

/**
 * Whether a peek shows: one with terminal lines once it has some, one without (the session is on
 * screen anyway) always, for its title, status and notes.
 */
export function peekShows(withLines: boolean, lines: readonly string[] | null): boolean {
  return !withLines || (lines !== null && lines.length > 0);
}

export function sameLines(a: readonly string[] | null, b: readonly string[] | null): boolean {
  if (a === null || b === null) return a === b;
  return a.length === b.length && a.every((line, index) => line === b[index]);
}

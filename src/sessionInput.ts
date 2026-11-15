// Input to a session (`write_session`). The backend takes at most 1 MiB per call and answers
// `false` while the session's input queue is full (its program does not read right now). Here a
// large paste is split into pieces below that limit, a full queue is waited out, and the writes
// of one session stay in order.

/** UTF-16 units per piece: at most 3 bytes of UTF-8 each, so a piece stays below 1 MiB. */
export const PIECE_UNITS = 256 * 1024;
/** How long a full queue is waited for before the write gives up. */
export const BUSY_LIMIT_MS = 30_000;

/** `text` in pieces of at most `units` UTF-16 units; a surrogate pair is never cut in two. */
export function splitInput(text: string, units: number = PIECE_UNITS): string[] {
  if (text.length <= units) return [text];
  const pieces: string[] = [];
  let start = 0;
  while (start < text.length) {
    let end = Math.min(start + units, text.length);
    const last = text.charCodeAt(end - 1);
    // A high surrogate at the end belongs to the next unit: keep the pair together.
    if (end < text.length && end - start > 1 && last >= 0xd800 && last <= 0xdbff) end -= 1;
    pieces.push(text.slice(start, end));
    start = end;
  }
  return pieces;
}

export interface InputTiming {
  /** Waits `ms`; replaced in tests. */
  sleep: (ms: number) => Promise<void>;
  /** Milliseconds since some start; replaced in tests. */
  now: () => number;
}

const realTiming: InputTiming = {
  sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  now: () => performance.now(),
};

/** Start and end of a bracketed paste (DECSET 2004), as `term.paste` wraps the text. */
export const PASTE_START = "\x1b[200~";
export const PASTE_END = "\x1b[201~";
/** How long the end of an abandoned paste is waited for: short, the notice is due. */
export const PASTE_END_LIMIT_MS = 2_000;

/**
 * The program did not take its input for [`BUSY_LIMIT_MS`]; what was not sent is dropped. Carries
 * no display text: the window names it in its own words (`errorText`). `dropped` marks a write
 * that never got its turn because an earlier one of the same session stalled (`inputQueue`); the
 * stall itself is reported once, by the write that waited.
 */
export class InputStalled extends Error {
  readonly dropped: boolean;

  constructor(dropped = false) {
    super("InputStalled");
    this.name = "InputStalled";
    this.dropped = dropped;
  }
}

/** Sends `piece` again after a short wait while `send` answers `false`; whether it got through. */
async function sendPatiently(
  send: (piece: string) => Promise<boolean>,
  piece: string,
  limit: number,
  timing: InputTiming,
): Promise<boolean> {
  const since = timing.now();
  let delay = 10;
  while (!(await send(piece))) {
    if (timing.now() - since > limit) return false;
    await timing.sleep(delay);
    delay = Math.min(delay * 2, 200);
  }
  return true;
}

/**
 * Sends `text` through `send` (one `write_session` call per piece): each piece again after a
 * short wait while the backend answers `false`, until [`BUSY_LIMIT_MS`] passed without progress.
 * Then the rest is dropped and [`InputStalled`] thrown. When the dropped rest held the end of a
 * bracketed paste, that end is sent on its own, or the program would stay in paste mode and take
 * every later key as pasted text.
 */
export async function writeInput(
  send: (piece: string) => Promise<boolean>,
  text: string,
  timing: InputTiming = realTiming,
): Promise<void> {
  let sentUnits = 0;
  for (const piece of splitInput(text)) {
    if (!(await sendPatiently(send, piece, BUSY_LIMIT_MS, timing))) {
      if (
        text.startsWith(PASTE_START) &&
        // Only a paste the program saw start needs an end; a lone end would arrive as keys.
        sentUnits >= PASTE_START.length &&
        text.lastIndexOf(PASTE_END) + PASTE_END.length > sentUnits
      ) {
        // Best effort: the stall is reported either way.
        await sendPatiently(send, PASTE_END, PASTE_END_LIMIT_MS, timing).catch(() => false);
      }
      throw new InputStalled();
    }
    sentUnits += piece.length;
  }
}

/**
 * Settles once `write` did: rejects only when the input stalled, so "Send to several" sends no
 * Enter after a paste that did not arrive. Other failures (session gone) resolve as before.
 */
export function settleStalled(write: Promise<unknown>): Promise<void> {
  return write.then(
    () => undefined,
    (error: unknown) => {
      if (error instanceof InputStalled) throw error;
    },
  );
}

/**
 * One chain of writes per session, so a piece waiting for a full queue is never overtaken by a
 * keystroke typed after it. A failed write does not block the ones after it, with one exception:
 * when a write stalls (`InputStalled`), every write of that session queued before the stall was
 * known is dropped as well. Otherwise keys typed while the program did not read would arrive with
 * a gap: the `n` of `n` and Enter lost, the Enter delivered on its own. Writes queued after the
 * stall go out as usual.
 */
export function inputQueue(
  send: (id: number, piece: string) => Promise<boolean>,
  timing: InputTiming = realTiming,
): (id: number, text: string) => Promise<void> {
  const chains = new Map<number, Promise<void>>();
  /** Per session: the number of the last write queued so far. */
  const queued = new Map<number, number>();
  /** Per session: writes up to this number are dropped, they were queued before a stall. */
  const dropThrough = new Map<number, number>();
  return (id, text) => {
    const number = (queued.get(id) ?? 0) + 1;
    queued.set(id, number);
    const write = async () => {
      if (number <= (dropThrough.get(id) ?? 0)) throw new InputStalled(true);
      try {
        await writeInput((piece) => send(id, piece), text, timing);
      } catch (error) {
        if (error instanceof InputStalled) dropThrough.set(id, queued.get(id) ?? number);
        throw error;
      }
    };
    const previous = chains.get(id) ?? Promise.resolve();
    const next = previous.catch(() => undefined).then(write);
    chains.set(id, next);
    const forget = () => {
      if (chains.get(id) !== next) return;
      chains.delete(id);
      queued.delete(id);
      dropThrough.delete(id);
    };
    next.then(forget, forget);
    return next;
  };
}

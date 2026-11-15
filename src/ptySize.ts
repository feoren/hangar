// Which terminal size the PTY of a session hears of, and when. A drag resizes xterm on every
// frame; the program in the session only hears of the size the drag ends on (each resize makes it
// redraw), never of a size its PTY already has, and never loses the last one, not even while the
// backend has not delivered the session's ID yet.

import type { TermSize } from "./api";
import { LatestDebounce, PTY_RESIZE_DEBOUNCE_MS } from "./latestDebounce";

export class PtySizeSync {
  private readonly debounce: LatestDebounce<TermSize>;
  private id: number | null = null;
  /** A size that settled before the backend delivered the ID; `attach` sends it. */
  private unsent: TermSize | null = null;
  /** The size the PTY has as far as the app knows. */
  private known: TermSize | null = null;
  private disposed = false;

  constructor(
    private readonly send: (id: number, size: TermSize) => void,
    delayMs: number = PTY_RESIZE_DEBOUNCE_MS,
  ) {
    this.debounce = new LatestDebounce((size) => {
      this.deliver(size);
    }, delayMs);
  }

  /** xterm took a new size; the PTY follows once the sizes rest. */
  resized(size: TermSize): void {
    this.debounce.push(size);
  }

  /** The backend delivered the session's ID; a size that settled before goes out now. */
  attach(id: number): void {
    this.id = id;
    const size = this.unsent;
    this.unsent = null;
    if (size !== null) this.deliver(size);
  }

  /** A new process starts with `size` (create, restart): its PTY has it already. */
  started(size: TermSize): TermSize {
    this.known = size;
    return size;
  }

  /** The terminal is gone: nothing more is sent. */
  dispose(): void {
    this.disposed = true;
    this.debounce.cancel();
  }

  private deliver(size: TermSize): void {
    if (this.disposed) return;
    if (this.id === null) {
      this.unsent = size;
      return;
    }
    const known = this.known;
    if (known !== null && known.cols === size.cols && known.rows === size.rows) return;
    this.known = size;
    this.send(this.id, size);
  }
}

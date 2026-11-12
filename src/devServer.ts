// Finds the address of a local dev server ("Local: http://localhost:5173/") in a session's
// output, so the sidebar can offer to open it. Pure, so it can be tested without xterm.
//
// Only the origin (scheme, host, port) is kept, never a path: a server that logs request URLs or
// an agent that prints `curl http://localhost:3000/api/users` would otherwise point the globe at
// an API route and report a new address with every chunk.

/** Raw output kept from the previous chunk, so an address split between chunks is still found. */
const TAIL = 512;

// CSI (with parameters), OSC (up to BEL or ST) and two-byte escapes. Dev servers color the port
// on its own ("localhost:\x1b[1m5173\x1b[22m/"), so the address only exists without them.
// eslint-disable-next-line no-control-regex -- escape sequences are exactly the target here.
const ANSI = /\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\)?|[0-~])/g;

/** Scheme, loopback host and port; the last group only tells whether a slash ended the port. */
const ADDRESS = /\b(https?):\/\/(localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1?\]):(\d{1,5})(\/)?/gi;

/** Output without colors and other escape sequences. */
export function stripAnsi(text: string): string {
  return text.replace(ANSI, "");
}

/**
 * The origin a browser should open for a match, with a trailing slash: the wildcard hosts
 * (`0.0.0.0`, `[::]`) mean "all interfaces" and become `localhost`. `null` for an impossible port.
 */
export function devServerUrl(scheme: string, host: string, port: string): string | null {
  const number = Number(port);
  if (!Number.isInteger(number) || number < 1 || number > 65535) return null;
  const lower = host.toLowerCase();
  const name = lower === "0.0.0.0" || lower === "[::]" ? "localhost" : lower;
  try {
    return new URL(`${scheme.toLowerCase()}://${name}:${String(number)}/`).href;
  } catch {
    return null;
  }
}

/** The origin of the last complete dev server address in `text`, `null` without one. */
export function lastDevServerUrl(text: string): string | null {
  let found: string | null = null;
  for (const match of text.matchAll(ADDRESS)) {
    const end = match.index + match[0].length;
    // A port touching the end of the text may go on in the next chunk; a slash ends it.
    if (match[4] === undefined && end >= text.length) continue;
    const url = devServerUrl(match[1] ?? "", match[2] ?? "", match[3] ?? "");
    if (url !== null) found = url;
  }
  return found;
}

/**
 * Follows one process's output and keeps the most recent dev server origin. `reset` when the
 * process ends or starts again: an address from before belongs to a server that is gone.
 */
export class DevServerDetector {
  private tail = "";
  private found: string | null = null;

  get url(): string | null {
    return this.found;
  }

  /** Feeds an output chunk; returns `true` only when the origin changed. */
  feed(chunk: string): boolean {
    const text = this.tail + chunk;
    this.tail = text.slice(-TAIL);
    // Cheap pre-check for the common case: plenty of output, no address in it.
    if (!text.includes("://")) return false;
    const url = lastDevServerUrl(stripAnsi(text));
    if (url === null || url === this.found) return false;
    this.found = url;
    return true;
  }

  /** Forgets the address; returns `true` when there was one. */
  reset(): boolean {
    this.tail = "";
    const had = this.found !== null;
    this.found = null;
    return had;
  }
}

// Whether paths printed in a terminal exist, asked of the backend (`stat_paths`) in batches and
// remembered for a while. The terminal asks when the mouse reaches a line, never per mouse move;
// lookups arriving close together share one backend call.

export type PathKind = "file" | "dir";
export type PathState = PathKind | "missing";

/** Paths per backend call; the backend refuses more (path_links.rs, MAX_PATHS). */
export const BATCH_SIZE = 64;
/** Lookups within this time share one call. */
export const BATCH_DELAY_MS = 40;
/** An existing path stays a link this long before it is checked again. */
export const FOUND_TTL_MS = 30_000;
/** A missing path is asked again after this; agents create files while you watch. */
export const MISSING_TTL_MS = 3_000;
/** Remembered paths at most; the oldest go first. */
export const CACHE_LIMIT = 2_000;

/** The backend's answer: one entry per path, `null` when nothing is there. */
export function parsePathKinds(value: unknown, count: number): (PathKind | null)[] | null {
  if (!Array.isArray(value) || value.length !== count) return null;
  const items: unknown[] = value;
  const kinds: (PathKind | null)[] = [];
  for (const item of items) {
    if (item === "file" || item === "dir" || item === null) kinds.push(item);
    else return null;
  }
  return kinds;
}

interface Entry {
  state: PathState;
  until: number;
}

export interface CheckerClock {
  now: () => number;
  later: (run: () => void, ms: number) => void;
}

const systemClock: CheckerClock = {
  now: () => Date.now(),
  later: (run, ms) => {
    window.setTimeout(run, ms);
  },
};

export class PathChecker {
  private readonly cache = new Map<string, Entry>();
  /** Paths waiting for the next batch, with whoever waits for them. */
  private queued = new Map<string, (() => void)[]>();
  /** Paths a running backend call asks about. */
  private readonly inFlight = new Map<string, Promise<void>>();
  private scheduled = false;

  constructor(
    private readonly check: (paths: string[]) => Promise<unknown>,
    private readonly clock: CheckerClock = systemClock,
  ) {}

  /** The remembered state, `undefined` when unknown or too old. */
  state(path: string): PathState | undefined {
    const entry = this.cache.get(path);
    if (entry === undefined) return undefined;
    if (entry.until <= this.clock.now()) {
      this.cache.delete(path);
      return undefined;
    }
    return entry.state;
  }

  /** Resolves once every path has a state (a failed call counts as missing, briefly). */
  lookup(paths: readonly string[]): Promise<void> {
    const waits: Promise<void>[] = [];
    for (const path of new Set(paths)) {
      if (this.state(path) !== undefined) continue;
      const running = this.inFlight.get(path);
      if (running !== undefined) {
        waits.push(running);
        continue;
      }
      waits.push(
        new Promise<void>((resolve) => {
          const waiting = this.queued.get(path);
          if (waiting === undefined) this.queued.set(path, [resolve]);
          else waiting.push(resolve);
        }),
      );
    }
    if (this.queued.size > 0 && !this.scheduled) {
      this.scheduled = true;
      this.clock.later(() => {
        this.flush();
      }, BATCH_DELAY_MS);
    }
    return Promise.all(waits).then(() => undefined);
  }

  private flush(): void {
    this.scheduled = false;
    const queued = [...this.queued];
    this.queued = new Map();
    for (let index = 0; index < queued.length; index += BATCH_SIZE) {
      this.send(queued.slice(index, index + BATCH_SIZE));
    }
  }

  private send(batch: [string, (() => void)[]][]): void {
    const paths = batch.map(([path]) => path);
    const done = this.check(paths)
      .then((answer) => parsePathKinds(answer, paths.length))
      .catch(() => null)
      .then((kinds) => {
        paths.forEach((path, index) => {
          const kind = kinds?.[index] ?? null;
          this.remember(path, kind ?? "missing");
          this.inFlight.delete(path);
        });
        for (const [, waiting] of batch) for (const resolve of waiting) resolve();
      });
    for (const path of paths) this.inFlight.set(path, done);
  }

  private remember(path: string, state: PathState): void {
    this.cache.delete(path);
    const ttl = state === "missing" ? MISSING_TTL_MS : FOUND_TTL_MS;
    this.cache.set(path, { state, until: this.clock.now() + ttl });
    while (this.cache.size > CACHE_LIMIT) {
      const oldest = this.cache.keys().next();
      if (oldest.done === true) break;
      this.cache.delete(oldest.value);
    }
  }
}

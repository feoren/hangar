// Power saving mode (Rust: `PowerState` in src-tauri/src/power.rs). Pure: the backend's answer,
// whether saving is on, the poll intervals while it is, and the store that holds the state and
// tells the pollers about a change. usePower.ts wires the store to the backend and the page.

import { BRANCH_REFRESH_MS } from "./gitBranches";
import { RESOURCE_POLL_MS } from "./resources";
import { SYSTEM_POLL_MS } from "./systemUsage";

export const POWER_STATES = ["unknown", "noBattery", "ac", "battery", "saver"] as const;
/**
 * Where the power comes from: `unknown` where the platform cannot tell, `noBattery` on a machine
 * without one, `saver` on battery with the system's battery saver on.
 */
export type PowerState = (typeof POWER_STATES)[number];

/** Class on the root element while saving: the row pulses stand still (styles.css). */
export const SAVING_CLASS = "power-saving";

/** The backend's answer; anything malformed counts as `unknown`. */
export function parsePowerState(value: unknown): PowerState {
  return POWER_STATES.find((state) => state === value) ?? "unknown";
}

/** Whether the machine has a battery Hangar can watch; only then is the setting offered. */
export function powerKnown(state: PowerState): boolean {
  return state === "ac" || state === "battery" || state === "saver";
}

/** Saving is on while the machine runs on battery and the setting is on; on AC never. */
export function savingActive(state: PowerState, enabled: boolean): boolean {
  return enabled && (state === "battery" || state === "saver");
}

/** The periodic work that slows down while saving. */
export type Poll = "system" | "resources" | "branches";

const NORMAL_MS: Readonly<Record<Poll, number>> = {
  system: SYSTEM_POLL_MS,
  resources: RESOURCE_POLL_MS,
  branches: BRANCH_REFRESH_MS,
};

/**
 * While saving: the system bar and the session rows every 5 s instead of 1.5 and 2 s, the git
 * branches every minute instead of every 15 s. Slow enough to let the processor sleep between
 * readings, fast enough that the numbers still follow a build.
 */
const SAVING_MS: Readonly<Record<Poll, number>> = {
  system: 5000,
  resources: 5000,
  branches: 60_000,
};

/** How often `poll` runs; on AC always its normal interval. */
export function pollInterval(poll: Poll, saving: boolean): number {
  return saving ? SAVING_MS[poll] : NORMAL_MS[poll];
}

/**
 * One repeating poll for a store outside React: at most one timer, which runs `run` at once on
 * every start and starts over when its interval changes (saving turned on or off).
 */
export class PollTimer {
  private timer: ReturnType<typeof setInterval> | null = null;
  private ms = 0;

  constructor(private readonly run: () => void) {}

  /** Runs every `ms` while `wanted`; stops otherwise. Calling it again with the same values does nothing. */
  set(wanted: boolean, ms: number): void {
    if (this.timer !== null && (!wanted || ms !== this.ms)) {
      clearInterval(this.timer);
      this.timer = null;
    }
    if (wanted && this.timer === null) {
      this.run();
      this.timer = setInterval(this.run, ms);
      this.ms = ms;
    }
  }
}

/** The power state and the setting; tells its listeners whenever either changes. */
export class PowerStore {
  private current: PowerState = "unknown";
  private enabled = true;
  private readonly listeners = new Set<() => void>();

  state(): PowerState {
    return this.current;
  }

  saving(): boolean {
    return savingActive(this.current, this.enabled);
  }

  /** A state from the backend (command answer or event). */
  accept(value: unknown): void {
    const next = parsePowerState(value);
    if (next === this.current) return;
    this.current = next;
    this.notify();
  }

  /** The setting "save energy on battery". */
  setEnabled(on: boolean): void {
    if (on === this.enabled) return;
    this.enabled = on;
    this.notify();
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private notify(): void {
    for (const listener of this.listeners) listener();
  }
}

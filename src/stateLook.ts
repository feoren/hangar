// How each session state looks in the sidebar: its color, how strongly the row pulses and how long
// one pulse takes. Stored in the settings; `color: null` means the state's own default, so the
// defaults keep today's look in both themes (working and pause follow the theme's green). Pure, so
// parsing, defaults and the pulse levels can be tested.

import type { Theme } from "./theme";

/** Working, done (waiting for you), asks (a question or decision) and pause (running, quiet). */
export const LOOK_STATES = ["working", "done", "asks", "idle"] as const;
export type LookState = (typeof LOOK_STATES)[number];

export const PULSE_STRENGTHS = ["off", "subtle", "medium", "strong"] as const;
export type PulseStrength = (typeof PULSE_STRENGTHS)[number];

/** Length of one pulse in seconds. */
export const PULSE_SECONDS = { min: 0.8, max: 4, step: 0.2, default: 2.8 } as const;

export interface StateLook {
  /** Lowercase `#rrggbb`, or `null` for the state's default color. */
  color: string | null;
  pulse: PulseStrength;
  seconds: number;
}

export type StateLooks = Readonly<Record<LookState, StateLook>>;

/** The calm cream that has marked waiting sessions since the first version. */
export const CREAM = "#f2e3c6";

/** Mirrors `--ok` per theme: the default of working and pause. */
const OK: Record<Theme, string> = { dark: "#57ab5a", light: "#1a7f37" };

export const DEFAULT_LOOKS: StateLooks = {
  working: { color: null, pulse: "off", seconds: PULSE_SECONDS.default },
  done: { color: null, pulse: "medium", seconds: PULSE_SECONDS.default },
  asks: { color: null, pulse: "medium", seconds: PULSE_SECONDS.default },
  idle: { color: null, pulse: "off", seconds: PULSE_SECONDS.default },
};

/** Done and asks wait for the user: their row is always tinted, even without a pulse. */
export function lookWaits(state: LookState): boolean {
  return state === "done" || state === "asks";
}

/** Tasteful choices beside the default; light enough for the dark sidebar, edges adapt in light. */
export const LOOK_PRESETS = [
  { id: "cream", color: CREAM },
  { id: "amber", color: "#f0b429" },
  // Cooler than the green of "running" (--ok), so waiting and running are not confused.
  { id: "mint", color: "#9ee8dc" },
  { id: "sky", color: "#8cc8ff" },
  { id: "violet", color: "#c4b0ff" },
  { id: "pink", color: "#f5a3c7" },
] as const;

export type LookPresetId = (typeof LOOK_PRESETS)[number]["id"];

/** `#rgb` or `#rrggbb`, normalized to `#rrggbb`; anything else yields `null`. */
export function normalizeColor(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const short = /^#([0-9a-f])([0-9a-f])([0-9a-f])$/i.exec(value);
  if (short) {
    const doubled = short.slice(1).map((digit) => digit + digit);
    return `#${doubled.join("")}`.toLowerCase();
  }
  return /^#[0-9a-f]{6}$/i.test(value) ? value.toLowerCase() : null;
}

/** The color a state shows when the user picked none. */
export function defaultColor(state: LookState, theme: Theme): string {
  return lookWaits(state) ? CREAM : OK[theme];
}

/** The color a state shows right now, as `#rrggbb`. */
export function resolvedColor(looks: StateLooks, state: LookState, theme: Theme): string {
  return looks[state].color ?? defaultColor(state, theme);
}

/** Presets for one state, without the one its default already shows. */
export function presetsFor(state: LookState): readonly (typeof LOOK_PRESETS)[number][] {
  return lookWaits(state) ? LOOK_PRESETS.filter((preset) => preset.color !== CREAM) : LOOK_PRESETS;
}

/** Rounded to the step size and clamped; one decimal, so 2.8 stays 2.8. */
export function clampPulseSeconds(value: number): number {
  const { min, max, step } = PULSE_SECONDS;
  const stepped = min + Math.round((value - min) / step) * step;
  return Math.round(Math.min(max, Math.max(min, stepped)) * 10) / 10;
}

export function isPulseStrength(value: unknown): value is PulseStrength {
  return typeof value === "string" && PULSE_STRENGTHS.some((strength) => strength === value);
}

export function isDefaultLook(state: LookState, look: StateLook): boolean {
  const base = DEFAULT_LOOKS[state];
  return look.color === base.color && look.pulse === base.pulse && look.seconds === base.seconds;
}

/** Former mint default; anyone who picked it gets the new one (same choice, easier to tell apart). */
const OLD_MINT = "#7ee0b5";

/** The single attention color of older versions, lifted to its current form; `null` = default. */
export function migrateAttentionColor(value: unknown): string | null {
  const color = normalizeColor(value);
  if (color === null || color === CREAM) return null;
  return color === OLD_MINT ? "#9ee8dc" : color;
}

function parseLook(value: unknown, fallback: StateLook): StateLook {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return fallback;
  const color: unknown = Reflect.get(value, "color");
  const pulse: unknown = Reflect.get(value, "pulse");
  const seconds: unknown = Reflect.get(value, "seconds");
  return {
    color: color === null ? null : (normalizeColor(color) ?? fallback.color),
    pulse: isPulseStrength(pulse) ? pulse : fallback.pulse,
    seconds:
      typeof seconds === "number" && Number.isFinite(seconds)
        ? clampPulseSeconds(seconds)
        : fallback.seconds,
  };
}

/**
 * Reads stored looks; missing or invalid parts fall back to the defaults. Without stored looks,
 * the single attention color of older versions colors both waiting states, as it did before.
 */
export function parseStateLooks(value: unknown, legacyAttentionColor: unknown): StateLooks {
  const legacy = migrateAttentionColor(legacyAttentionColor);
  const base: StateLooks = {
    ...DEFAULT_LOOKS,
    done: { ...DEFAULT_LOOKS.done, color: legacy },
    asks: { ...DEFAULT_LOOKS.asks, color: legacy },
  };
  if (typeof value !== "object" || value === null || Array.isArray(value)) return base;
  return {
    working: parseLook(Reflect.get(value, "working"), base.working),
    done: parseLook(Reflect.get(value, "done"), base.done),
    asks: parseLook(Reflect.get(value, "asks"), base.asks),
    idle: parseLook(Reflect.get(value, "idle"), base.idle),
  };
}

export interface PulseLevels {
  /** Percent of the state color at the calm end of the pulse. */
  low: number;
  /** Percent at its peak. */
  high: number;
  /** Still tint: without a pulse, and with reduced motion. */
  still: number;
  /** Whether the row pulses at all. */
  animated: boolean;
}

/** Share of the readable peak per strength; "medium" is the breath Hangar always had. */
const SHARES: Record<Exclude<PulseStrength, "off">, { low: number; high: number }> = {
  subtle: { low: 0.45, high: 0.7 },
  medium: { low: 0.3, high: 1 },
  strong: { low: 0, high: 1 },
};

/** Still tint of a waiting row without a pulse, relative to the peak. */
const STILL_SHARE = 0.65;

/**
 * Tint levels of a row from its readable `peak` (attentionTint). Waiting rows keep a tint without
 * a pulse; working and pause show none then, exactly as before. With reduced motion, a waiting
 * row stays at its peak as it always did.
 */
export function pulseLevels(peak: number, pulse: PulseStrength, waits: boolean): PulseLevels {
  if (pulse === "off") {
    const still = waits ? Math.round(peak * STILL_SHARE) : 0;
    return { low: still, high: still, still, animated: false };
  }
  const share = SHARES[pulse];
  const high = Math.round(peak * share.high);
  return {
    low: Math.round(peak * share.low),
    high,
    still: waits ? peak : Math.round(peak * STILL_SHARE),
    animated: true,
  };
}

/** `looks` with one state's look changed. */
export function withLook(
  looks: StateLooks,
  state: LookState,
  patch: Partial<StateLook>,
): StateLooks {
  const look = { ...looks[state], ...patch };
  return {
    working: state === "working" ? look : looks.working,
    done: state === "done" ? look : looks.done,
    asks: state === "asks" ? look : looks.asks,
    idle: state === "idle" ? look : looks.idle,
  };
}

/** Which swatch shows as chosen: the default, a preset, or the custom color. */
export function chosenSwatch(
  state: LookState,
  look: StateLook,
): "default" | LookPresetId | "custom" {
  if (look.color === null) return "default";
  return presetsFor(state).find((preset) => preset.color === look.color)?.id ?? "custom";
}

/** Classes that hand a state's look to a row (styles.css); empty for a plain row. */
export function lookClass(state: LookState | null): string {
  return state === null ? "" : `look look-${state}`;
}

/** "2.8 s" in the language's number format. */
export function formatPulseSeconds(seconds: number, language: string): string {
  return new Intl.NumberFormat(language, {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  }).format(seconds);
}

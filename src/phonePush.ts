// Push to the phone (Rust: src-tauri/src/push.rs). The backend owns the settings, the secrets in
// the keychain and the sending; this module reads its answers and decides when a session's state
// is worth a push: when it has stayed waiting, done or failed for PUSH_DELAY_MS, once per state.
// Quiet hours (quietHours.ts) hold every push back.

import type { AttentionState } from "./attention";
import type { MessageKey } from "./i18n";

export type PushProvider = "off" | "ntfy" | "pushover";
export type PushEvent = "waiting" | "done" | "failed";

export const PUSH_PROVIDERS: readonly PushProvider[] = ["off", "ntfy", "pushover"];
export const PUSH_EVENTS: readonly PushEvent[] = ["waiting", "done", "failed"];

/** A session must stay in its state this long before its push goes out. */
export const PUSH_DELAY_MS = 60_000;

export type PushEvents = Record<PushEvent, boolean>;

/** What the backend shows: the settings and whether each secret is stored, never a secret. */
export interface PushView {
  provider: PushProvider;
  ntfyUrl: string;
  /** The message names the session by its name (which may hold the first prompt). */
  includeTitle: boolean;
  events: PushEvents;
  ntfyToken: boolean;
  pushoverUser: boolean;
  pushoverToken: boolean;
}

/** A save. Each secret: `null` keeps the stored one, "" removes it, any other text replaces it. */
export interface PushInput {
  provider: PushProvider;
  ntfyUrl: string;
  includeTitle: boolean;
  events: PushEvents;
  ntfyToken: string | null;
  pushoverUser: string | null;
  pushoverToken: string | null;
}

export const DEFAULT_PUSH_VIEW: PushView = {
  provider: "off",
  ntfyUrl: "",
  includeTitle: false,
  events: { waiting: true, done: true, failed: true },
  ntfyToken: false,
  pushoverUser: false,
  pushoverToken: false,
};

function isProvider(value: unknown): value is PushProvider {
  return PUSH_PROVIDERS.some((provider) => provider === value);
}

function flag(raw: object, key: string, fallback: boolean): boolean {
  const value: unknown = Reflect.get(raw, key);
  return typeof value === "boolean" ? value : fallback;
}

/** The backend's push settings; anything invalid falls back to "off" field by field. */
export function parsePushView(raw: unknown): PushView {
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return DEFAULT_PUSH_VIEW;
  const provider: unknown = Reflect.get(raw, "provider");
  const ntfyUrl: unknown = Reflect.get(raw, "ntfyUrl");
  const events: unknown = Reflect.get(raw, "events");
  const defaults = DEFAULT_PUSH_VIEW.events;
  const eventsObject = typeof events === "object" && events !== null ? events : {};
  return {
    provider: isProvider(provider) ? provider : "off",
    ntfyUrl: typeof ntfyUrl === "string" ? ntfyUrl : "",
    includeTitle: flag(raw, "includeTitle", false),
    events: {
      waiting: flag(eventsObject, "waiting", defaults.waiting),
      done: flag(eventsObject, "done", defaults.done),
      failed: flag(eventsObject, "failed", defaults.failed),
    },
    ntfyToken: flag(raw, "ntfyToken", false),
    pushoverUser: flag(raw, "pushoverUser", false),
    pushoverToken: flag(raw, "pushoverToken", false),
  };
}

/**
 * The same check as the backend's, so the form can say so before saving: https, a host, no user
 * info, query or fragment, and a topic of letters, digits, "-" and "_" at the end of the path.
 */
export function isNtfyUrl(text: string): boolean {
  let url: URL;
  try {
    url = new URL(text.trim());
  } catch {
    return false;
  }
  if (url.protocol !== "https:" || url.hostname === "") return false;
  if (url.username !== "" || url.password !== "" || url.search !== "" || url.hash !== "") {
    return false;
  }
  const topic = url.pathname.split("/").pop() ?? "";
  return /^[A-Za-z0-9_-]{1,64}$/.test(topic);
}

/** Without an access token, anyone who knows the topic reads along: it must be this long. */
export const MIN_OPEN_TOPIC_CHARS = 16;

function parsedUrl(text: string): URL | null {
  try {
    return new URL(text.trim());
  } catch {
    return null;
  }
}

/** "https://host:port" of an address, as the backend binds the token to it; `null` if none. */
export function urlOrigin(text: string): string | null {
  const url = parsedUrl(text);
  return url === null || url.origin === "null" ? null : url.origin;
}

/** The topic at the end of an ntfy address; "" without one. */
export function ntfyTopic(text: string): string {
  return parsedUrl(text)?.pathname.split("/").pop() ?? "";
}

/**
 * The stored token would go to another server: it is bound to the origin it was entered for, so
 * the form asks for it again (or for its removal) before saving.
 */
export function needsTokenAgain(
  view: Pick<PushView, "ntfyUrl" | "ntfyToken">,
  address: string,
  tokenTyped: boolean,
  tokenRemoved: boolean,
): boolean {
  if (!view.ntfyToken || tokenTyped || tokenRemoved || address.trim() === "") return false;
  return urlOrigin(address) !== urlOrigin(view.ntfyUrl);
}

/** Without a token, a topic shorter than MIN_OPEN_TOPIC_CHARS is easy to guess. */
export function isWeakTopic(address: string, hasToken: boolean): boolean {
  return !hasToken && isNtfyUrl(address) && ntfyTopic(address).length < MIN_OPEN_TOPIC_CHARS;
}

/** 128 random bits as base64url without padding: 22 characters ntfy accepts as a topic. */
export function randomTopic(
  bytes: Uint8Array = crypto.getRandomValues(new Uint8Array(16)),
): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** The address with `topic` as its last segment; ntfy.sh when there is no usable address yet. */
export function withTopic(address: string, topic: string): string {
  const url = parsedUrl(address);
  if (url === null || url.protocol !== "https:" || url.hostname === "") {
    return `https://ntfy.sh/${topic}`;
  }
  const segments = url.pathname.split("/");
  segments[segments.length - 1] = topic;
  return `${url.origin}${segments.join("/")}`;
}

/** The push a session's new state stands for; `null` for working and resting. */
export function pushEventFor(
  state: AttentionState,
  exitCode: number | null | undefined,
): PushEvent | null {
  if (state !== "attention") return null;
  if (exitCode === undefined) return "waiting";
  return exitCode === 0 ? "done" : "failed";
}

/**
 * Message template of a push, sent with its `{name}` placeholder: the backend fills in the
 * session's program and number (or its name, if allowed), never terminal content.
 */
export const PUSH_TEXTS: Record<PushEvent, MessageKey> = {
  waiting: "push.waiting",
  done: "push.done",
  failed: "push.failed",
};

const ERROR_TEXTS: Record<string, MessageKey> = {
  invalidUrl: "push.error.invalidUrl",
  invalidToken: "push.error.invalidToken",
  invalidKey: "push.error.invalidKey",
  incomplete: "push.error.incomplete",
  tokenOrigin: "push.error.tokenOrigin",
  weakTopic: "push.error.weakTopic",
  rateLimited: "push.error.rateLimited",
  keychain: "push.error.keychain",
  storage: "push.error.storage",
  network: "push.error.network",
  unauthorized: "push.error.unauthorized",
  rejected: "push.error.rejected",
};

/** The text for a backend error kind (`PushError::kind`). */
export function pushErrorKey(error: unknown): MessageKey {
  return (typeof error === "string" ? ERROR_TEXTS[error] : undefined) ?? "push.error.unknown";
}

interface Timers {
  setTimeout(callback: () => void, ms: number): number;
  clearTimeout(handle: number): void;
}

const windowTimers: Timers = {
  setTimeout: (callback, ms) => window.setTimeout(callback, ms),
  clearTimeout: (handle) => {
    window.clearTimeout(handle);
  },
};

/**
 * Holds one pending push per session. A state that lasts PUSH_DELAY_MS sends once; another state
 * before that replaces it, and the same state again changes nothing, so one stretch of waiting
 * is one push at most. The backend adds its own limit of one push per session every 30 s.
 */
export class PushScheduler {
  private readonly pending = new Map<number, { event: PushEvent; timer: number | null }>();
  private onDue: ((id: number, event: PushEvent) => void) | null = null;

  constructor(
    private readonly delayMs: number = PUSH_DELAY_MS,
    private readonly timers: Timers = windowTimers,
  ) {}

  /** Who sends a due push; set from an effect, since it reads the app's refs. */
  setSender(send: ((id: number, event: PushEvent) => void) | null): void {
    this.onDue = send;
  }

  change(id: number, event: PushEvent | null): void {
    const current = this.pending.get(id);
    if (current?.event === event) return;
    this.forget(id);
    if (event === null) return;
    const timer = this.timers.setTimeout(() => {
      // Sent: the entry stays without a timer, so the same state does not send again.
      this.pending.set(id, { event, timer: null });
      this.onDue?.(id, event);
    }, this.delayMs);
    this.pending.set(id, { event, timer });
  }

  forget(id: number): void {
    const current = this.pending.get(id);
    if (current?.timer != null) this.timers.clearTimeout(current.timer);
    this.pending.delete(id);
  }
}

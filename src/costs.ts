// Tokens and estimated cost per session and per local day (backend: src-tauri/src/cost_service.rs,
// contract: docs/wiki/pages/syntheses/costs-and-handover.md). Checks the backend's report,
// formats amounts and decides when the daily budget warning is due. Pure, so it is tested
// without React. Sessions of programs without readable usage simply have no entry.

import type { Translate } from "./i18n";
import { compactTokens } from "./contextUsage";

export interface TokenCounts {
  input: number;
  output: number;
  cacheWrite5m: number;
  cacheWrite1h: number;
  cacheRead: number;
}

/** Rust `CostAmount`. */
export interface CostAmount {
  tokens: TokenCounts;
  /** All tokens together. */
  total: number;
  /** Estimated US dollars of the priced responses; `null` when none has a price. */
  costUsd: number | null;
  /** Some responses have no price: the cost is less than the whole. */
  partial: boolean;
}

/** One running session's conversation so far (Rust `SessionCost`). */
export interface SessionCost extends CostAmount {
  sessionId: number;
}

/** One conversation's share of a day (Rust `DaySession`). */
export interface DaySession extends CostAmount {
  /** The running session, `null` once it was closed. */
  sessionId: number | null;
  title: string;
  program: string;
}

export interface DayCost extends CostAmount {
  /** Local day, "YYYY-MM-DD". */
  day: string;
  /** Most expensive first. */
  sessions: DaySession[];
}

export interface CostReport {
  today: string;
  /** Running sessions with known usage. */
  sessions: ReadonlyMap<number, SessionCost>;
  /** Today first, then the days before it. */
  days: DayCost[];
  priceSource: string;
  priceDate: string;
}

const DAY = /^\d{4}-\d{2}-\d{2}$/;
/** Longest budget accepted, in dollars. */
export const MAX_BUDGET_USD = 100_000;

function count(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

function text(value: unknown, limit = 200): string {
  return typeof value === "string" ? value.slice(0, limit) : "";
}

function amount(value: object): CostAmount | null {
  const tokens: unknown = Reflect.get(value, "tokens");
  const total = count(Reflect.get(value, "total"));
  const cost: unknown = Reflect.get(value, "costUsd");
  if (typeof tokens !== "object" || tokens === null || total === null) return null;
  const field = (name: keyof TokenCounts) => count(Reflect.get(tokens, name)) ?? 0;
  const costUsd = typeof cost === "number" && Number.isFinite(cost) && cost >= 0 ? cost : null;
  return {
    tokens: {
      input: field("input"),
      output: field("output"),
      cacheWrite5m: field("cacheWrite5m"),
      cacheWrite1h: field("cacheWrite1h"),
      cacheRead: field("cacheRead"),
    },
    total,
    costUsd,
    partial: costUsd !== null && Reflect.get(value, "partial") === true,
  };
}

function record(value: unknown): object | null {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? value : null;
}

/** Checks the answer of `cost_report`; `null` if malformed. */
export function parseCostReport(value: unknown): CostReport | null {
  const raw = record(value);
  if (raw === null) return null;
  const today: unknown = Reflect.get(raw, "today");
  const sessionList: unknown = Reflect.get(raw, "sessions");
  const dayList: unknown = Reflect.get(raw, "days");
  if (typeof today !== "string" || !Array.isArray(sessionList) || !Array.isArray(dayList)) {
    return null;
  }
  const sessions = new Map<number, SessionCost>();
  for (const item of sessionList) {
    const entry = record(item);
    const id = entry === null ? null : count(Reflect.get(entry, "sessionId"));
    const parsed = entry === null ? null : amount(entry);
    if (id !== null && parsed !== null) sessions.set(id, { ...parsed, sessionId: id });
  }
  const days: DayCost[] = [];
  for (const item of dayList) {
    const entry = record(item);
    if (entry === null) continue;
    const day: unknown = Reflect.get(entry, "day");
    const whole = amount(entry);
    const list: unknown = Reflect.get(entry, "sessions");
    if (typeof day !== "string" || !DAY.test(day) || whole === null || !Array.isArray(list)) {
      continue;
    }
    const shares: DaySession[] = [];
    for (const share of list) {
      const shareEntry = record(share);
      const parsed = shareEntry === null ? null : amount(shareEntry);
      if (shareEntry === null || parsed === null) continue;
      shares.push({
        ...parsed,
        sessionId: count(Reflect.get(shareEntry, "sessionId")),
        title: text(Reflect.get(shareEntry, "title")),
        program: text(Reflect.get(shareEntry, "program")),
      });
    }
    days.push({ ...whole, day, sessions: shares });
  }
  return {
    today,
    sessions,
    days,
    priceSource: text(Reflect.get(raw, "priceSource"), 400),
    priceDate: text(Reflect.get(raw, "priceDate"), 40),
  };
}

/** "$0.42", "0,42 $": dollars in the language's format; tiny amounts as "<$0.01". */
export function formatUsd(value: number, language: string): string {
  const format = (amount: number) =>
    new Intl.NumberFormat(language, {
      style: "currency",
      currency: "USD",
      // Whole dollars from 100 on: cents no longer matter for an estimate.
      minimumFractionDigits: amount >= 100 ? 0 : 2,
      maximumFractionDigits: amount >= 100 ? 0 : 2,
    }).format(amount);
  if (value > 0 && value < 0.01) return `<${format(0.01)}`;
  return format(value);
}

/**
 * The cost as shown next to tokens: "~$0.42", with a trailing "+" when some responses have no
 * price; `null` when nothing has one.
 */
export function costText(amount: CostAmount, language: string): string | null {
  if (amount.costUsd === null) return null;
  return `~${formatUsd(amount.costUsd, language)}${amount.partial ? "+" : ""}`;
}

/** "Tokens 123k · ~$0.42" for the session peek; `null` without known usage. */
export function costNote(
  amount: CostAmount | undefined,
  language: string,
  t: Translate,
): string | null {
  if (amount === undefined || amount.total === 0) return null;
  const tokens = compactTokens(amount.total);
  const cost = costText(amount, language);
  return cost === null ? t("costs.peekTokens", { tokens }) : t("costs.peek", { tokens, cost });
}

/** The report's entry for today, if the backend listed it. */
export function todayCost(report: CostReport): DayCost | undefined {
  return report.days.find((day) => day.day === report.today);
}

/**
 * Today's tokens and cost of a link group (ADR 0014): the sum of its members' shares of today
 * (`report.days`, so a conversation that began yesterday counts only from midnight). Partial
 * when a member's share is, or when some members have a price and others have none. `null`
 * while no member has known usage today.
 */
export function groupCost(
  report: CostReport | null,
  members: readonly number[],
): CostAmount | null {
  const shares = (report === null ? undefined : todayCost(report))?.sessions.filter(
    (share) => share.sessionId !== null && members.includes(share.sessionId) && share.total > 0,
  );
  if (shares === undefined || shares.length === 0) return null;
  const sum = (pick: (share: CostAmount) => number) =>
    shares.reduce((total, share) => total + pick(share), 0);
  const priced = shares.filter((share) => share.costUsd !== null);
  return {
    tokens: {
      input: sum((share) => share.tokens.input),
      output: sum((share) => share.tokens.output),
      cacheWrite5m: sum((share) => share.tokens.cacheWrite5m),
      cacheWrite1h: sum((share) => share.tokens.cacheWrite1h),
      cacheRead: sum((share) => share.tokens.cacheRead),
    },
    total: sum((share) => share.total),
    costUsd: priced.length === 0 ? null : sum((share) => share.costUsd ?? 0),
    partial:
      priced.length > 0 && (priced.length < shares.length || shares.some((share) => share.partial)),
  };
}

/**
 * A stored budget: whole cents above zero up to MAX_BUDGET_USD, else none. Checked after
 * rounding, so the backend (`main_store.rs`) never gets a zero.
 */
export function parseBudget(value: unknown): number | null {
  if (typeof value !== "number" || !Number.isFinite(value)) return null;
  const dollars = Math.round(value * 100) / 100;
  return dollars <= 0 || dollars > MAX_BUDGET_USD ? null : dollars;
}

/**
 * Where an older version kept the day the budget notice last showed; the backend keeps it now
 * (`mainStore.ts`), and this key is only read once for the migration.
 */
export const BUDGET_WARNED_STORAGE_KEY = "hangar.costs.warnedDay";

/**
 * The day the budget notice last showed: strictly "YYYY-MM-DD" with a month from 01 to 12 and a
 * day from 01 to 31, as the backend checks it (`main_store.rs`); else none.
 */
export function parseWarnedDay(value: unknown): string | null {
  if (typeof value !== "string" || !DAY.test(value)) return null;
  const month = Number(value.slice(5, 7));
  const day = Number(value.slice(8, 10));
  return month >= 1 && month <= 12 && day >= 1 && day <= 31 ? value : null;
}

/** How far today's cost got: the early warning at a share of the budget, or the budget itself. */
export type BudgetLevel = "early" | "reached";

/** The day and level the budget notice last showed (backend `BudgetWarned`). */
export interface BudgetWarned {
  day: string;
  level: BudgetLevel;
}

/**
 * The stored day and level; a day alone (what an older version kept) is the day the budget was
 * reached, as that version only warned then. Anything else: none.
 */
export function parseBudgetWarned(value: unknown): BudgetWarned | null {
  if (typeof value === "string") {
    const day = parseWarnedDay(value);
    return day === null ? null : { day, level: "reached" };
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const day = parseWarnedDay(Reflect.get(value, "day"));
  const level: unknown = Reflect.get(value, "level");
  if (day === null || (level !== "early" && level !== "reached")) return null;
  return { day, level };
}

/** The early warning's choices in percent of the budget; `null` turns it off. */
export const EARLY_PERCENTS = [50, 70, 80, 90] as const;
/** The early warning before anything was chosen (backend `DEFAULT_EARLY_PERCENT`). */
export const DEFAULT_EARLY_PERCENT = 80;

/** A stored share for the early warning: a whole percent from 1 to 99, else off. */
export function parseEarlyPercent(value: unknown): number | null {
  return typeof value === "number" && Number.isInteger(value) && value >= 1 && value <= 99
    ? value
    : null;
}

/** The budget field's text ("12.5", "12,5") as dollars; empty means no budget. */
export function budgetFromInput(input: string): number | null {
  const cleaned = input.trim().replace(/^\$/, "").replace(",", ".");
  if (cleaned === "" || !/^\d+(\.\d*)?$/.test(cleaned)) return null;
  return parseBudget(Number(cleaned));
}

export interface BudgetCheck {
  report: CostReport | null;
  /** Dollars per day; `null`: no budget. */
  budget: number | null;
  /** The early warning's share of the budget in percent; `null`: only the budget warns. */
  earlyPercent: number | null;
  /** The day and level a warning last showed, so each level shows once per day. */
  warned: BudgetWarned | null;
  /** Quiet hours right now: the warning waits until they end. */
  quiet: boolean;
}

export interface BudgetDue {
  level: BudgetLevel;
  /** Today's estimated cost in dollars. */
  cost: number;
}

/**
 * The warning that is due now: "reached" once today's cost reached the budget, "early" once it
 * reached the early share, each once a day. A level shown today silences itself and the lower
 * one; after quiet hours in which the cost passed both, only "reached" shows.
 */
export function budgetWarning(check: BudgetCheck): BudgetDue | null {
  const { report, budget } = check;
  if (report === null || budget === null || check.quiet) return null;
  const cost = todayCost(report)?.costUsd ?? null;
  if (cost === null) return null;
  const early = check.earlyPercent === null ? null : (budget * check.earlyPercent) / 100;
  const level: BudgetLevel | null =
    cost >= budget ? "reached" : early !== null && cost >= early ? "early" : null;
  if (level === null) return null;
  const shown = check.warned?.day === report.today ? check.warned.level : null;
  if (shown === "reached" || shown === level) return null;
  return { level, cost };
}

/** Local midnight of "YYYY-MM-DD"; `null` if malformed. */
export function dayDate(day: string): Date | null {
  if (!DAY.test(day)) return null;
  const [year, month, date] = day.split("-").map(Number);
  if (year === undefined || month === undefined || date === undefined) return null;
  const result = new Date(year, month - 1, date);
  return result.getMonth() === month - 1 ? result : null;
}

/** "Today", "Yesterday", else the weekday and date in the language's format. */
export function dayLabel(day: string, today: string, language: string, t: Translate): string {
  if (day === today) return t("costs.today");
  const date = dayDate(day);
  const now = dayDate(today);
  if (date === null || now === null) return day;
  const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1);
  if (date.getTime() === yesterday.getTime()) return t("costs.yesterday");
  return new Intl.DateTimeFormat(language, {
    weekday: "short",
    day: "numeric",
    month: "short",
  }).format(date);
}

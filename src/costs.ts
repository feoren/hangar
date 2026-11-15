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

/** A stored budget: dollars above zero up to MAX_BUDGET_USD, else none. */
export function parseBudget(value: unknown): number | null {
  if (typeof value !== "number" || !Number.isFinite(value)) return null;
  if (value <= 0 || value > MAX_BUDGET_USD) return null;
  return Math.round(value * 100) / 100;
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
  /** The last day a warning showed ("YYYY-MM-DD"), so it shows once per day. */
  warnedDay: string | null;
  /** Quiet hours right now: the warning waits until they end. */
  quiet: boolean;
}

/** Today's cost once it reached the budget and the warning is due; `null` otherwise. */
export function budgetWarning(check: BudgetCheck): number | null {
  const { report, budget } = check;
  if (report === null || budget === null || check.quiet) return null;
  if (check.warnedDay === report.today) return null;
  const cost = todayCost(report)?.costUsd ?? null;
  return cost !== null && cost >= budget ? cost : null;
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

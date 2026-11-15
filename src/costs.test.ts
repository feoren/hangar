import { describe, expect, it } from "vitest";

import {
  budgetFromInput,
  budgetWarning,
  costNote,
  costText,
  dayLabel,
  formatUsd,
  parseBudget,
  parseCostReport,
  todayCost,
  type CostReport,
} from "./costs";
import { createTranslator } from "./i18n";

const t = createTranslator("en");
const tokens = { input: 1, output: 2, cacheWrite5m: 3, cacheWrite1h: 4, cacheRead: 5 };

function payload(todayCostUsd: number | null) {
  return {
    today: "2026-11-13",
    sessions: [
      { sessionId: 3, tokens, total: 123_400, costUsd: 0.42, partial: false },
      { sessionId: 4, tokens, total: 900, costUsd: null, partial: false },
      { sessionId: "x", tokens, total: 1, costUsd: 1 },
    ],
    days: [
      {
        day: "2026-11-13",
        tokens,
        total: 124_300,
        costUsd: todayCostUsd,
        partial: true,
        sessions: [
          {
            sessionId: 3,
            title: "Claude Code",
            program: "Claude Code",
            tokens,
            total: 123_400,
            costUsd: 0.42,
            partial: false,
          },
          {
            sessionId: null,
            title: "Old",
            program: "Claude Code",
            tokens,
            total: 900,
            costUsd: null,
          },
        ],
      },
      { day: "2026-11-12", tokens, total: 0, costUsd: null, partial: false, sessions: [] },
      { day: "bad", tokens, total: 0, sessions: [] },
    ],
    priceSource: "docs",
    priceDate: "2026-11-03",
  };
}

function report(todayCostUsd: number | null): CostReport {
  const parsed = parseCostReport(payload(todayCostUsd));
  if (parsed === null) throw new Error("report did not parse");
  return parsed;
}

describe("parseCostReport", () => {
  it("keeps valid entries and drops broken ones", () => {
    const parsed = report(5);
    expect([...parsed.sessions.keys()]).toEqual([3, 4]);
    expect(parsed.sessions.get(3)?.costUsd).toBe(0.42);
    expect(parsed.sessions.get(4)?.costUsd).toBeNull();
    expect(parsed.days.map((day) => day.day)).toEqual(["2026-11-13", "2026-11-12"]);
    expect(parsed.days[0]?.sessions[1]?.sessionId).toBeNull();
    expect(parsed.days[0]?.partial).toBe(true);
    expect(todayCost(parsed)?.total).toBe(124_300);
    expect(parsed.priceDate).toBe("2026-11-03");
  });

  it("refuses what is no report", () => {
    expect(parseCostReport(null)).toBeNull();
    expect(parseCostReport([])).toBeNull();
    expect(parseCostReport({ today: 1, sessions: [], days: [] })).toBeNull();
  });
});

describe("cost texts", () => {
  it("formats dollars, tiny amounts and partial costs", () => {
    expect(formatUsd(0.42, "en")).toBe("$0.42");
    expect(formatUsd(0, "en")).toBe("$0.00");
    expect(formatUsd(0.001, "en")).toBe("<$0.01");
    expect(formatUsd(1234.5, "en")).toBe("$1,235");
    const amount = { tokens, total: 10, costUsd: 0.42, partial: true };
    expect(costText(amount, "en")).toBe("~$0.42+");
    expect(costText({ ...amount, costUsd: null }, "en")).toBeNull();
  });

  it("writes the peek line with or without a price", () => {
    const parsed = report(5);
    expect(costNote(parsed.sessions.get(3), "en", t)).toBe("Tokens 123.4k · ~$0.42");
    expect(costNote(parsed.sessions.get(4), "en", t)).toBe("Tokens 900");
    expect(costNote(undefined, "en", t)).toBeNull();
  });

  it("names today and yesterday, other days by date", () => {
    expect(dayLabel("2026-11-13", "2026-11-13", "en", t)).toBe("Today");
    expect(dayLabel("2026-11-12", "2026-11-13", "en", t)).toBe("Yesterday");
    expect(dayLabel("2026-03-01", "2026-03-02", "en", t)).toBe("Yesterday");
    expect(dayLabel("2026-11-10", "2026-11-13", "en", t)).toMatch(/10/);
    expect(dayLabel("nonsense", "2026-11-13", "en", t)).toBe("nonsense");
  });
});

describe("daily budget", () => {
  it("reads stored and typed budgets", () => {
    expect(parseBudget(12.345)).toBe(12.35);
    expect(parseBudget(0)).toBeNull();
    expect(parseBudget(-1)).toBeNull();
    expect(parseBudget("5")).toBeNull();
    expect(parseBudget(Number.NaN)).toBeNull();
    expect(budgetFromInput(" 12,5 ")).toBe(12.5);
    expect(budgetFromInput("$7")).toBe(7);
    expect(budgetFromInput("")).toBeNull();
    expect(budgetFromInput("abc")).toBeNull();
  });

  it("warns once a day when the budget is reached, never in quiet hours", () => {
    const base = { budget: 5, warnedDay: null, quiet: false };
    expect(budgetWarning({ ...base, report: report(5) })).toBe(5);
    expect(budgetWarning({ ...base, report: report(4.99) })).toBeNull();
    expect(budgetWarning({ ...base, report: report(null) })).toBeNull();
    expect(budgetWarning({ ...base, report: report(9), quiet: true })).toBeNull();
    expect(budgetWarning({ ...base, report: report(9), warnedDay: "2026-11-13" })).toBeNull();
    expect(budgetWarning({ ...base, report: report(9), warnedDay: "2026-11-12" })).toBe(9);
    expect(budgetWarning({ ...base, report: report(9), budget: null })).toBeNull();
    expect(budgetWarning({ ...base, report: null })).toBeNull();
  });
});

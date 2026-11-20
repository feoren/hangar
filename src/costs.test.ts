import { describe, expect, it } from "vitest";

import {
  budgetFromInput,
  budgetWarning,
  costNote,
  costText,
  dayLabel,
  formatUsd,
  groupCost,
  MAX_BUDGET_USD,
  parseBudget,
  parseBudgetWarned,
  parseCostReport,
  parseEarlyPercent,
  parseWarnedDay,
  todayCost,
  type BudgetCheck,
  type CostReport,
} from "./costs";
import { isQuietTime } from "./quietHours";
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
    // Checked after rounding: the backend never gets a zero or more than the maximum.
    expect(parseBudget(0.004)).toBeNull();
    expect(parseBudget(MAX_BUDGET_USD)).toBe(MAX_BUDGET_USD);
    expect(parseBudget(MAX_BUDGET_USD + 0.01)).toBeNull();
    expect(budgetFromInput(" 12,5 ")).toBe(12.5);
    expect(budgetFromInput("$7")).toBe(7);
    expect(budgetFromInput("")).toBeNull();
    expect(budgetFromInput("abc")).toBeNull();
  });

  it("reads the warned day strictly as a calendar day", () => {
    expect(parseWarnedDay("2026-11-14")).toBe("2026-11-14");
    expect(parseWarnedDay("9999-12-31")).toBe("9999-12-31");
    for (const day of [
      null,
      14,
      "",
      "2026-11-1",
      "2026-11-145",
      "2026/11/14",
      "2026-00-14",
      "2026-13-14",
      "2026-11-00",
      "2026-11-32",
      "２０２６-11-14",
      "2026-11-14\n",
      " 2026-11-14",
    ]) {
      expect(parseWarnedDay(day), String(day)).toBeNull();
    }
  });

  it("warns once a day when the budget is reached, never in quiet hours", () => {
    const base: Omit<BudgetCheck, "report"> = {
      budget: 5,
      earlyPercent: null,
      warned: null,
      quiet: false,
    };
    const reached = (cost: number) => ({ level: "reached", cost });
    expect(budgetWarning({ ...base, report: report(5) })).toEqual(reached(5));
    expect(budgetWarning({ ...base, report: report(4.99) })).toBeNull();
    expect(budgetWarning({ ...base, report: report(null) })).toBeNull();
    expect(budgetWarning({ ...base, report: report(9), quiet: true })).toBeNull();
    const today = { day: "2026-11-13", level: "reached" } as const;
    expect(budgetWarning({ ...base, report: report(9), warned: today })).toBeNull();
    const before = { day: "2026-11-12", level: "reached" } as const;
    expect(budgetWarning({ ...base, report: report(9), warned: before })).toEqual(reached(9));
    expect(budgetWarning({ ...base, report: report(9), budget: null })).toBeNull();
    expect(budgetWarning({ ...base, report: null })).toBeNull();
  });

  it("warns early once, then once more at the budget, each level once a day", () => {
    const base: Omit<BudgetCheck, "report"> = {
      budget: 10,
      earlyPercent: 80,
      warned: null,
      quiet: false,
    };
    expect(budgetWarning({ ...base, report: report(7.99) })).toBeNull();
    expect(budgetWarning({ ...base, report: report(8) })).toEqual({ level: "early", cost: 8 });
    const early = { day: "2026-11-13", level: "early" } as const;
    expect(budgetWarning({ ...base, report: report(9), warned: early })).toBeNull();
    expect(budgetWarning({ ...base, report: report(10), warned: early })).toEqual({
      level: "reached",
      cost: 10,
    });
    const reached = { day: "2026-11-13", level: "reached" } as const;
    expect(budgetWarning({ ...base, report: report(12), warned: reached })).toBeNull();
    // Off: only the budget warns.
    expect(budgetWarning({ ...base, earlyPercent: null, report: report(9) })).toBeNull();
  });

  it("starts both levels again on a new day", () => {
    const base: Omit<BudgetCheck, "report"> = {
      budget: 10,
      earlyPercent: 80,
      // Yesterday ended past the budget.
      warned: { day: "2026-11-12", level: "reached" },
      quiet: false,
    };
    expect(budgetWarning({ ...base, report: report(8.5) })).toEqual({ level: "early", cost: 8.5 });
    expect(budgetWarning({ ...base, report: report(11) })).toEqual({ level: "reached", cost: 11 });
  });

  it("waits through quiet hours and then shows only the highest level reached", () => {
    const hours = { enabled: true, from: 22 * 60, to: 7 * 60 };
    const base: Omit<BudgetCheck, "report" | "quiet"> = {
      budget: 10,
      earlyPercent: 80,
      warned: null,
    };
    const at = (hour: number) => isQuietTime(hours, new Date(2026, 10, 13, hour, 30));
    // 23:30: past the budget, but quiet.
    expect(budgetWarning({ ...base, report: report(12), quiet: at(23) })).toBeNull();
    // 06:30, still quiet.
    expect(budgetWarning({ ...base, report: report(12), quiet: at(6) })).toBeNull();
    // 07:30: the early level was passed in silence; only "reached" shows, once.
    expect(budgetWarning({ ...base, report: report(12), quiet: at(7) })).toEqual({
      level: "reached",
      cost: 12,
    });
  });

  it("reads the warned day and level, an old day alone as reached", () => {
    expect(parseBudgetWarned({ day: "2026-11-14", level: "early" })).toEqual({
      day: "2026-11-14",
      level: "early",
    });
    expect(parseBudgetWarned("2026-11-14")).toEqual({ day: "2026-11-14", level: "reached" });
    for (const value of [
      null,
      "2026-13-01",
      { day: "2026-11-14" },
      { day: "2026-11-14", level: "over" },
      { day: "x", level: "early" },
      ["2026-11-14"],
      7,
    ]) {
      expect(parseBudgetWarned(value), JSON.stringify(value)).toBeNull();
    }
  });

  it("takes an early share from 1 to 99 percent", () => {
    expect(parseEarlyPercent(80)).toBe(80);
    expect(parseEarlyPercent(1)).toBe(1);
    expect(parseEarlyPercent(99)).toBe(99);
    for (const value of [0, 100, 80.5, "80", null, Number.NaN]) {
      expect(parseEarlyPercent(value), String(value)).toBeNull();
    }
  });
});

describe("groupCost", () => {
  function shares(
    list: { id: number | null; total: number; cost: number | null; partial?: boolean }[],
  ) {
    const parsed = parseCostReport({
      today: "2026-11-13",
      sessions: [],
      days: [
        {
          day: "2026-11-13",
          tokens,
          total: 0,
          costUsd: null,
          partial: false,
          sessions: list.map((entry) => ({
            sessionId: entry.id,
            title: "t",
            program: "Claude Code",
            tokens,
            total: entry.total,
            costUsd: entry.cost,
            partial: entry.partial === true,
          })),
        },
        // Yesterday's share of member 1 does not count.
        {
          day: "2026-11-12",
          tokens,
          total: 5000,
          costUsd: 9,
          partial: false,
          sessions: [{ sessionId: 1, title: "t", program: "p", tokens, total: 5000, costUsd: 9 }],
        },
      ],
    });
    if (parsed === null) throw new Error("report did not parse");
    return parsed;
  }

  it("sums today's shares of the members only", () => {
    const value = shares([
      { id: 1, total: 1000, cost: 0.5 },
      { id: 2, total: 3000, cost: 0.25 },
      { id: 3, total: 9000, cost: 4 },
      { id: null, total: 9000, cost: 4 },
    ]);
    const sum = groupCost(value, [1, 2]);
    expect(sum?.total).toBe(4000);
    expect(sum?.costUsd).toBeCloseTo(0.75);
    expect(sum?.partial).toBe(false);
    expect(sum?.tokens.input).toBe(2);
    expect(sum === null ? null : costText(sum, "en")).toBe("~$0.75");
  });

  it("marks the sum partial when a member has no price or a partial one", () => {
    const mixed = shares([
      { id: 1, total: 1000, cost: 0.5 },
      { id: 2, total: 3000, cost: null },
    ]);
    expect(groupCost(mixed, [1, 2])?.partial).toBe(true);
    const cut = shares([{ id: 1, total: 1000, cost: 0.5, partial: true }]);
    expect(groupCost(cut, [1, 2])?.partial).toBe(true);
    const unpriced = shares([{ id: 2, total: 3000, cost: null }]);
    expect(groupCost(unpriced, [2])).toMatchObject({ costUsd: null, partial: false, total: 3000 });
  });

  it("has nothing without usage of a member today", () => {
    expect(groupCost(null, [1])).toBeNull();
    expect(groupCost(shares([{ id: 1, total: 0, cost: null }]), [1])).toBeNull();
    expect(groupCost(shares([{ id: 3, total: 10, cost: 1 }]), [1, 2])).toBeNull();
  });
});

import { describe, expect, it, vi } from "vitest";

import { badgeKind, badgeLabel, badgeText, TaskbarBadge } from "./taskbarBadge";

describe("badgeKind", () => {
  it("uses the overlay icon on Windows and the badge count elsewhere", () => {
    expect(badgeKind("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Edg/140")).toBe("overlay");
    expect(badgeKind("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit")).toBe("count");
    expect(badgeKind("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit")).toBe("count");
  });
});

describe("badgeLabel", () => {
  it("shows 1 to 9, then 9+", () => {
    expect(badgeLabel(0)).toBeNull();
    expect(badgeLabel(-1)).toBeNull();
    expect(badgeLabel(1)).toBe("1");
    expect(badgeLabel(9)).toBe("9");
    expect(badgeLabel(10)).toBe("9+");
  });

  it("keeps the real count for a system badge", () => {
    expect(badgeText(12, "count")).toBe("12");
    expect(badgeText(12, "overlay")).toBe("9+");
    expect(badgeText(0, "count")).toBeNull();
  });
});

describe("TaskbarBadge", () => {
  const color = "#f2e3c6";

  it("sends only changes of what is shown", () => {
    const apply = vi.fn(() => Promise.resolve());
    const badge = new TaskbarBadge("overlay", apply);
    badge.update(0, color);
    badge.update(2, color);
    badge.update(2, color);
    badge.update(11, color);
    badge.update(14, color);
    badge.update(0, color);
    expect(apply.mock.calls).toEqual([
      [2, "2", color],
      [11, "9+", color],
      [0, null, color],
    ]);
  });

  it("draws again in a new color, but does not clear twice", () => {
    const apply = vi.fn(() => Promise.resolve());
    const badge = new TaskbarBadge("overlay", apply);
    badge.update(0, color);
    badge.update(0, "#8cc8ff");
    badge.update(3, color);
    badge.update(3, "#8cc8ff");
    expect(apply.mock.calls).toEqual([
      [3, "3", color],
      [3, "3", "#8cc8ff"],
    ]);
  });

  it("clears a shown badge on quit, and nothing when none is shown", async () => {
    const apply = vi.fn(() => Promise.resolve());
    const badge = new TaskbarBadge("count", apply);
    await badge.clear();
    expect(apply).not.toHaveBeenCalled();
    badge.update(4, color);
    await badge.clear();
    await badge.clear();
    expect(apply.mock.calls).toEqual([
      [4, "4", color],
      [0, null, color],
    ]);
  });

  it("does not let a failing clear block quitting", async () => {
    const apply = vi.fn(() => Promise.reject(new Error("no window")));
    const badge = new TaskbarBadge("overlay", apply);
    badge.update(2, color);
    await expect(badge.clear()).resolves.toBeUndefined();
  });

  it("swallows errors and tries again on the next change", async () => {
    const apply = vi.fn(() => Promise.reject(new Error("no window")));
    const badge = new TaskbarBadge("count", apply);
    badge.update(1, color);
    await Promise.resolve();
    await Promise.resolve();
    badge.update(1, color);
    expect(apply).toHaveBeenCalledTimes(2);
  });
});

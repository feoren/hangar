import { describe, expect, it } from "vitest";

import { isDue, PROFILES_REFRESH_MIN_MS } from "./timing";

describe("isDue", () => {
  it("is due the first time and after the interval", () => {
    expect(isDue(null, 0, PROFILES_REFRESH_MIN_MS)).toBe(true);
    expect(isDue(0, 29_999, PROFILES_REFRESH_MIN_MS)).toBe(false);
    expect(isDue(0, 30_000, PROFILES_REFRESH_MIN_MS)).toBe(true);
  });
});

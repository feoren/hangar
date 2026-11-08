import { describe, expect, it } from "vitest";

import { safeLinkUrl, sanitizePaste } from "./termSafety";

describe("sanitizePaste", () => {
  it("keeps normal text, tabs and line breaks", () => {
    expect(sanitizePaste("echo hello\tworld\r\nsecond line\näöü 😀")).toBe(
      "echo hello\tworld\r\nsecond line\näöü 😀",
    );
  });

  it("removes the bracketed paste end so nothing slips through as typed input", () => {
    expect(sanitizePaste("harmless\x1b[201~rm -rf ~\r")).toBe("harmless[201~rm -rf ~\r");
    expect(sanitizePaste("\x1b[200~text\x1b[201~")).toBe("[200~text[201~");
  });

  it("removes other C0 characters, DEL and C1 characters", () => {
    expect(sanitizePaste("a\x00b\x03c\x07d\x08e\x7ff")).toBe("abcdef");
    expect(sanitizePaste("x\u009b31my")).toBe("x31my");
    expect(sanitizePaste("\x0b\x0c")).toBe("");
  });
});

describe("safeLinkUrl", () => {
  it("lets http and https through", () => {
    expect(safeLinkUrl("https://example.com/a?b=1")).toBe("https://example.com/a?b=1");
    expect(safeLinkUrl(" http://localhost:3000 ")).toBe("http://localhost:3000/");
  });

  it("rejects other schemes and nonsense", () => {
    expect(safeLinkUrl("javascript:alert(1)")).toBeNull();
    expect(safeLinkUrl("file:///C:/Windows/system32/calc.exe")).toBeNull();
    expect(safeLinkUrl("vscode://file/x")).toBeNull();
    expect(safeLinkUrl("data:text/html,<b>x</b>")).toBeNull();
    expect(safeLinkUrl("not a link")).toBeNull();
    expect(safeLinkUrl("")).toBeNull();
  });
});

import { describe, expect, it } from "vitest";

import { osc8Decision, safeLinkUrl, sanitizePaste } from "./termSafety";

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

describe("osc8Decision", () => {
  it("asks when the shown address names another host than the target", () => {
    expect(osc8Decision("https://github.com/org/repo", "https://example.net/org/repo")).toBe(
      "confirm",
    );
    expect(osc8Decision("github.com/org/repo", "https://github.com.example.net/")).toBe("confirm");
    expect(osc8Decision("www.example.com", "https://example.org/")).toBe("confirm");
  });

  it("opens when the shown address has the target's host, even with a deeper path", () => {
    expect(osc8Decision("https://github.com/org/repo", "https://github.com/org/repo/pull/7")).toBe(
      "open",
    );
    expect(osc8Decision("GitHub.com/org", "https://github.com/org/repo")).toBe("open");
    expect(osc8Decision("http://localhost:3000", "http://localhost:3000/marker")).toBe("open");
  });

  it("opens plain words, which claim no address", () => {
    expect(osc8Decision("Open the pull request", "https://example.com/pr/1")).toBe("open");
    expect(osc8Decision("", "https://example.com/")).toBe("open");
  });

  it("opens versions and bare file names, which name no host", () => {
    const target = "https://github.com/org/repo/blob/main/README.md";
    expect(osc8Decision("README.md", target)).toBe("open");
    expect(osc8Decision("app.test.ts", target)).toBe("open");
    expect(osc8Decision("v0.8.0", "https://github.com/org/repo/releases")).toBe("open");
    expect(osc8Decision("1.2.3", "https://example.com/")).toBe("open");
  });

  it("still asks for a file-like name with a path, an address or a port", () => {
    expect(osc8Decision("example.md/org", "https://example.net/")).toBe("confirm");
    expect(osc8Decision("10.0.0.1", "https://example.net/")).toBe("confirm");
    expect(osc8Decision("localhost:3000", "https://example.net/")).toBe("confirm");
    expect(osc8Decision("example.io", "https://example.net/")).toBe("confirm");
  });

  it("asks when the shown text looks like a path", () => {
    expect(osc8Decision("/tmp/marker.txt", "https://example.com/")).toBe("confirm");
    expect(osc8Decision("C:\\Users\\marker.txt", "https://example.com/")).toBe("confirm");
    expect(osc8Decision("./src/app.ts", "https://example.com/")).toBe("confirm");
    expect(osc8Decision("~/notes.md", "https://example.com/")).toBe("confirm");
    expect(osc8Decision("file:///C:/marker.txt", "https://example.com/")).toBe("confirm");
  });
});

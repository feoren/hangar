import { describe, expect, it } from "vitest";

import { addressParts, osc8Decision, safeLinkUrl, sanitizePaste } from "./termSafety";

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

  it("rejects addresses with a user name or password, which hide the real host", () => {
    expect(safeLinkUrl("https://github.com@evil.example/")).toBeNull();
    expect(safeLinkUrl("https://user:secret@example.com/")).toBeNull();
    expect(safeLinkUrl("https://:secret@example.com/")).toBeNull();
  });
});

describe("addressParts", () => {
  it("cuts an address around its host", () => {
    expect(addressParts("https://github.com/org/repo")).toEqual({
      before: "https://",
      host: "github.com",
      after: "/org/repo",
    });
    expect(addressParts("http://localhost:3000/")).toEqual({
      before: "http://",
      host: "localhost",
      after: ":3000/",
    });
    expect(addressParts("http://[::1]:8080/x")).toEqual({
      before: "http://",
      host: "[::1]",
      after: ":8080/x",
    });
  });

  it("keeps anything without a findable host whole", () => {
    expect(addressParts("not a link")).toEqual({ before: "not a link", host: "", after: "" });
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

  it("opens versions and a file name that is the target's file", () => {
    const target = "https://github.com/org/repo/blob/main/README.md";
    expect(osc8Decision("README.md", target)).toBe("open");
    expect(osc8Decision("readme.md", target)).toBe("open");
    expect(osc8Decision("Read README.md", target)).toBe("open");
    expect(osc8Decision("v0.8.0", "https://github.com/org/repo/releases")).toBe("open");
    expect(osc8Decision("1.2.3", "https://example.com/")).toBe("open");
  });

  it("asks when a file name leads to anything but that file", () => {
    const target = "https://evil.example/x";
    expect(osc8Decision("README.md", target)).toBe("confirm");
    expect(osc8Decision("install.sh", target)).toBe("confirm");
    expect(osc8Decision("update.zip", target)).toBe("confirm");
    expect(osc8Decision("app.test.ts", "https://github.com/org/repo/blob/main/README.md")).toBe(
      "confirm",
    );
    expect(osc8Decision("Get update.zip now", "https://example.com/setup.exe")).toBe("confirm");
  });

  it("asks when a word around the link claims another host", () => {
    const target = "https://evil.example/x";
    for (const shown of [
      "github.com.",
      "github.com,",
      "(github.com)",
      '"github.com/acme"',
      "see github.com/acme",
      "Open https://github.com/acme",
      "github .com",
      "github . com/acme",
      "https://github.com/acme",
      "GITHUB.COM",
      "github.com:8443/x",
      "[::1]:3000",
      "https://git",
      "\uff47\uff49\uff54\uff48\uff55\uff42.\uff43\uff4f\uff4d",
    ]) {
      expect(osc8Decision(shown, target), shown).toBe("confirm");
    }
  });

  it("opens words and sentences that claim no host, even with punctuation", () => {
    const target = "https://example.com/pr/1";
    expect(osc8Decision("Open the pull request", target)).toBe("open");
    expect(osc8Decision("(details)", target)).toBe("open");
    expect(osc8Decision("Done. See the log, e.g. here!", target)).toBe("open");
    expect(osc8Decision("see example.com/pr", target)).toBe("open");
    expect(osc8Decision("(https://example.com/pr/1).", target)).toBe("open");
  });

  it("asks when the shown text hides characters nobody sees", () => {
    const target = "https://evil.example/x";
    for (const shown of [
      "github\u200b.com/acme",
      "git\u00adhub.com",
      "github\u2060.com/acme",
      "\u202egithub.com",
      "github.com\u200e",
      "Open the\u200b pull request",
    ]) {
      expect(osc8Decision(shown, target), JSON.stringify(shown)).toBe("confirm");
    }
  });

  it("reads ideographic and fullwidth dots as the dot of a host name", () => {
    const target = "https://evil.example/x";
    expect(osc8Decision("github\u3002com/acme", target)).toBe("confirm");
    expect(osc8Decision("github\uff0ecom/acme", target)).toBe("confirm");
    expect(osc8Decision("github\uff61com/acme", target)).toBe("confirm");
    expect(osc8Decision("github\u3002com/acme", "https://github.com/acme")).toBe("open");
  });

  it("keeps emoji whole: their joiners and variation selectors hide nothing", () => {
    const target = "https://example.com/team";
    expect(osc8Decision("Team \u{1F469}\u200d\u{1F4BB}", target)).toBe("open");
    expect(osc8Decision("Love \u2764\ufe0f", target)).toBe("open");
  });

  it("asks when the shown address carries a user name", () => {
    expect(osc8Decision("https://github.com@evil.example", "https://evil.example/")).toBe(
      "confirm",
    );
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

describe("osc8Decision on common agent output", () => {
  const repo = "https://github.com/acme/app";
  it.each([
    ["PR #123", `${repo}/pull/123`],
    ["#42", `${repo}/issues/42`],
    ["docs", "https://docs.example.com/"],
    ["Docs:", "https://docs.example.com/guide"],
    ["github.com/acme/app", repo],
    ["github.com/acme/app", `${repo}/`],
    ["https://github.com/acme/app/pull/5", `${repo}/pull/5`],
    ["localhost:3000", "http://localhost:3000/"],
    ["http://localhost:5173", "http://localhost:5173/"],
    ["README.md", `${repo}/blob/main/README.md`],
    ["Cargo.toml", `${repo}/blob/main/src-tauri/Cargo.toml`],
    ["src/app.ts", `${repo}/blob/main/src/app.ts`],
    ["app.ts:42", `${repo}/blob/main/src/app.ts#L42`],
    ["app.ts:42:7", `${repo}/blob/main/src/app.ts#L42`],
    ["v0.8.0", `${repo}/releases/tag/v0.8.0`],
    ["Python 3.12", "https://docs.python.org/3.12/"],
    ["example.com", "https://www.example.com/"],
    ["www.example.com", "https://example.com/"],
    ["docs.rs/serde", "https://docs.rs/serde/latest/serde/"],
    ["e.g. the guide", "https://example.com/guide"],
    ["(see the changelog)", `${repo}/blob/main/CHANGELOG.md`],
    ["Team \u{1F469}‍\u{1F4BB}", "https://example.com/team"],
  ])("opens %j pointing to %s", (shown, target) => {
    expect(osc8Decision(shown, target)).toBe("open");
  });

  it.each([
    // A framework name reads like a file name: it asks once and shows the real address.
    ["Node.js", "https://nodejs.org/"],
    ["app.ts:42", "https://evil.example/app.ts.html"],
    ["www.example.com", "https://example.org/"],
    ["example.com", "https://www.example.com.evil.example/"],
    ["localhost:3000", "http://127.0.0.1:3000/"],
  ])("asks for %j pointing to %s", (shown, target) => {
    expect(osc8Decision(shown, target)).toBe("confirm");
  });
});

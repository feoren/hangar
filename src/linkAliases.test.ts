import { describe, expect, it } from "vitest";

import { ALIAS_MAX, aliasSlug, memberAliases } from "./linkAliases";
import { validMemberName } from "./links";

describe("aliasSlug", () => {
  it("makes lowercase ASCII with single dashes and spells out umlauts", () => {
    expect(aliasSlug("Hangar")).toBe("hangar");
    expect(aliasSlug("hangar.feoren.com")).toBe("hangar-feoren-com");
    expect(aliasSlug("  Größe  Übung ")).toBe("groesse-uebung");
    expect(aliasSlug("Café_Crème")).toBe("cafe-creme");
    expect(aliasSlug("Claude Code")).toBe("claude-code");
  });

  it("cuts long names at a word, never inside one", () => {
    const slug = aliasSlug("ich habe so viele Agenten nun installiert");
    expect(slug).toBe("ich-habe-so-viele");
    expect(slug.length).toBeLessThanOrEqual(ALIAS_MAX);
    expect(aliasSlug("customer-portal-backend-service")).toBe("customer-portal");
  });

  it("cuts a single word longer than the limit hard", () => {
    expect(aliasSlug("a".repeat(30))).toBe("a".repeat(ALIAS_MAX));
  });

  it("is empty without letters or digits", () => {
    expect(aliasSlug("!? #")).toBe("");
    expect(aliasSlug("修复")).toBe("");
  });
});

describe("memberAliases", () => {
  it("names each member after its project folder", () => {
    expect(
      memberAliases([
        { path: "E:\\Work\\hangar", program: "Claude Code" },
        { path: "E:\\Work\\hangar.feoren.com", program: "Codex CLI" },
        { path: "/home/me/hangar-server", program: "Claude Code" },
      ]),
    ).toEqual(["hangar", "hangar-feoren-com", "hangar-server"]);
  });

  it("takes the program where members share a folder, numbered where still taken", () => {
    expect(
      memberAliases([
        { path: "E:\\Work\\hangar", program: "Claude Code" },
        { path: "E:\\Work\\hangar\\", program: "Codex CLI" },
        { path: "E:\\Other\\hangar", program: "Claude Code" },
        { path: "E:\\Work\\web", program: "Gemini CLI" },
      ]),
    ).toEqual(["claude-code", "codex-cli", "claude-code-2", "web"]);
  });

  it("never gives the reserved names and always a valid one", () => {
    const names = memberAliases([
      { path: "C:\\x\\all", program: "Claude Code" },
      { path: "C:\\x\\lead", program: "Codex" },
      { path: "", program: "" },
      { path: "C:\\x\\修复", program: "修复" },
    ]);
    expect(names).toEqual(["all-2", "lead-2", "agent", "agent-2"]);
    for (const name of names) expect(validMemberName(name)).toBe(true);
  });
});

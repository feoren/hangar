import { describe, expect, it } from "vitest";

import { isProgramTitle, projectFolder, sessionLabels, type LabelInput } from "./sessionLabel";

describe("projectFolder", () => {
  it("takes the last folder of Windows, POSIX and WSL paths", () => {
    expect(projectFolder("E:\\Work\\hangar")).toBe("hangar");
    expect(projectFolder("/home/me/projects/site")).toBe("site");
    expect(projectFolder("\\\\wsl.localhost\\Ubuntu\\home\\me\\api")).toBe("api");
    expect(projectFolder("\\\\wsl$\\Ubuntu\\home\\me\\api")).toBe("api");
  });

  it("ignores separators at the end", () => {
    expect(projectFolder("E:\\Work\\hangar\\")).toBe("hangar");
    expect(projectFolder("/home/me/site//")).toBe("site");
  });

  it("keeps drive and file system roots as they are", () => {
    expect(projectFolder("C:\\")).toBe("C:\\");
    expect(projectFolder("/")).toBe("/");
  });

  it("is empty without a path", () => {
    expect(projectFolder("")).toBe("");
    expect(projectFolder("   ")).toBe("");
  });
});

describe("isProgramTitle", () => {
  it("accepts the program name and its numbered forms", () => {
    expect(isProgramTitle("Claude Code", ["Claude Code"])).toBe(true);
    expect(isProgramTitle("Claude Code (2)", ["Claude Code"])).toBe(true);
    expect(isProgramTitle(" claude code (12) ", ["Claude Code"])).toBe(true);
    expect(isProgramTitle("", ["Claude Code"])).toBe(true);
  });

  it("rejects names the person gave", () => {
    expect(isProgramTitle("Release prep", ["Claude Code"])).toBe(false);
    expect(isProgramTitle("Claude Code (fix)", ["Claude Code"])).toBe(false);
    expect(isProgramTitle("Claude Code 2", ["Claude Code"])).toBe(false);
    expect(isProgramTitle("Claude", ["Claude Code"])).toBe(false);
    expect(isProgramTitle("anything", [""])).toBe(false);
  });
});

const names: Record<string, string> = { claude: "Claude Code", codex: "Codex CLI" };
const nameOf = (id: string): string | undefined => names[id];

function session(id: number, patch: Partial<LabelInput> = {}): LabelInput {
  return { id, title: "Claude Code", cwd: "E:\\Work\\hangar", profileId: "claude", ...patch };
}

describe("sessionLabels", () => {
  it("leads with the folder and moves the program to the second line", () => {
    const labels = sessionLabels([session(1)], nameOf);
    expect(labels.get(1)).toEqual({ primary: "hangar", secondary: "Claude Code", byFolder: true });
  });

  it("drops the number when the folders differ", () => {
    const labels = sessionLabels(
      [session(1), session(2, { title: "Claude Code (2)", cwd: "/srv/site" })],
      nameOf,
    );
    expect(labels.get(2)).toEqual({ primary: "site", secondary: "Claude Code", byFolder: true });
  });

  it("keeps the numbered title for the same program in folders of the same name", () => {
    const labels = sessionLabels(
      [session(1), session(2, { title: "Claude Code (2)", cwd: "D:\\Other\\Hangar\\" })],
      nameOf,
    );
    expect(labels.get(1)?.secondary).toBe("Claude Code");
    expect(labels.get(2)?.secondary).toBe("Claude Code (2)");
  });

  it("names different programs in one folder by program only", () => {
    const labels = sessionLabels(
      [session(1), session(2, { profileId: "codex", title: "Codex CLI" })],
      nameOf,
    );
    expect(labels.get(1)?.secondary).toBe("Claude Code");
    expect(labels.get(2)?.secondary).toBe("Codex CLI");
  });

  it("keeps a name the person gave, with the folder below it", () => {
    const labels = sessionLabels([session(1, { title: "Release prep" })], nameOf);
    expect(labels.get(1)).toEqual({
      primary: "Release prep",
      secondary: "hangar",
      byFolder: false,
    });
  });

  it("treats a name that only repeats the folder like the program name", () => {
    const labels = sessionLabels(
      [session(1, { title: "hangar" }), session(2, { title: "Hangar", cwd: "D:\\Other\\hangar" })],
      nameOf,
    );
    expect(labels.get(1)).toEqual({ primary: "hangar", secondary: "Claude Code", byFolder: true });
    expect(labels.get(2)).toEqual({ primary: "hangar", secondary: "Claude Code", byFolder: true });
  });

  it("recognizes the profile id as a title of an unknown or restored program", () => {
    const labels = sessionLabels([session(1, { profileId: "gone", title: "gone" })], nameOf);
    expect(labels.get(1)).toEqual({ primary: "hangar", secondary: "gone", byFolder: true });
  });

  it("leaves update sessions and sessions without a folder by their title", () => {
    const labels = sessionLabels([session(1, { kind: "update" }), session(2, { cwd: "" })], nameOf);
    expect(labels.get(1)?.primary).toBe("Claude Code");
    expect(labels.get(2)?.primary).toBe("Claude Code");
  });
});

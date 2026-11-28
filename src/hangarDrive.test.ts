import { describe, expect, it } from "vitest";

import {
  backUpFolder,
  canBackUp,
  driveMarkKey,
  isLocalFolder,
  NO_DRIVE,
  parseDriveStatus,
  driveFolder,
} from "./hangarDrive";

describe("parseDriveStatus", () => {
  it("pairs each mark with the path it was asked for", () => {
    const view = parseDriveStatus(["C:\\a", "C:\\b", "C:\\c"], {
      installed: true,
      marks: ["paused", null, "active"],
    });
    expect(view.installed).toBe(true);
    expect([...view.marks]).toEqual([
      ["C:\\a", "paused"],
      ["C:\\c", "active"],
    ]);
  });

  it("drops states it does not know and treats a broken answer as no Hangar Drive", () => {
    expect(parseDriveStatus(["C:\\a"], { installed: true, marks: ["later"] }).marks.size).toBe(0);
    expect(parseDriveStatus(["C:\\a"], { installed: false, marks: ["active"] })).toBe(NO_DRIVE);
    expect(parseDriveStatus(["C:\\a"], null)).toBe(NO_DRIVE);
    expect(parseDriveStatus(["C:\\a"], { installed: true })).toBe(NO_DRIVE);
    expect(parseDriveStatus(["C:\\a"], "yes")).toBe(NO_DRIVE);
  });
});

describe("driveMarkKey", () => {
  it("names the state only where the folder does not simply sync", () => {
    expect(driveMarkKey("active")).toBe("drive.synced");
    expect(driveMarkKey("other")).toBe("drive.synced");
    expect(driveMarkKey("paused")).toBe("drive.paused");
    expect(driveMarkKey("stopped")).toBe("drive.stopped");
  });
});

describe("isLocalFolder", () => {
  it("takes local absolute folders only", () => {
    expect(isLocalFolder("C:\\Users\\Jörg Müller\\proj #1")).toBe(true);
    expect(isLocalFolder("C:/work")).toBe(true);
    expect(isLocalFolder("D:")).toBe(true);
    expect(isLocalFolder("/home/a/proj")).toBe(true);
    for (const path of [
      "",
      "proj",
      "C:proj",
      "\\\\server\\share",
      "//server/share",
      "\\\\?\\C:\\a",
      "\\\\.\\C:\\a",
      "C:\\a\\..\\b",
      "C:\\a\nb",
      "\\work",
    ]) {
      expect(isLocalFolder(path), path).toBe(false);
    }
  });

  it("refuses reserved device names and parts ending in a dot or space on Windows", () => {
    for (const path of [
      "C:\\CON",
      "C:\\a\\nul.txt",
      "C:\\a\\Aux.tar.gz",
      "C:\\a\\COM1\\b",
      "C:\\a\\lpt9",
      "C:\\a\\prn .txt",
      "C:\\a\\proj.",
      "C:\\a\\proj \\b",
    ]) {
      expect(isLocalFolder(path), path).toBe(false);
    }
    for (const path of ["C:\\a\\console", "C:\\a\\com10", "C:\\a\\.git", "C:\\a\\v1.2", "C:\\"]) {
      expect(isLocalFolder(path), path).toBe(true);
    }
    expect(isLocalFolder("/home/a/con")).toBe(true);
    expect(isLocalFolder("/home/a/proj.")).toBe(true);
  });
});

describe("driveFolder", () => {
  it("is the worktree's main folder, else the working folder", () => {
    expect(driveFolder({ cwd: "C:\\a\\proj" })).toBe("C:\\a\\proj");
    expect(
      driveFolder({
        cwd: "C:\\a\\proj\\.claude\\worktrees\\x",
        worktree: { path: "C:\\a\\proj\\.claude\\worktrees\\x", repoRoot: "C:\\a\\proj" },
      }),
    ).toBe("C:\\a\\proj");
  });

  it("is nothing for update sessions and network folders", () => {
    expect(driveFolder({ cwd: "C:\\Users\\a", kind: "update" })).toBeNull();
    expect(driveFolder({ cwd: "\\\\server\\share\\proj" })).toBeNull();
    expect(driveFolder({ cwd: "" })).toBeNull();
  });
});

describe("canBackUp", () => {
  const installed = { installed: true, marks: new Map([["C:\\a\\proj", "active" as const]]) };

  it("offers only an installed Hangar Drive a folder it does not hold yet", () => {
    expect(canBackUp(installed, "C:\\a\\other")).toBe(true);
    expect(canBackUp(installed, "C:\\a\\proj")).toBe(false);
    expect(canBackUp(installed, null)).toBe(false);
    expect(canBackUp(NO_DRIVE, "C:\\a\\other")).toBe(false);
  });
});

describe("backUpFolder", () => {
  it("hands over the repository's main folder for a session in a subfolder", async () => {
    const asked: string[] = [];
    const folder = await backUpFolder({ cwd: "C:\\a\\proj\\src" }, (path) => {
      asked.push(path);
      return Promise.resolve("C:\\a\\proj");
    });
    expect(folder).toBe("C:\\a\\proj");
    expect(asked).toEqual(["C:\\a\\proj\\src"]);
  });

  it("keeps the session's folder outside a repository or when git fails", async () => {
    expect(await backUpFolder({ cwd: "C:\\a\\plain" }, () => Promise.resolve(null))).toBe(
      "C:\\a\\plain",
    );
    expect(
      await backUpFolder({ cwd: "C:\\a\\plain" }, () => Promise.reject(new Error("no git"))),
    ).toBe("C:\\a\\plain");
  });

  it("asks git nothing for a worktree or an update session", async () => {
    const never = () => Promise.reject(new Error("asked"));
    expect(
      await backUpFolder(
        { cwd: "C:\\a\\wt", worktree: { path: "C:\\a\\wt", repoRoot: "C:\\a\\proj" } },
        never,
      ),
    ).toBe("C:\\a\\proj");
    expect(await backUpFolder({ cwd: "C:\\Users\\a", kind: "update" }, never)).toBeNull();
  });
});

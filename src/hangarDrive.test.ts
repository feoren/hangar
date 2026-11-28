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
  it("pairs each mark and refusal with the path it was asked for", () => {
    const view = parseDriveStatus(["C:\\a", "C:\\b", "C:\\c"], {
      ready: true,
      home: "C:\\Users\\a",
      marks: ["paused", null, "active"],
      refused: [false, true, false],
    });
    expect(view.ready).toBe(true);
    expect(view.home).toBe("C:\\Users\\a");
    expect([...view.marks]).toEqual([
      ["C:\\a", "paused"],
      ["C:\\c", "active"],
    ]);
    expect([...view.refused]).toEqual(["C:\\b"]);
  });

  it("counts a missing refusal as refused", () => {
    const view = parseDriveStatus(["C:\\a", "C:\\b"], {
      ready: true,
      home: null,
      marks: [],
      refused: [false],
    });
    expect([...view.refused]).toEqual(["C:\\b"]);
    expect(view.home).toBeNull();
  });

  it("drops states it does not know and treats a broken answer as no Hangar Drive", () => {
    const answer = { ready: true, home: null, marks: ["later"], refused: [false] };
    expect(parseDriveStatus(["C:\\a"], answer).marks.size).toBe(0);
    expect(parseDriveStatus(["C:\\a"], { ...answer, ready: false })).toBe(NO_DRIVE);
    expect(parseDriveStatus(["C:\\a"], { ...answer, refused: undefined })).toBe(NO_DRIVE);
    expect(parseDriveStatus(["C:\\a"], { installed: true, marks: [] })).toBe(NO_DRIVE);
    expect(parseDriveStatus(["C:\\a"], null)).toBe(NO_DRIVE);
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
    for (const path of ["C:\\a\\console", "C:\\a\\com10", "C:\\a\\v1.2", "C:\\"]) {
      expect(isLocalFolder(path), path).toBe(true);
    }
    expect(isLocalFolder("/home/a/con")).toBe(true);
    expect(isLocalFolder("/home/a/proj.")).toBe(true);
  });

  it("refuses parts starting with a dot at any level", () => {
    for (const path of [
      "C:\\a\\.git",
      "C:\\.config\\proj",
      "C:/a/.ssh/b",
      "C:\\a\\.\\b",
      "/home/a/.config",
      "/home/.a/proj",
    ]) {
      expect(isLocalFolder(path), path).toBe(false);
    }
    expect(isLocalFolder("C:\\a\\proj.d")).toBe(true);
  });

  it("refuses the home folder and its direct children, not deeper folders", () => {
    const home = "C:\\Users\\Jörg";
    for (const path of [
      "C:\\Users\\Jörg",
      "c:\\users\\jörg\\",
      "C:\\Users\\Jörg\\Documents",
      "C:/USERS/JÖRG/Desktop",
    ]) {
      expect(isLocalFolder(path, home), path).toBe(false);
    }
    expect(isLocalFolder("C:\\Users\\Jörg\\Documents\\proj", home)).toBe(true);
    expect(isLocalFolder("C:\\Users", home)).toBe(true);
    expect(isLocalFolder("C:\\Users\\Jörg2", home)).toBe(true);
    expect(isLocalFolder("C:\\Users\\Jörg\\Documents")).toBe(true);
    expect(isLocalFolder("/home/a/proj", "/home/a")).toBe(false);
    expect(isLocalFolder("/home/A/proj", "/home/a")).toBe(true);
    expect(isLocalFolder("/home/a/work/proj", "/home/a")).toBe(true);
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
  const ready = {
    ready: true,
    home: "C:\\Users\\a",
    marks: new Map([["C:\\a\\proj", "active" as const]]),
    refused: new Set(["C:\\a\\hidden"]),
  };

  it("offers only a ready Hangar Drive a folder it does not hold yet", () => {
    expect(canBackUp(ready, "C:\\a\\other")).toBe(true);
    expect(canBackUp(ready, "C:\\a\\proj")).toBe(false);
    expect(canBackUp(ready, null)).toBe(false);
    expect(canBackUp(NO_DRIVE, "C:\\a\\other")).toBe(false);
  });

  it("offers nothing the backend refuses or the rules forbid", () => {
    expect(canBackUp(ready, "C:\\a\\hidden")).toBe(false);
    expect(canBackUp(ready, "C:\\Users\\a\\Documents")).toBe(false);
    expect(canBackUp(ready, "C:\\Users\\a")).toBe(false);
    expect(canBackUp(ready, "C:\\Users\\a\\Documents\\proj")).toBe(true);
    expect(canBackUp(ready, "C:\\a\\.git")).toBe(false);
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

  it("hands over nothing when the main folder is one Hangar Drive refuses", async () => {
    expect(
      await backUpFolder(
        { cwd: "C:\\Users\\a\\proj\\src" },
        () => Promise.resolve("C:\\Users\\a\\proj"),
        "C:\\Users\\a",
      ),
    ).toBeNull();
    expect(
      await backUpFolder(
        { cwd: "C:\\Users\\a\\proj" },
        () => Promise.resolve(null),
        "C:\\Users\\a",
      ),
    ).toBeNull();
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

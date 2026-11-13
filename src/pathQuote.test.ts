import { describe, expect, it } from "vitest";

import {
  dropText,
  insertPathText,
  isShellKind,
  quotePath,
  shellKind,
  wslPath,
  type TreePath,
} from "./pathQuote";

describe("shellKind", () => {
  it("recognizes shells by file name, with path and extension", () => {
    expect(shellKind("pwsh")).toBe("powershell");
    expect(shellKind("C:\\Program Files\\PowerShell\\7\\pwsh.exe")).toBe("powershell");
    expect(shellKind("powershell.exe")).toBe("powershell");
    expect(shellKind("PowerShell.EXE")).toBe("powershell");
    expect(shellKind("cmd.exe")).toBe("cmd");
    expect(shellKind("C:\\Windows\\System32\\cmd.exe")).toBe("cmd");
    expect(shellKind("/bin/bash")).toBe("posix");
    expect(shellKind("zsh")).toBe("posix");
    expect(shellKind("/usr/bin/fish")).toBe("posix");
    expect(shellKind("sh")).toBe("posix");
    expect(shellKind("C:\\Program Files\\Git\\bin\\bash.exe")).toBe("posix");
    expect(shellKind("wsl.exe")).toBe("wsl");
    expect(shellKind("wsl")).toBe("wsl");
  });

  it("treats bash from the Windows system folder as WSL, Git Bash as POSIX", () => {
    expect(shellKind("C:\\Windows\\System32\\bash.exe")).toBe("wsl");
    expect(shellKind("c:/windows/system32/BASH.EXE")).toBe("wsl");
    expect(shellKind("C:\\Windows\\Sysnative\\bash.exe")).toBe("wsl");
    expect(shellKind("C:\\Program Files\\Git\\usr\\bin\\bash.exe")).toBe("posix");
    expect(shellKind("bash.exe")).toBe("posix");
  });

  it("treats everything else as an agent", () => {
    expect(shellKind("claude")).toBe("agent");
    expect(shellKind("C:\\Users\\a\\AppData\\Roaming\\npm\\codex.cmd")).toBe("agent");
    expect(shellKind("aider")).toBe("agent");
    expect(shellKind("")).toBe("agent");
    expect(shellKind("bashful")).toBe("agent");
  });
});

describe("PowerShell", () => {
  it("leaves simple paths alone", () => {
    expect(quotePath("C:\\Users\\Ada\\notes.md", "powershell")).toBe("C:\\Users\\Ada\\notes.md");
  });

  it("uses single quotes for spaces and doubles apostrophes", () => {
    expect(quotePath("C:\\My Files\\a.txt", "powershell")).toBe("'C:\\My Files\\a.txt'");
    expect(quotePath("C:\\Ada's\\a.txt", "powershell")).toBe("'C:\\Ada''s\\a.txt'");
  });

  it("doubles typographic single quotes, which PowerShell also ends strings with", () => {
    expect(quotePath("C:\\Ada\u2019s Notes", "powershell")).toBe("'C:\\Ada\u2019\u2019s Notes'");
  });

  it("quotes characters with a meaning in PowerShell", () => {
    expect(quotePath("C:\\a$b", "powershell")).toBe("'C:\\a$b'");
    expect(quotePath("C:\\a,b", "powershell")).toBe("'C:\\a,b'");
    expect(quotePath("C:\\x (1)\\y", "powershell")).toBe("'C:\\x (1)\\y'");
    expect(quotePath("C:\\a`b", "powershell")).toBe("'C:\\a`b'");
  });

  it("keeps Unicode letters and UNC paths", () => {
    expect(quotePath("C:\\Übersicht\\日本", "powershell")).toBe("C:\\Übersicht\\日本");
    expect(quotePath("\\\\server\\share\\file.txt", "powershell")).toBe(
      "\\\\server\\share\\file.txt",
    );
    expect(quotePath("\\\\server\\my share\\file.txt", "powershell")).toBe(
      "'\\\\server\\my share\\file.txt'",
    );
  });
});

describe("cmd", () => {
  it("uses double quotes only when needed", () => {
    expect(quotePath("C:\\a\\b.txt", "cmd")).toBe("C:\\a\\b.txt");
    expect(quotePath("C:\\My Files\\b.txt", "cmd")).toBe('"C:\\My Files\\b.txt"');
    expect(quotePath("C:\\a&b", "cmd")).toBe('"C:\\a&b"');
    expect(quotePath("C:\\Ada's", "cmd")).toBe('"C:\\Ada\'s"');
    expect(quotePath("\\\\server\\my share", "cmd")).toBe('"\\\\server\\my share"');
  });

  // Documents a known limit: cmd expands %NAME% inside double quotes when NAME is defined, and an
  // interactive command line has no escape for it. The path is passed through unchanged.
  it("leaves percent signs as they are, quoted like any other special character", () => {
    expect(quotePath("C:\\100%\\a.txt", "cmd")).toBe('"C:\\100%\\a.txt"');
    expect(quotePath("C:\\%PATH%\\a.txt", "cmd")).toBe('"C:\\%PATH%\\a.txt"');
  });
});

describe("POSIX shells", () => {
  it("leaves simple paths alone", () => {
    expect(quotePath("/home/ada/file.txt", "posix")).toBe("/home/ada/file.txt");
  });

  it("uses single quotes and escapes apostrophes", () => {
    expect(quotePath("/home/ada/my file", "posix")).toBe("'/home/ada/my file'");
    expect(quotePath("/home/ada/ada's", "posix")).toBe("'/home/ada/ada'\\''s'");
    expect(quotePath('/tmp/say "hi"', "posix")).toBe("'/tmp/say \"hi\"'");
    expect(quotePath("/tmp/$HOME", "posix")).toBe("'/tmp/$HOME'");
  });

  it("quotes Windows paths, because backslashes escape in POSIX shells", () => {
    expect(quotePath("C:\\Users\\Ada", "posix")).toBe("'C:\\Users\\Ada'");
  });

  it("keeps Unicode letters unquoted", () => {
    expect(quotePath("/home/zoë/файл", "posix")).toBe("/home/zoë/файл");
  });
});

describe("WSL", () => {
  it("maps drive and WSL share paths", () => {
    expect(wslPath("C:\\Users\\Ada\\a.txt")).toBe("/mnt/c/Users/Ada/a.txt");
    expect(wslPath("D:\\")).toBe("/mnt/d");
    expect(wslPath("\\\\wsl$\\Ubuntu\\home\\ada")).toBe("/home/ada");
    expect(wslPath("\\\\wsl.localhost\\Ubuntu-22.04\\etc\\hosts")).toBe("/etc/hosts");
    expect(wslPath("\\\\wsl$\\Ubuntu")).toBe("/");
    expect(wslPath("/already/posix")).toBe("/already/posix");
  });

  it("quotes like a POSIX shell after mapping", () => {
    expect(quotePath("C:\\My Files\\ada's.txt", "wsl")).toBe("'/mnt/c/My Files/ada'\\''s.txt'");
    expect(quotePath("C:\\a\\b.txt", "wsl")).toBe("/mnt/c/a/b.txt");
  });

  it("leaves other UNC paths to the POSIX quoting", () => {
    expect(quotePath("\\\\server\\share", "wsl")).toBe("'\\\\server\\share'");
  });
});

describe("agent CLIs", () => {
  it("quote only paths with spaces or quotes", () => {
    expect(quotePath("C:\\a\\b.png", "agent")).toBe("C:\\a\\b.png");
    expect(quotePath("C:\\My Files\\b.png", "agent")).toBe('"C:\\My Files\\b.png"');
    expect(quotePath("/tmp/ada's.png", "agent")).toBe('"/tmp/ada\'s.png"');
    expect(quotePath('/tmp/say "hi".png', "agent")).toBe("'/tmp/say \"hi\".png'");
    expect(quotePath('/tmp/ada\'s "x"\\y.png', "agent")).toBe('"/tmp/ada\'s \\"x\\"\\\\y.png"');
    expect(quotePath("C:\\Übersicht\\日本.png", "agent")).toBe("C:\\Übersicht\\日本.png");
  });
});

describe("dropText", () => {
  it("joins several paths and ends with a space", () => {
    expect(dropText(["C:\\a.txt", "C:\\b c.txt"], "powershell")).toBe("C:\\a.txt 'C:\\b c.txt' ");
  });

  it("is empty without paths", () => {
    expect(dropText([], "cmd")).toBe("");
    expect(dropText([""], "cmd")).toBe("");
  });
});

describe("isShellKind", () => {
  it("accepts only the known kinds", () => {
    expect(isShellKind("powershell")).toBe(true);
    expect(isShellKind("agent")).toBe(true);
    expect(isShellKind("bash")).toBe(false);
    expect(isShellKind(undefined)).toBe(false);
  });
});

describe("insertPathText", () => {
  const repo = String.raw`C:\work\shop`;
  const file = (path: string, root = repo): TreePath => ({ root, path, folder: false });
  const folder = (path: string, root = repo): TreePath => ({ root, path, folder: true });

  describe("agent", () => {
    it("mentions the path from the session's folder with @ and forward slashes", () => {
      expect(insertPathText(file("xyz_test/test.md"), repo, "agent")).toBe("@xyz_test/test.md ");
      expect(insertPathText(file("README.md"), "c:/work/shop/", "agent")).toBe("@README.md ");
    });

    it("marks a folder with a trailing slash", () => {
      expect(insertPathText(folder("src/components"), repo, "agent")).toBe("@src/components/ ");
    });

    it("quotes paths with spaces or quotes so the agent reads one path", () => {
      expect(insertPathText(file("docs/my notes.md"), repo, "agent")).toBe('@"docs/my notes.md" ');
      expect(insertPathText(file("ada's.md"), repo, "agent")).toBe('@"ada\'s.md" ');
      expect(insertPathText(file('say "hi".md'), repo, "agent")).toBe("@'say \"hi\".md' ");
    });

    it("keeps umlauts and other scripts as they are", () => {
      expect(insertPathText(file("Übersicht/日本.md"), repo, "agent")).toBe("@Übersicht/日本.md ");
    });

    it("names a path outside the session's folder absolutely", () => {
      expect(insertPathText(file("src/a.ts"), String.raw`C:\work\other`, "agent")).toBe(
        "@C:/work/shop/src/a.ts ",
      );
      // A folder whose name only starts like the session's folder is outside it.
      expect(insertPathText(file("a.ts"), String.raw`C:\work\sh`, "agent")).toBe(
        "@C:/work/shop/a.ts ",
      );
      expect(insertPathText(folder("my dir"), "/home/ada/b", "agent")).toBe(
        '@"C:/work/shop/my dir/" ',
      );
    });

    it("works from a worktree session's own folder", () => {
      const worktree = String.raw`C:\work\shop-worktrees\fix-login`;
      expect(insertPathText(file("src/login.ts", worktree), worktree, "agent")).toBe(
        "@src/login.ts ",
      );
      // Dragged onto a session in the main checkout, the worktree's file stays the right one.
      expect(insertPathText(file("src/login.ts", worktree), repo, "agent")).toBe(
        "@C:/work/shop-worktrees/fix-login/src/login.ts ",
      );
    });

    it("works on POSIX paths, case sensitive", () => {
      expect(insertPathText(file("a/b.md", "/home/ada/app"), "/home/ada/app", "agent")).toBe(
        "@a/b.md ",
      );
      expect(insertPathText(file("a/b.md", "/home/ada/app"), "/home/ada/App", "agent")).toBe(
        "@/home/ada/app/a/b.md ",
      );
    });

    it("names a file of a drive's root folder", () => {
      expect(insertPathText(file("a.md", "C:\\"), "D:\\", "agent")).toBe("@C:/a.md ");
      expect(insertPathText(file("a.md", "C:\\"), "c:", "agent")).toBe("@a.md ");
    });
  });

  describe("powershell", () => {
    it("types a relative path with backslashes and no @", () => {
      expect(insertPathText(file("xyz_test/test.md"), repo, "powershell")).toBe(
        "xyz_test\\test.md ",
      );
      expect(insertPathText(folder("src/components"), repo, "powershell")).toBe("src\\components ");
    });

    it("single-quotes spaces and special characters, doubling single quotes", () => {
      expect(insertPathText(file("my notes/a.md"), repo, "powershell")).toBe("'my notes\\a.md' ");
      expect(insertPathText(file("ada's.md"), repo, "powershell")).toBe("'ada''s.md' ");
      expect(insertPathText(file("$x.md"), repo, "powershell")).toBe("'$x.md' ");
    });

    it("keeps umlauts unquoted", () => {
      expect(insertPathText(file("Übersicht/ä.md"), repo, "powershell")).toBe("Übersicht\\ä.md ");
    });

    it("types a path outside the session's folder absolutely", () => {
      expect(insertPathText(file("a b.md"), String.raw`D:\elsewhere`, "powershell")).toBe(
        "'C:\\work\\shop\\a b.md' ",
      );
    });

    it("keeps a name starting with a dash from reading as a parameter", () => {
      expect(insertPathText(file("-x.md"), repo, "powershell")).toBe(".\\-x.md ");
    });
  });

  describe("cmd", () => {
    it("types backslashes, double quotes only when needed", () => {
      expect(insertPathText(file("xyz_test/test.md"), repo, "cmd")).toBe("xyz_test\\test.md ");
      expect(insertPathText(file("my notes/a&b.md"), repo, "cmd")).toBe('"my notes\\a&b.md" ');
      expect(insertPathText(folder("src"), repo, "cmd")).toBe("src ");
      expect(insertPathText(file("a.md"), String.raw`D:\x`, "cmd")).toBe("C:\\work\\shop\\a.md ");
    });
  });

  describe("posix", () => {
    it("types relative paths with forward slashes", () => {
      expect(insertPathText(file("xyz_test/test.md"), repo, "posix")).toBe("xyz_test/test.md ");
      expect(insertPathText(folder("src"), repo, "posix")).toBe("src ");
    });

    it("single-quotes spaces and quotes", () => {
      expect(insertPathText(file("my notes.md"), repo, "posix")).toBe("'my notes.md' ");
      expect(insertPathText(file("ada's.md"), repo, "posix")).toBe("'ada'\\''s.md' ");
      expect(insertPathText(file("-x.md"), repo, "posix")).toBe("./-x.md ");
    });

    it("names a Windows path outside the folder as Git Bash does", () => {
      expect(insertPathText(file("a b.md"), String.raw`D:\x`, "posix")).toBe(
        "'/c/work/shop/a b.md' ",
      );
      expect(insertPathText(file("ä.md", "/home/zoë"), "/tmp", "posix")).toBe("/home/zoë/ä.md ");
    });
  });

  describe("special characters", () => {
    it("single-quotes every PowerShell metacharacter", () => {
      for (const name of ["a$b", "a`b", "a(b)", "a&b", "a;b", "a,b", "@a", "a{b}", "a[b]", "#a"]) {
        expect(insertPathText(file(name), repo, "powershell")).toBe(`'${name}' `);
      }
      expect(insertPathText(file("a\u2019b"), repo, "powershell")).toBe("'a\u2019\u2019b' ");
    });

    it("double-quotes cmd metacharacters", () => {
      for (const name of ["a&b", "a^b", "a%b%", "a b", "a|b", "a(b)"]) {
        expect(insertPathText(file(name), repo, "cmd")).toBe(`"${name}" `);
      }
    });

    it("single-quotes POSIX metacharacters so nothing expands", () => {
      for (const name of ["a$(b)", "a;b", "a`b`", "~a", "a*b", String.raw`a\b`]) {
        expect(insertPathText(file(name, "/srv"), "/srv", "posix")).toBe(`'${name}' `);
      }
    });

    it("inserts nothing for a name with a line break or another control character", () => {
      for (const kind of ["agent", "powershell", "cmd", "posix", "wsl"] as const) {
        expect(insertPathText(file("a\nmarker b", "/srv"), "/srv", kind)).toBe("");
        expect(insertPathText(file("a\u001b[201~b", "/srv"), "/srv", kind)).toBe("");
        expect(insertPathText(file("a\u009bb", "/srv"), "/srv", kind)).toBe("");
        expect(insertPathText(file("a.md", "/sr\rv"), "/srv", kind)).toBe("");
      }
    });
  });

  describe("wsl", () => {
    it("lowercases the drive and maps WSL shares to Linux paths", () => {
      expect(insertPathText(file("a.md", String.raw`D:\Data`), String.raw`C:\x`, "wsl")).toBe(
        "/mnt/d/Data/a.md ",
      );
      expect(
        insertPathText(file("a.md", "\\\\wsl$\\Ubuntu\\home\\ada"), String.raw`C:\x`, "wsl"),
      ).toBe("/home/ada/a.md ");
      expect(insertPathText(file("-a.md", "/srv"), "/srv", "wsl")).toBe("./-a.md ");
    });

    it("types relative paths inside the folder and /mnt paths outside", () => {
      expect(insertPathText(file("xyz_test/test.md"), repo, "wsl")).toBe("xyz_test/test.md ");
      expect(insertPathText(file("a b.md"), String.raw`D:\x`, "wsl")).toBe(
        "'/mnt/c/work/shop/a b.md' ",
      );
    });
  });
});

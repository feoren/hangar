import { describe, expect, it } from "vitest";

import {
  dropText,
  gitBashPath,
  hasControl,
  unsafeDropPath,
  insertPathText,
  isShellKind,
  mentionPath,
  quotePath,
  skipsDropPath,
  shellKind,
  visiblePath,
  wslPath,
  type TreePath,
} from "./pathQuote";
import { unquotePath } from "./gitDiff";

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

  it("maps paths holding line or paragraph separators like any other path", () => {
    expect(wslPath("C:\\a\u2028b\\c.txt")).toBe("/mnt/c/a\u2028b/c.txt");
    expect(wslPath("C:\\a\u2029b")).toBe("/mnt/c/a\u2029b");
    expect(wslPath("\\\\wsl$\\Ubuntu\\home\\a\u2028b")).toBe("/home/a\u2028b");
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
  it("quote only paths with spaces, quotes, $ or backticks, in double quotes when nothing expands", () => {
    expect(quotePath("C:\\a\\b.png", "agent")).toBe("C:\\a\\b.png");
    expect(quotePath("C:\\My Files\\b.png", "agent")).toBe('"C:\\My Files\\b.png"');
    expect(quotePath("/tmp/ada's.png", "agent")).toBe('"/tmp/ada\'s.png"');
    expect(quotePath('/tmp/say "hi".png', "agent")).toBe("'/tmp/say \"hi\".png'");
    expect(quotePath("C:\\Übersicht\\日本.png", "agent")).toBe("C:\\Übersicht\\日本.png");
  });

  // An agent's `!` mode hands the input to bash, and PowerShell expands double quotes too: a
  // dropped path must not run `$(...)` or backticks there.
  it("never puts $ or backticks inside double quotes, and skips what cannot be quoted", () => {
    expect(quotePath("C:\\in\\report $(marker).txt", "agent")).toBe(
      "'C:\\in\\report $(marker).txt'",
    );
    expect(quotePath("/tmp/a$(marker).png", "agent")).toBe("'/tmp/a$(marker).png'");
    expect(quotePath("/tmp/a`marker`.png", "agent")).toBe("'/tmp/a`marker`.png'");
    expect(quotePath("/tmp/ada's $(marker).png", "agent")).toBe("");
    expect(quotePath("/tmp/ada's `marker`.png", "agent")).toBe("");
    expect(quotePath('/tmp/ada\'s "x".png', "agent")).toBe("");
  });

  it("show control, separator and bidi characters in git's quoting", () => {
    expect(quotePath("/tmp/a\nb", "agent")).toBe('"/tmp/a\\nb"');
    expect(quotePath("/tmp/a\rb\tc", "agent")).toBe('"/tmp/a\\rb\\tc"');
    expect(quotePath('C:\\x\\"y"\u0007', "agent")).toBe('"C:\\\\x\\\\\\"y\\"\\a"');
    expect(quotePath("/tmp/a\u2029b", "agent")).toBe('"/tmp/a\\342\\200\\251b"');
    // Joiners stay: Persian and Indic names and emoji need them.
    expect(quotePath("/tmp/a\u200db", "agent")).toBe("/tmp/a\u200db");
  });
});

describe("visiblePath", () => {
  it("leaves a plain path alone, spaces, umlauts, emoji and joiners included", () => {
    for (const path of [
      "src/app.ts",
      "my notes.md",
      "\u00dcbersicht/\u65e5 \u{1f600}.md",
      "a\u200db",
    ]) {
      expect(visiblePath(path)).toBe(path);
    }
  });

  it("quotes quotes and backslashes as git does, so a name cannot pass as quoting", () => {
    expect(visiblePath('"x"')).toBe('"\\"x\\""');
    expect(visiblePath("a\\b")).toBe('"a\\\\b"');
  });

  it("reads back into the real name", () => {
    const paths = [
      "one\ntwo.ts",
      '"x"',
      'say "hi"\n.ts',
      "back\\slash\t.ts",
      "\\001\n",
      "\u0000\u0007\b\v\f\r\u001b\u007f\u0085\u009f",
      "\u00fc\u2028\u65e5\u202e\u{1f600}\u2066x\u200e\u061c",
      "a\u200db\n",
    ];
    for (const path of paths) {
      const shown = visiblePath(path);
      expect(unsafeDropPath(shown)).toBe(false);
      expect(unquotePath(shown)).toBe(path);
    }
  });
});

describe("mentionPath", () => {
  it("keeps double quotes for spaces, single quotes once $ or a backtick is in the name", () => {
    expect(mentionPath("src/app.ts")).toBe("src/app.ts");
    expect(mentionPath("docs/my notes.md")).toBe('"docs/my notes.md"');
    expect(mentionPath("docs/my $(marker).md")).toBe("'docs/my $(marker).md'");
    expect(mentionPath("docs/a`marker`.md")).toBe("'docs/a`marker`.md'");
    expect(mentionPath('say "hi".md')).toBe("'say \"hi\".md'");
    expect(mentionPath('ada\'s "x"\\y.md')).toBe('"ada\'s \\"x\\"\\\\y.md"');
  });
});

describe("skipsDropPath", () => {
  it("skips paths no quoting makes safe for the program", () => {
    expect(skipsDropPath("C:\\in\\report $(marker).txt", "cmd")).toBe(true);
    expect(skipsDropPath("C:\\in\\a`marker`.txt", "cmd")).toBe(true);
    expect(skipsDropPath("/tmp/ada's $(marker).png", "agent")).toBe(true);
    expect(skipsDropPath("/tmp/a\nb", "posix")).toBe(true);
    expect(skipsDropPath("C:\\in\\report $(marker).txt", "powershell")).toBe(false);
    expect(skipsDropPath("C:\\in\\report $(marker).txt", "agent")).toBe(false);
    expect(skipsDropPath("C:\\My Files\\b.txt", "cmd")).toBe(false);
  });
});

describe("dropText", () => {
  it("leaves out a path with $ or a backtick for cmd, which a pwsh behind it would expand", () => {
    expect(dropText(["C:\\in\\report $(marker).txt"], "cmd")).toBe("");
    expect(dropText(["C:\\in\\a.txt", "C:\\in\\b`marker`.txt"], "cmd")).toBe("C:\\in\\a.txt ");
    expect(dropText(["C:\\in\\report $(marker).txt"], "agent")).toBe(
      "'C:\\in\\report $(marker).txt' ",
    );
  });

  it("joins several paths and ends with a space", () => {
    expect(dropText(["C:\\a.txt", "C:\\b c.txt"], "powershell")).toBe("C:\\a.txt 'C:\\b c.txt' ");
  });

  it("is empty without paths", () => {
    expect(dropText([], "cmd")).toBe("");
    expect(dropText([""], "cmd")).toBe("");
  });

  it("leaves out paths with control characters, which no quoting makes safe", () => {
    expect(dropText(["/tmp/a\nb"], "agent")).toBe("");
    expect(dropText(["/tmp/a\nb", "/tmp/ok"], "posix")).toBe("/tmp/ok ");
    expect(dropText(["C:\\a\u001b[201~b.txt"], "powershell")).toBe("");
  });

  it("rejects every C0 and C1 control character on its own, at both range edges", () => {
    for (const control of ["\u0000", "\u001f", "\u007f", "\u0080", "\u009b", "\u009f"]) {
      for (const kind of ["agent", "powershell", "cmd", "posix", "wsl"] as const) {
        expect(dropText([`/tmp/a${control}b`, "/tmp/ok"], kind)).toBe(dropText(["/tmp/ok"], kind));
        expect(dropText([`/tmp/a${control}b`], kind)).toBe("");
      }
    }
  });

  it("keeps the printable neighbours of the control ranges", () => {
    expect(dropText(["/tmp/a b~c"], "agent")).not.toBe("");
    expect(dropText(["/tmp/a\u00a0b"], "agent")).not.toBe("");
  });

  it("leaves out paths with bidi controls, which read otherwise than they are", () => {
    expect(dropText(["/tmp/invoice\u202etxt.sh"], "posix")).toBe("");
    expect(dropText(["C:\\a\u200fb.txt", "C:\\ok.txt"], "powershell")).toBe("C:\\ok.txt ");
    expect(dropText(["/tmp/x\u2066y"], "agent")).toBe("");
  });

  it("keeps Persian, Indic and emoji file names with zero-width joiners", () => {
    const persian = "/tmp/\u0645\u06cc\u200c\u062e\u0648\u0627\u0647\u0645.txt";
    expect(dropText([persian], "posix")).toBe(`'${persian}' `);
    const devanagari = "/tmp/\u0915\u094d\u200d\u0937.md";
    expect(dropText([devanagari], "agent")).toBe(`${devanagari} `);
    const emoji = "/tmp/\u{1F468}\u200d\u{1F4BB} notes.md";
    expect(dropText([emoji], "agent")).toBe(`"${emoji}" `);
  });
});

describe("hasControl", () => {
  it("finds line breaks, ESC and C1 characters", () => {
    expect(hasControl("/tmp/a\rb")).toBe(true);
    expect(hasControl("C:\\a\u009bb")).toBe(true);
    expect(hasControl("C:\\Users\\Jane Doe\\ä.txt")).toBe(false);
  });
});

describe("unsafeDropPath", () => {
  it("finds control and bidi characters and separators, nothing else", () => {
    expect(unsafeDropPath("/tmp/a\nb")).toBe(true);
    expect(unsafeDropPath("/tmp/a\u202eb")).toBe(true);
    expect(unsafeDropPath("/tmp/a\u2029b")).toBe(true);
    expect(unsafeDropPath("/tmp/a\u061cb")).toBe(true);
    expect(unsafeDropPath("/tmp/a\u{e0069}b")).toBe(true);
    expect(unsafeDropPath("/tmp/\u0645\u06cc\u200c\u0631\u0648\u0645")).toBe(false);
    expect(unsafeDropPath("C:\\Users\\Jane Doe\\Übersicht 日本.txt")).toBe(false);
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

    it("single-quotes a name with $ or a backtick, so a shell would not expand it", () => {
      expect(insertPathText(file("my $(marker).md"), repo, "agent")).toBe("@'my $(marker).md' ");
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

    it("inserts nothing for a name with $ or a backtick", () => {
      expect(insertPathText(file("report $(marker).md"), repo, "cmd")).toBe("");
      expect(insertPathText(file("a`marker`.md"), repo, "cmd")).toBe("");
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

    // Such a path is no longer inserted into a shell (see "special characters"); the drive
    // mapping itself still has to cope with it.
    it("maps the drive of an absolute path holding line or paragraph separators", () => {
      expect(gitBashPath("C:/work/shop/a\u2028b.md")).toBe("/c/work/shop/a\u2028b.md");
      expect(wslPath("C:/work/shop/a\u2029b.md")).toBe("/mnt/c/work/shop/a\u2029b.md");
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

    // A shell runs what it reads while the person sees something else; the agent gets the
    // path as a mention it reads itself, and the drop warning covers it there.
    it("inserts nothing into a shell for a name that reads otherwise than it is", () => {
      for (const kind of ["powershell", "cmd", "posix", "wsl"] as const) {
        expect(insertPathText(file("invoice‮txt.sh", "/srv"), "/srv", kind)).toBe("");
        expect(insertPathText(file("a b", "/srv"), "/srv", kind)).toBe("");
        expect(insertPathText(file("a\u{e0069}b", "/srv"), "/srv", kind)).toBe("");
        expect(insertPathText(file("a.md", "/s⁦rv"), "/srv", kind)).toBe("");
      }
      // Joiners in Persian names and emoji change no reading order and stay.
      const persian = "می‌ر";
      expect(insertPathText(file(persian, "/srv"), "/srv", "posix")).toContain(persian);
      expect(insertPathText(file("a‮b", "/srv"), "/srv", "agent")).not.toBe("");
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

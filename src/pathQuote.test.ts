import { describe, expect, it } from "vitest";

import { dropText, quotePath, shellKind, wslPath } from "./pathQuote";

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
    expect(quotePath('/tmp/say "hi".png', "agent")).toBe('"/tmp/say \\"hi\\".png"');
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

import { describe, expect, it } from "vitest";

import {
  capDiff,
  DIFF_MAX_CHARS,
  expandVariables,
  fileValue,
  hasVariables,
  missingSummary,
  type SnippetVariable,
  unsafeSummary,
  usedVariables,
} from "./snippetVariables";
import type { ShellKind } from "./pathQuote";

describe("usedVariables", () => {
  it("finds known and unknown names once, in order, ignoring case and inner spaces", () => {
    expect(usedVariables("{{diff}} for {{ Branch }} and {{diff}}, {{foo}} {{foo}}")).toEqual({
      known: ["diff", "branch"],
      unknown: ["foo"],
    });
  });

  it("ignores text that only looks similar", () => {
    expect(usedVariables("{file} {{ }} {{1abc}} { {file} }")).toEqual({ known: [], unknown: [] });
    expect(hasVariables("Review the changes")).toBe(false);
    expect(hasVariables("Review {{diff}}")).toBe(true);
  });
});

describe("expandVariables", () => {
  it("fills every variable", () => {
    expect(
      expandVariables("Look at {{file}} on {{branch}} in {{folder}}:\n{{selection}}", {
        file: "@src/App.tsx",
        branch: "main",
        folder: "C:\\work\\app",
        selection: "error 42",
      }),
    ).toEqual({ ok: true, text: "Look at @src/App.tsx on main in C:\\work\\app:\nerror 42" });
  });

  it("leaves text without variables as it is", () => {
    expect(expandVariables("Write tests", {})).toEqual({ ok: true, text: "Write tests" });
  });

  it("does not expand variables inside values", () => {
    expect(expandVariables("{{selection}}", { selection: "{{diff}}" })).toEqual({
      ok: true,
      text: "{{diff}}",
    });
  });

  it("refuses when a known value is missing or blank, naming all of them", () => {
    expect(
      expandVariables("{{file}} {{diff}} {{folder}} {{selection}}", {
        folder: "/x",
        selection: "  ",
      }),
    ).toEqual({ ok: false, missing: ["file", "diff", "selection"] });
  });

  it("leaves unknown names as they are", () => {
    expect(expandVariables("{{folder}} {{ticket}} {{ user.name }}", { folder: "/x" })).toEqual({
      ok: true,
      text: "/x {{ticket}} {{ user.name }}",
    });
    expect(expandVariables("Hello {{name}}", {})).toEqual({ ok: true, text: "Hello {{name}}" });
  });
});

describe("expandVariables for a shell", () => {
  const SHELLS = ["posix", "wsl", "powershell", "cmd"] as const;
  const filled = (text: string) => ({ ok: true, text });
  const refusedFor = (name: SnippetVariable) => ({ ok: false, unsafe: [name] });

  it("leaves values for an agent as they are", () => {
    expect(expandVariables("Review {{branch}}", { branch: "x;echo marker" }, "agent")).toEqual(
      filled("Review x;echo marker"),
    );
  });

  it("keeps plain values plain", () => {
    for (const kind of SHELLS) {
      expect(expandVariables("git switch {{branch}}", { branch: "feature/x-1.2" }, kind)).toEqual(
        filled("git switch feature/x-1.2"),
      );
    }
    expect(expandVariables("git switch {{branch}}", { branch: "ветка/分支" }, "posix")).toEqual(
      filled("git switch ветка/分支"),
    );
  });

  it("quotes a value with spaces as one word, for the shell's rules", () => {
    const quoted = (kind: ShellKind) =>
      expandVariables("cd {{folder}}", { folder: "C:\\work\\my app" }, kind);
    expect(quoted("cmd")).toEqual(filled('cd "C:\\work\\my app"'));
    expect(quoted("powershell")).toEqual(filled("cd 'C:\\work\\my app'"));
    // Git Bash takes a Windows folder with `/`, which needs no escaping there.
    expect(quoted("posix")).toEqual(filled("cd 'C:/work/my app'"));
    expect(quoted("wsl")).toEqual(filled("cd '/mnt/c/work/my app'"));
  });

  it("takes ordinary Windows folders in every shell, quoted by Hangar or by the snippet", () => {
    const folder = { folder: "C:\\Program Files (x86)\\Tool & Co" };
    const fill = (text: string, kind: ShellKind) => expandVariables(text, folder, kind);
    expect(fill("cd {{folder}}", "cmd")).toEqual(filled('cd "C:\\Program Files (x86)\\Tool & Co"'));
    expect(fill('cd "{{folder}}"', "cmd")).toEqual(
      filled('cd "C:\\Program Files (x86)\\Tool & Co"'),
    );
    expect(fill("cd {{folder}}", "powershell")).toEqual(
      filled("cd 'C:\\Program Files (x86)\\Tool & Co'"),
    );
    expect(fill('cd "{{folder}}"', "powershell")).toEqual(
      filled('cd "C:\\Program Files (x86)\\Tool & Co"'),
    );
    expect(fill("cd {{folder}}", "posix")).toEqual(filled("cd 'C:/Program Files (x86)/Tool & Co'"));
    expect(fill("cd '{{folder}}'", "posix")).toEqual(
      filled("cd 'C:/Program Files (x86)/Tool & Co'"),
    );
    expect(fill("cd {{folder}}", "wsl")).toEqual(
      filled("cd '/mnt/c/Program Files (x86)/Tool & Co'"),
    );
  });

  it("does not quote twice when the snippet already quotes the variable", () => {
    const folder = { folder: "/home/me/my app" };
    expect(expandVariables('cd "{{folder}}"', folder, "posix")).toEqual(
      filled('cd "/home/me/my app"'),
    );
    expect(expandVariables("cd '{{ folder }}'", folder, "powershell")).toEqual(
      filled("cd '/home/me/my app'"),
    );
    expect(expandVariables('cd "{{folder}}/sub dir"', folder, "posix")).toEqual(
      filled('cd "/home/me/my app/sub dir"'),
    );
  });

  // A value Hangar quotes itself cannot leave its quotes, whatever it holds.
  it("quotes any value where the snippet leaves the variable unquoted", () => {
    const value = 'x\'; echo "marker" $(marker) `marker` \\ !marker | & > (y)';
    expect(expandVariables("git log {{selection}}", { selection: value }, "posix")).toEqual(
      filled("git log 'x'\\''; echo \"marker\" $(marker) `marker` '\\\\' '\\!'marker | & > (y)'"),
    );
    expect(expandVariables("git log {{selection}}", { selection: value }, "powershell")).toEqual(
      filled(`git log '${value.replace(/'/g, "''")}'`),
    );
    expect(expandVariables("echo {{selection}}", { selection: "it\u2019s" }, "powershell")).toEqual(
      filled("echo 'it\u2019\u2019s'"),
    );
    expect(expandVariables("printf %s {{selection}}", { selection: "one\ntwo" }, "posix")).toEqual(
      filled("printf %s 'one\ntwo'"),
    );
  });

  it("refuses for cmd what its double quotes cannot hold, quoted by Hangar or not", () => {
    const risky = ['x"y', "%marker%", "x!marker!", "x$(marker)", "x`y`", "x'y", "x\u2019y", "x\ny"];
    for (const branch of risky) {
      const label = JSON.stringify(branch);
      expect(expandVariables("git switch {{branch}}", { branch }, "cmd"), label).toEqual(
        refusedFor("branch"),
      );
      expect(expandVariables('git switch "{{branch}}"', { branch }, "cmd"), label).toEqual(
        refusedFor("branch"),
      );
    }
    expect(expandVariables("git switch {{branch}}", { branch: "x;y&z|w" }, "cmd")).toEqual(
      filled('git switch "x;y&z|w"'),
    );
  });

  it("refuses a value that would end or break open the snippet's own quotes", () => {
    const refused = (text: string, branch: string, kind: ShellKind) => {
      const label = `${kind}: ${text} ${JSON.stringify(branch)}`;
      expect(expandVariables(text, { branch }, kind), label).toEqual(refusedFor("branch"));
    };
    for (const value of ["x'y", "x\\", "x!y"]) refused("echo '{{branch}}'", value, "posix");
    for (const value of ['x"y', "x$(marker)", "x`y`", "x\\", "x!y"]) {
      refused('echo "{{branch}}"', value, "posix");
    }
    for (const value of ["x'y", "x\u2018y", "x\u201by"]) {
      refused("echo '{{branch}}'", value, "powershell");
    }
    for (const value of ['x"y', "x\u201cy", "x$(marker)", "x`y"]) {
      refused('echo "{{branch}}"', value, "powershell");
    }
    // Inside quotes, what does not end them is fine.
    expect(expandVariables("echo '{{branch}}'", { branch: 'a "b" $c `d`' }, "posix")).toEqual(
      filled("echo 'a \"b\" $c `d`'"),
    );
    expect(expandVariables('echo "{{branch}}"', { branch: "a 'b' (c) & d" }, "powershell")).toEqual(
      filled("echo \"a 'b' (c) & d\""),
    );
  });

  it("reads the snippet's quoting with the shell's escapes", () => {
    const branch = { branch: "a;b" };
    // An escaped quote opens nothing: the variable is outside quotes and gets Hangar's.
    expect(expandVariables('echo \\" {{branch}}', branch, "posix")).toEqual(
      filled("echo \\\" 'a;b'"),
    );
    expect(expandVariables('echo `" {{branch}}', branch, "powershell")).toEqual(
      filled("echo `\" 'a;b'"),
    );
    // cmd's escaped quote opens a string for a PowerShell behind it.
    expect(expandVariables('echo ^" {{branch}}', branch, "cmd")).toEqual(refusedFor("branch"));
    // A single quote means nothing to cmd, but a PowerShell behind it reads one.
    expect(expandVariables("echo '{{branch}}'", branch, "cmd")).toEqual(filled("echo '\"a;b\"'"));
    // Doubled quotes stay inside the string.
    expect(expandVariables("echo 'it''s {{branch}}'", branch, "powershell")).toEqual(
      filled("echo 'it''s a;b'"),
    );
    // `$(` outside quotes reads like the top level.
    expect(expandVariables("cd $(dirname {{folder}})", { folder: "/a b/c" }, "posix")).toEqual(
      filled("cd $(dirname '/a b/c')"),
    );
  });

  it("lets only plain values through where the snippet's quoting cannot be read", () => {
    const places: [string, ShellKind][] = [
      ['echo "$(cat {{branch}})"', "posix"],
      ["echo `cat {{branch}}`", "posix"],
      ["echo ${x:-{{branch}}}", "posix"],
      ["echo $'{{branch}}'", "posix"],
      ["cat <<END\n{{branch}}\nEND", "posix"],
      ["echo \\{{branch}}", "posix"],
      ["echo ${{branch}}", "posix"],
      ["# don't\necho {{branch}}", "posix"],
      ['echo "$({{branch}})"', "powershell"],
      ["echo @'\n{{branch}}\n'@", "powershell"],
      ["cmd --% {{branch}}", "powershell"],
      ["echo `{{branch}}", "powershell"],
      ["# it's\necho {{branch}}", "powershell"],
      ["echo ^{{branch}}", "cmd"],
      ["echo '\"'{{branch}}", "cmd"],
    ];
    for (const [text, kind] of places) {
      const label = `${kind}: ${JSON.stringify(text)}`;
      expect(expandVariables(text, { branch: "x;marker" }, kind), label).toEqual(
        refusedFor("branch"),
      );
      expect(expandVariables(text, { branch: "feature/x 1" }, kind), label).toMatchObject({
        ok: true,
      });
    }
    // A backslash escapes in POSIX shells, so it is not plain there.
    expect(expandVariables('echo "$(ls {{folder}})"', { folder: "/x\\y" }, "posix")).toEqual(
      refusedFor("folder"),
    );
  });

  it("names a missing value before a risky one", () => {
    expect(expandVariables("{{branch}} '{{diff}}'", { branch: "x'y" }, "posix")).toEqual({
      ok: false,
      missing: ["diff"],
    });
  });

  it("names every risky variable once, in order of use", () => {
    expect(
      expandVariables(
        "'{{selection}}' '{{branch}}' {{selection}}",
        { selection: "a'b", branch: "c'd" },
        "posix",
      ),
    ).toEqual({ ok: false, unsafe: ["selection", "branch"] });
  });

  it("leaves unknown names, and values holding braces, as they are", () => {
    expect(expandVariables("{{name}} {{branch}}", { branch: "{{folder}}" }, "posix")).toEqual(
      filled("{{name}} '{{folder}}'"),
    );
  });
});

describe("fileValue", () => {
  it("mentions the file for an agent", () => {
    expect(fileValue("src/app.ts", "agent")).toBe("@src/app.ts");
    expect(fileValue("docs/my notes.md", "agent")).toBe('@"docs/my notes.md"');
  });

  it("gives a shell the path in its own form, unquoted (expandVariables quotes)", () => {
    expect(fileValue("src/app.ts", "posix")).toBe("src/app.ts");
    expect(fileValue("src/app.ts", "cmd")).toBe("src\\app.ts");
    expect(fileValue("src/app.ts", "powershell")).toBe("src\\app.ts");
    expect(fileValue("-rf.txt", "posix")).toBe("./-rf.txt");
  });
});

describe("unsafeSummary", () => {
  it("lists the variables", () => {
    expect(unsafeSummary(["branch", "folder"])).toBe("{{branch}}, {{folder}}");
  });
});

describe("capDiff", () => {
  const note = (count: number) => `[${String(count)} lines left out]`;

  it("keeps a diff that fits, without trailing space", () => {
    expect(capDiff("a\nb\n\n", note)).toBe("a\nb");
  });

  it("cuts after the last full line and names the lines left out", () => {
    const diff = ["line 1", "line 2", "line 3", "line 4"].join("\n");
    expect(capDiff(diff, note, 15)).toBe("line 1\nline 2\n[2 lines left out]");
  });

  it("stays within the default limit plus the note", () => {
    const diff = Array.from({ length: 5000 }, (_, index) => `+ line ${String(index)}`).join("\n");
    const capped = capDiff(diff, note);
    expect(capped.length).toBeLessThanOrEqual(DIFF_MAX_CHARS + 40);
    expect(capped.endsWith("lines left out]")).toBe(true);
  });
});

describe("missingSummary", () => {
  it("names each missing variable with its reason", () => {
    expect(missingSummary(["file", "diff"], (name) => `no ${name}`)).toBe(
      "{{file}}: no file, {{diff}}: no diff",
    );
  });
});

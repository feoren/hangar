import { describe, expect, it } from "vitest";

import type { Profile } from "./api";
import {
  emptyForm,
  formFromProfile,
  isAbsolutePath,
  joinArgs,
  looksSecret,
  newProfileId,
  parseProfileEnv,
  parseSecretStatuses,
  splitArgs,
  toInput,
  validateForm,
  type ProfileForm,
} from "./profileForm";

const profile: Profile = {
  id: "work",
  name: "Work shell",
  command: "pwsh",
  args: ["-NoLogo", "-File", "C:\\My Scripts\\start.ps1"],
  cwd: "C:\\src",
  icon: null,
  source: "user",
  installed: true,
  homepage: null,
  installHint: null,
  editable: true,
  env: ["EDITOR"],
  secretEnv: ["OPENAI_API_KEY"],
};

function form(patch: Partial<ProfileForm> = {}): ProfileForm {
  return { ...emptyForm("x"), name: "X", command: "x", ...patch };
}

describe("splitArgs", () => {
  it("splits on spaces and keeps quoted parts together", () => {
    expect(splitArgs(`--model opus "C:\\Program Files\\x" 'it''s'`)).toEqual({
      ok: true,
      args: ["--model", "opus", "C:\\Program Files\\x", "its"],
    });
    expect(splitArgs("  ")).toEqual({ ok: true, args: [] });
    expect(splitArgs(`a "" b`)).toEqual({ ok: true, args: ["a", "", "b"] });
  });

  it('knows only \\" and \\\\ inside double quotes, so Windows paths stay as typed', () => {
    expect(splitArgs(`"say \\"hi\\"" C:\\tools\\x.exe`)).toEqual({
      ok: true,
      args: ['say "hi"', "C:\\tools\\x.exe"],
    });
  });

  it("refuses an open quote", () => {
    expect(splitArgs(`"open`)).toEqual({ ok: false });
    expect(splitArgs(`it's`)).toEqual({ ok: false });
  });

  it("round-trips through joinArgs", () => {
    const cases = [
      [],
      ["plain"],
      ["with space"],
      [""],
      ["it's"],
      ['say "hi"'],
      ["back\\slash\\", "C:\\dir with space\\"],
      ["both ' and \""],
    ];
    for (const args of cases) {
      expect(splitArgs(joinArgs(args))).toEqual({ ok: true, args });
    }
  });
});

describe("isAbsolutePath", () => {
  it("accepts drive, UNC and POSIX paths", () => {
    expect(isAbsolutePath("C:\\src")).toBe(true);
    expect(isAbsolutePath("d:/src")).toBe(true);
    expect(isAbsolutePath("\\\\server\\share")).toBe(true);
    expect(isAbsolutePath("/home/ada")).toBe(true);
    expect(isAbsolutePath("src")).toBe(false);
    expect(isAbsolutePath("C:src")).toBe(false);
  });
});

describe("looksSecret", () => {
  it("spots typical credential names", () => {
    for (const name of ["OPENAI_API_KEY", "GITHUB_TOKEN", "DB_PASSWORD", "AWS_SECRET_ACCESS_KEY"]) {
      expect(looksSecret(name), name).toBe(true);
    }
    for (const name of ["EDITOR", "KEYBOARD_LAYOUT", "PATH", "MONKEY"]) {
      expect(looksSecret(name), name).toBe(false);
    }
  });
});

describe("newProfileId", () => {
  it("derives a lowercase id and avoids taken ones regardless of case", () => {
    expect(newProfileId("My Agent", [])).toBe("my-agent");
    expect(newProfileId("Ärger Tool", [])).toBe("arger-tool");
    expect(newProfileId("My Agent", ["MY-AGENT", "my-agent-2"])).toBe("my-agent-3");
    expect(newProfileId("!!!", [])).toBe("program");
  });
});

describe("formFromProfile and toInput", () => {
  it("round-trips an editable profile", () => {
    const loaded = formFromProfile(profile, { EDITOR: "code" });
    expect(loaded.isNew).toBe(false);
    expect(loaded.args).toBe(`-NoLogo -File 'C:\\My Scripts\\start.ps1'`);
    expect(validateForm(loaded)).toEqual([]);
    expect(toInput(loaded)).toEqual({
      id: "work",
      name: "Work shell",
      command: "pwsh",
      args: profile.args,
      cwd: "C:\\src",
      env: { EDITOR: "code" },
      secretEnv: ["OPENAI_API_KEY"],
    });
  });

  it("opens a built-in entry as new, so saving creates the user's own copy", () => {
    const catalog = { ...profile, editable: false, env: [], secretEnv: [] };
    expect(formFromProfile(catalog, {}).isNew).toBe(true);
  });

  it("leaves out empty rows and trims names", () => {
    const input = toInput(
      form({
        cwd: "  ",
        env: [
          { key: "a", name: " A ", value: "1" },
          { key: "b", name: "", value: "" },
        ],
        secrets: [
          { key: "c", name: " TOKEN " },
          { key: "d", name: "" },
        ],
      }),
    );
    expect(input).toMatchObject({ cwd: null, env: { A: "1" }, secretEnv: ["TOKEN"] });
  });
});

describe("validateForm", () => {
  it("points at the fields to fix", () => {
    const problems = validateForm(
      form({
        name: " ",
        command: "",
        args: `"open`,
        cwd: "relative",
        env: [
          { key: "a", name: "A=B", value: "" },
          { key: "b", name: "SAME", value: "1" },
          { key: "c", name: "SAME", value: "2" },
          { key: "d", name: "TOKEN", value: "x" },
        ],
        secrets: [
          { key: "e", name: "TOKEN" },
          { key: "f", name: "1BAD" },
          { key: "g", name: "TOKEN" },
        ],
      }),
    );
    expect(problems.map((p) => [p.field, p.message])).toEqual([
      ["name", "profile.nameMissing"],
      ["command", "profile.commandMissing"],
      ["args", "profile.argsQuote"],
      ["cwd", "profile.cwdRelative"],
      ["env:a", "profile.envName"],
      ["env:c", "profile.duplicateName"],
      ["env:d", "profile.alsoSecret"],
      ["secret:f", "profile.secretName"],
      ["secret:g", "profile.duplicateName"],
    ]);
  });

  it("accepts a minimal program", () => {
    expect(validateForm(form())).toEqual([]);
  });
});

describe("backend answers", () => {
  it("reads env values and secret states, dropping malformed parts", () => {
    expect(parseProfileEnv({ A: "1", B: 2 })).toEqual({ A: "1" });
    expect(parseProfileEnv(null)).toEqual({});
    const states = parseSecretStatuses([
      { name: "TOKEN", stored: true },
      { name: "KEY", stored: "no" },
      "x",
    ]);
    expect([...states]).toEqual([["TOKEN", true]]);
    expect(parseSecretStatuses({}).size).toBe(0);
  });
});

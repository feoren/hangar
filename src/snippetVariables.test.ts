import { describe, expect, it } from "vitest";

import {
  capDiff,
  DIFF_MAX_CHARS,
  expandVariables,
  hasVariables,
  missingSummary,
  usedVariables,
} from "./snippetVariables";

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

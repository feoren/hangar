import { describe, expect, it } from "vitest";

import { errorText } from "./api";
import { backendErrorText, splitBackendError } from "./backendError";
import { t } from "./i18n";
import { InputStalled } from "./sessionInput";

describe("backendErrorText", () => {
  it("shows a network folder in the window's own words", () => {
    expect(
      backendErrorText(
        "[remote_cwd] Working folder '\\\\server\\share' is a network share or device path",
      ),
    ).toBe(t("terminal.remoteFolder"));
  });

  it("shows a missing folder in the window's own words", () => {
    expect(backendErrorText("[bad_cwd] Working folder '/gone' does not exist")).toBe(
      t("terminal.missingFolder"),
    );
  });

  it("keeps the text of an unknown code, without the code", () => {
    expect(backendErrorText("[later_kind] Something new")).toBe("Something new");
  });

  it("shows neither the detail of an unknown code nor that of a known one", () => {
    expect(backendErrorText("[later_kind] Something new\nC:\\some\\path")).toBe("Something new");
    expect(backendErrorText("[bad_cwd] Working folder does not exist\n/gone")).toBe(
      t("terminal.missingFolder"),
    );
  });

  it("leaves plain errors alone", () => {
    expect(backendErrorText("Unknown session 3")).toBe("Unknown session 3");
    expect(backendErrorText("[not a code] text")).toBe("[not a code] text");
  });
});

describe("errorText", () => {
  it("translates codes from the backend", () => {
    expect(errorText("[remote_cwd] Working folder 'x' is a network share")).toBe(
      t("terminal.remoteFolder"),
    );
  });

  it("names a stalled input in the window's own words", () => {
    expect(errorText(new InputStalled())).toBe(t("terminal.inputStalled"));
  });
});

describe("splitBackendError", () => {
  it("takes code, text and detail apart at the first line break", () => {
    expect(splitBackendError("[snapshot_linked] A path leads through a link\nsrc/a\nb")).toEqual({
      code: "snapshot_linked",
      text: "A path leads through a link",
      detail: "src/a\nb",
    });
    expect(splitBackendError("[snapshot_working] A session is working")).toEqual({
      code: "snapshot_working",
      text: "A session is working",
      detail: null,
    });
    expect(splitBackendError("[snapshot_symlink] Symbolic link\n")).toEqual({
      code: "snapshot_symlink",
      text: "Symbolic link",
      detail: "",
    });
  });

  it("keeps a message without a code whole", () => {
    expect(splitBackendError("fatal: one\ntwo")).toEqual({
      code: null,
      text: "fatal: one\ntwo",
      detail: null,
    });
    expect(splitBackendError("[Not_a_code] x")).toEqual({
      code: null,
      text: "[Not_a_code] x",
      detail: null,
    });
  });
});

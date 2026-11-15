import { describe, expect, it } from "vitest";

import { errorText } from "./api";
import { backendErrorText } from "./backendError";
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

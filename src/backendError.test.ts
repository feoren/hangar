import { describe, expect, it } from "vitest";

import { errorText } from "./api";
import { backendErrorText, splitBackendError } from "./backendError";
import { t, type MessageKey } from "./i18n";
import { InputStalled } from "./sessionInput";

describe("backendErrorText", () => {
  it("shows a network folder in the window's own words", () => {
    expect(
      backendErrorText(
        "[remote_cwd] Working folder '\\\\server\\share' is a network share or device path",
      ),
    ).toBe(t("terminal.remoteFolder"));
  });

  it("shows the file viewer limit in the window's own words", () => {
    // `FileViewerError::TooManyWindows` (file_viewer.rs).
    expect(
      backendErrorText(
        "[viewer_limit] At most 10 file viewers can be open at once; close one first",
      ),
    ).toBe(t("files.tooManyViewers"));
  });

  it("shows a missing folder in the window's own words", () => {
    expect(backendErrorText("[bad_cwd] Working folder '/gone' does not exist")).toBe(
      t("terminal.missingFolder"),
    );
  });

  it("shows partner project errors in the window's own words", () => {
    // The codes of `PartnerError` (partners.rs).
    const codes: [string, MessageKey][] = [
      ["partner_name", "partnerError.name"],
      ["partner_folder", "partnerError.folder"],
      ["partner_missing_folder", "partnerError.missingFolder"],
      ["partner_self", "partnerError.self"],
      ["partner_nested", "partnerError.nested"],
      ["partner_name_taken", "partnerError.nameTaken"],
      ["partner_name_in_use", "partnerError.nameInUse"],
      ["partner_already", "partnerError.already"],
      ["partner_too_many", "partnerError.tooMany"],
      ["partner_not_partner", "partnerError.notPartner"],
      ["partner_no_project", "partnerError.noProject"],
      ["partner_wrong_target", "partnerError.wrongTarget"],
      ["partner_empty", "partnerError.empty"],
      ["partner_too_long", "partnerError.tooLong"],
      ["partner_forged_header", "partnerError.forgedHeader"],
      ["partner_invalid_list", "partnerError.invalidList"],
      ["partner_full", "partnerError.full"],
      ["partner_message_gone", "partnerError.messageGone"],
      ["partner_pair_full", "partnerError.pairFull"],
      ["partner_shell_sender", "partnerError.shellSender"],
    ];
    for (const [code, key] of codes) {
      const text = backendErrorText(`[${code}] English text`);
      expect(text, code).toBe(t(key));
      expect(text, code).not.toBe("English text");
    }
    expect(
      errorText('[partner_name_in_use] The short name "web" already names another project'),
    ).toBe(t("partnerError.nameInUse"));
  });

  it("shows link group errors in the window's own words", () => {
    // The codes of `LinkError` (links.rs) the window can meet; the rest only reach agents.
    const codes: [string, MessageKey][] = [
      ["link_too_few", "linkError.tooFew"],
      ["link_too_many", "linkError.tooMany"],
      ["link_twice", "linkError.twice"],
      ["link_already", "linkError.already"],
      ["link_groups_full", "linkError.groupsFull"],
      ["link_not_linkable", "linkError.notLinkable"],
      ["link_unknown_group", "linkError.unknownGroup"],
      ["link_not_member", "linkError.notMember"],
      ["link_name", "links.nameInvalid"],
      ["link_name_reserved", "links.nameReserved"],
      ["link_name_taken", "links.nameTwice"],
      ["link_message_gone", "partnerError.messageGone"],
      ["link_wrong_target", "linkError.wrongTarget"],
      ["link_claimed", "linkError.claimed"],
    ];
    for (const [code, key] of codes) {
      const text = backendErrorText(`[${code}] English text`);
      expect(text, code).toBe(t(key));
      expect(text, code).not.toBe("English text");
    }
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

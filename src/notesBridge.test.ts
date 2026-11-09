import { describe, expect, it } from "vitest";

import {
  DEFAULT_WINDOW_PREFS,
  EMPTY_CONTEXT,
  isSendable,
  parseNotesCommand,
  parseNotesContext,
  parseNotesMessage,
  parseWindowPrefs,
  sendableText,
} from "./notesBridge";

describe("parseNotesMessage", () => {
  it("accepts the three kinds", () => {
    expect(parseNotesMessage({ kind: "sendToTerminal", text: "hi\n", sessionId: 3 })).toEqual({
      kind: "sendToTerminal",
      text: "hi\n",
      sessionId: 3,
    });
    expect(parseNotesMessage({ kind: "sendToTerminal", text: "hi" })).toEqual({
      kind: "sendToTerminal",
      text: "hi",
      sessionId: null,
    });
    expect(parseNotesMessage({ kind: "notesChanged", folderKey: "general" })).toEqual({
      kind: "notesChanged",
      folderKey: "general",
    });
    expect(parseNotesMessage({ kind: "showMain" })).toEqual({ kind: "showMain" });
  });

  it("rejects anything the backend would not send", () => {
    expect(parseNotesMessage({ kind: "sendToTerminal", text: "" })).toBeNull();
    expect(parseNotesMessage({ kind: "sendToTerminal", text: "a\u001b[201~b" })).toBeNull();
    expect(parseNotesMessage({ kind: "sendToTerminal", text: "a", sessionId: 1.5 })).toBeNull();
    expect(parseNotesMessage({ kind: "notesChanged", folderKey: "../x" })).toBeNull();
    expect(parseNotesMessage({ kind: "trayQuit" })).toBeNull();
    expect(parseNotesMessage("showMain")).toBeNull();
    expect(parseNotesMessage(null)).toBeNull();
  });
});

describe("sendable text", () => {
  it("allows tab and line breaks only", () => {
    expect(isSendable("a\tb\r\nc")).toBe(true);
    expect(isSendable("a\u0007")).toBe(false);
    expect(isSendable("a\u009b")).toBe(false);
    expect(isSendable("")).toBe(false);
    expect(sendableText("a\u001b[31mb\tc\n")).toBe("a[31mb\tc\n");
  });
});

describe("parseNotesCommand", () => {
  it("accepts known commands", () => {
    expect(parseNotesCommand({ kind: "quickNote" })).toEqual({ kind: "quickNote" });
    expect(parseNotesCommand({ kind: "close" })).toEqual({ kind: "close" });
    expect(parseNotesCommand({ kind: "capture", text: "x", source: "Claude" })).toEqual({
      kind: "capture",
      text: "x",
      source: "Claude",
    });
  });

  it("rejects broken ones", () => {
    expect(parseNotesCommand({ kind: "capture", text: "", source: "" })).toBeNull();
    expect(parseNotesCommand({ kind: "eval" })).toBeNull();
    expect(parseNotesCommand([])).toBeNull();
  });
});

describe("context and window prefs", () => {
  it("reads the session context defensively", () => {
    expect(
      parseNotesContext(
        JSON.stringify({
          sessionFolder: "C:\\p",
          activeId: 2,
          sessions: [{ id: 2, title: "Claude" }, { id: "x" }, null],
        }),
      ),
    ).toEqual({ sessionFolder: "C:\\p", activeId: 2, sessions: [{ id: 2, title: "Claude" }] });
    expect(parseNotesContext("{")).toEqual(EMPTY_CONTEXT);
    expect(parseNotesContext(null)).toEqual(EMPTY_CONTEXT);
  });

  it("reads the window prefs with defaults", () => {
    expect(parseWindowPrefs(JSON.stringify({ detached: true, onTop: true }))).toEqual({
      detached: true,
      onTop: true,
    });
    expect(parseWindowPrefs(JSON.stringify({ detached: "yes" }))).toEqual(DEFAULT_WINDOW_PREFS);
    expect(parseWindowPrefs("[")).toEqual(DEFAULT_WINDOW_PREFS);
  });
});

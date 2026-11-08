import { describe, expect, it } from "vitest";

import { parseActivatedSession } from "./api";
import { notificationText, shouldNotify } from "./desktopNotify";
import { createTranslator } from "./i18n";

describe("shouldNotify", () => {
  it("notifies only on attention, with the setting on, in the background", () => {
    expect(shouldNotify("attention", { enabled: true, windowFocused: false })).toBe(true);
    expect(shouldNotify("attention", { enabled: true, windowFocused: true })).toBe(false);
    expect(shouldNotify("attention", { enabled: false, windowFocused: false })).toBe(false);
    expect(shouldNotify("working", { enabled: true, windowFocused: false })).toBe(false);
    expect(shouldNotify("idle", { enabled: true, windowFocused: false })).toBe(false);
  });
});

describe("notificationText", () => {
  const t = createTranslator("en");
  const session = { title: "Claude Code", cwd: "C:\\src\\hangar" };

  it("says the session is waiting while its process runs", () => {
    expect(notificationText(session, false, t)).toEqual({
      title: "Claude Code is waiting for you",
      body: "src/hangar",
    });
  });

  it("says the session has finished after an exit", () => {
    expect(notificationText(session, true, t).title).toBe("Claude Code has finished");
  });

  it("uses the UI language", () => {
    expect(notificationText(session, false, createTranslator("de")).title).toBe(
      "Claude Code wartet auf dich",
    );
  });
});

describe("parseActivatedSession", () => {
  it("reads the session ID", () => {
    expect(parseActivatedSession({ sessionId: 4 })).toBe(4);
  });

  it("rejects malformed payloads", () => {
    expect(parseActivatedSession(null)).toBeNull();
    expect(parseActivatedSession({ sessionId: "4" })).toBeNull();
    expect(parseActivatedSession({ sessionId: 1.5 })).toBeNull();
    expect(parseActivatedSession(4)).toBeNull();
  });
});

import { describe, expect, it } from "vitest";

import { editorKeyAction, editorPosition, type EditorKey } from "./queueEditor";

const key = (patch: Partial<EditorKey>): EditorKey => ({
  key: "Enter",
  shiftKey: false,
  ctrlKey: false,
  altKey: false,
  metaKey: false,
  isComposing: false,
  ...patch,
});

describe("editorKeyAction", () => {
  it("queues on Enter and on Ctrl+Enter", () => {
    expect(editorKeyAction(key({}))).toBe("save");
    expect(editorKeyAction(key({ ctrlKey: true }))).toBe("save");
  });

  it("leaves Shift+Enter, composition and other keys to the textarea", () => {
    expect(editorKeyAction(key({ shiftKey: true }))).toBe("type");
    expect(editorKeyAction(key({ isComposing: true }))).toBe("type");
    expect(editorKeyAction(key({ key: "a" }))).toBe("type");
    expect(editorKeyAction(key({ key: "Escape" }))).toBe("type");
  });
});

describe("editorPosition", () => {
  const size = { width: 380, height: 240 };
  const viewport = { width: 1200, height: 800 };

  it("sits where the menu was", () => {
    expect(editorPosition({ x: 120, y: 200 }, size, viewport)).toEqual({ left: 120, top: 200 });
  });

  it("moves in to stay inside the window", () => {
    expect(editorPosition({ x: 1100, y: 760 }, size, viewport)).toEqual({
      left: 1200 - 380 - 16,
      top: 800 - 240 - 16,
    });
    expect(editorPosition({ x: -5, y: 0 }, size, viewport)).toEqual({ left: 16, top: 16 });
  });

  it("is centered without an anchor", () => {
    expect(editorPosition(null, size, viewport)).toBeNull();
  });
});

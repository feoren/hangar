// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { AutoGrowTextarea } from "./AutoGrowTextarea";
import { fitTextareaHeight } from "./autoGrow";

/** happy-dom does no layout: content height comes from the line count, 20px a line. */
const LINE = 20;
const BORDER = 2;

function px(lines: number): string {
  return `${String(lines * LINE + BORDER)}px`;
}

function fakeLayout(field: HTMLTextAreaElement): void {
  Object.defineProperty(field, "scrollHeight", {
    configurable: true,
    get: () => field.value.split("\n").length * LINE,
  });
  Object.defineProperty(field, "clientHeight", {
    configurable: true,
    get: () => (Number.parseFloat(field.style.height) || 0) - BORDER,
  });
  Object.defineProperty(field, "offsetHeight", {
    configurable: true,
    get: () => Number.parseFloat(field.style.height) || 0,
  });
}

describe("fitTextareaHeight", () => {
  it("fits the content plus the border", () => {
    const field = document.createElement("textarea");
    fakeLayout(field);
    field.value = "a\nb\nc";
    fitTextareaHeight(field);
    expect(field.style.height).toBe(px(3));
  });

  it("shrinks again when lines are removed", () => {
    const field = document.createElement("textarea");
    fakeLayout(field);
    field.value = "a\nb\nc\nd";
    fitTextareaHeight(field);
    field.value = "a";
    fitTextareaHeight(field);
    expect(field.style.height).toBe(px(1));
  });

  it("never goes below the dragged height", () => {
    const field = document.createElement("textarea");
    fakeLayout(field);
    field.value = "a";
    fitTextareaHeight(field, 150);
    expect(field.style.height).toBe("150px");
  });

  it("keeps the scroll position of a scrolled container", () => {
    const box = document.createElement("div");
    const field = document.createElement("textarea");
    box.append(field);
    fakeLayout(field);
    box.scrollTop = 80;
    field.value = "a";
    fitTextareaHeight(field);
    expect(box.scrollTop).toBe(80);
  });
});

describe("AutoGrowTextarea", () => {
  let host: HTMLDivElement;
  let root: Root | null = null;
  let field: HTMLTextAreaElement | null = null;

  function render(value: string) {
    act(() => {
      root ??= createRoot(host);
      root.render(
        <AutoGrowTextarea
          aria-label="Text"
          value={value}
          onChange={() => undefined}
          ref={(element) => {
            if (element !== null && element !== field) fakeLayout(element);
            field = element;
          }}
        />,
      );
    });
  }

  function textarea(): HTMLTextAreaElement {
    if (field === null) throw new Error("textarea missing");
    return field;
  }

  beforeEach(() => {
    host = document.createElement("div");
    document.body.append(host);
  });

  afterEach(() => {
    act(() => root?.unmount());
    root = null;
    field = null;
    host.remove();
  });

  it("grows with the text", () => {
    render("one");
    expect(textarea().style.height).toBe(px(1));
    render("one\ntwo\nthree\nfour");
    expect(textarea().style.height).toBe(px(4));
  });

  it("keeps a height dragged by hand until the text needs more", () => {
    render("one");
    const element = textarea();
    act(() => {
      element.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    });
    // The resize corner changes the inline height while dragging.
    element.style.height = "200px";
    act(() => {
      window.dispatchEvent(new PointerEvent("pointerup"));
    });
    render("one\ntwo");
    expect(element.style.height).toBe("200px");
    render(Array.from({ length: 12 }, (_, i) => `line ${String(i)}`).join("\n"));
    expect(element.style.height).toBe(px(12));
  });

  it("does not take a click into the text for a drag", () => {
    render("one\ntwo\nthree");
    const element = textarea();
    act(() => {
      element.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      window.dispatchEvent(new PointerEvent("pointerup"));
    });
    render("one");
    expect(element.style.height).toBe(px(1));
  });
});

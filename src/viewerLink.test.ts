import { describe, expect, it } from "vitest";

import { renderViewerMarkdown } from "./markdownPreview";
import { viewerLinkAction } from "./viewerLink";

/** The address and visible text of the first link, as the viewer reads them after a click. */
function firstLink(markdown: string): { href: string; shown: string } {
  const match = /data-href="([^"]*)"[^>]*>([^<]*)</u.exec(renderViewerMarkdown(markdown));
  if (match === null) throw new Error("no link rendered");
  const decode = (text: string) =>
    text
      .replace(/&amp;/g, "&")
      .replace(/&quot;/g, '"')
      .replace(/&lt;/g, "<");
  return { href: decode(match[1] ?? ""), shown: decode(match[2] ?? "") };
}

describe("viewerLinkAction", () => {
  it("asks before a Markdown link whose text names another host", () => {
    const { href, shown } = firstLink("[github.com/acme](https://evil.example/x)");
    expect(viewerLinkAction(href, shown)).toEqual({
      kind: "confirm",
      url: "https://evil.example/x",
    });
    const sentence = firstLink("[see https://github.com/acme.](https://evil.example/x)");
    expect(viewerLinkAction(sentence.href, sentence.shown).kind).toBe("confirm");
  });

  it("asks before a link whose text is a file name of another target", () => {
    const { href, shown } = firstLink("[install.sh](https://evil.example/x)");
    expect(viewerLinkAction(href, shown).kind).toBe("confirm");
  });

  it("opens plain words and addresses that are what they show", () => {
    const words = firstLink("[the docs](https://example.com/docs)");
    expect(viewerLinkAction(words.href, words.shown)).toEqual({
      kind: "open",
      url: "https://example.com/docs",
    });
    const bare = firstLink("See https://tauri.app/start for more.");
    expect(viewerLinkAction(bare.href, bare.shown).kind).toBe("open");
  });

  it("opens nothing that is not a plain http(s) address", () => {
    expect(viewerLinkAction("https://github.com@evil.example/", "github.com")).toEqual({
      kind: "ignore",
    });
    expect(viewerLinkAction("javascript:alert(1)", "x")).toEqual({ kind: "ignore" });
  });
});

import { describe, expect, it } from "vitest";

import { renderViewerMarkdown } from "./markdownPreview";
import { viewerLinkAction, wordAround } from "./viewerLink";

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

/**
 * What a click on the `index`th link of the rendered Markdown reads, as the viewer builds it: the
 * text of the link's block (its first paragraph here) with the link's place in it, widened to the
 * whitespace around. Tags are dropped like `textContent` drops them.
 */
function clickedShown(markdown: string, index: number): { href: string; shown: string } {
  const html = renderViewerMarkdown(markdown);
  const block = /<p>(.*?)<\/p>/su.exec(html)?.[1] ?? "";
  const decode = (text: string) =>
    text
      .replace(/&amp;/g, "&")
      .replace(/&quot;/g, '"')
      .replace(/&lt;/g, "<")
      .replace(/&gt;/g, ">");
  let text = "";
  let links = 0;
  let depth = 0;
  let start = -1;
  let end = -1;
  let href = "";
  for (const token of block.matchAll(/<[^>]*>|[^<]+/gu)) {
    const part = token[0];
    if (!part.startsWith("<")) {
      text += decode(part);
      continue;
    }
    const opened = /data-href="([^"]*)"/u.exec(part);
    if (opened !== null) {
      if (links === index) {
        start = text.length;
        href = decode(opened[1] ?? "");
        depth = 1;
      }
      links += 1;
    } else if (depth > 0 && part.startsWith("<span")) depth += 1;
    else if (depth > 0 && part === "</span>") {
      depth -= 1;
      if (depth === 0) end = text.length;
    }
  }
  if (start < 0 || end < start) throw new Error("no such link");
  return { href, shown: wordAround(text, start, end) };
}

describe("wordAround", () => {
  it("widens the link to the whitespace around it", () => {
    expect(wordAround("see github.com/acme now", 15, 19)).toBe("github.com/acme");
    expect(wordAround("acme", 0, 4)).toBe("acme");
    expect(wordAround("a\nb.c/acme\td", 6, 10)).toBe("b.c/acme");
  });

  it("keeps the spaces inside a link of several words", () => {
    expect(wordAround("x Open the pull request.", 2, 23)).toBe("Open the pull request.");
  });
});

describe("the viewer judges a link with the text around it", () => {
  it("asks when plain text before the link names another host", () => {
    const { href, shown } = clickedShown("github.com/[acme](https://evil.example/)", 0);
    expect(shown).toBe("github.com/acme");
    expect(viewerLinkAction(href, shown).kind).toBe("confirm");
  });

  it("asks when a neighbouring link names another host", () => {
    const markdown = "[github.com/](https://github.com/)[acme](https://evil.example/)";
    const second = clickedShown(markdown, 1);
    expect(second.shown).toBe("github.com/acme");
    expect(viewerLinkAction(second.href, second.shown).kind).toBe("confirm");
    const first = clickedShown(markdown, 0);
    expect(viewerLinkAction(first.href, first.shown).kind).toBe("open");
  });

  it("asks when code before the link names another host", () => {
    const { href, shown } = clickedShown("`github.com/`[acme](https://evil.example/)", 0);
    expect(viewerLinkAction(href, shown).kind).toBe("confirm");
  });

  it("asks when the link's own text names another host, whatever the text around it says", () => {
    const markdown = "evil.example/[github.com](https://evil.example/)";
    const { href, shown } = clickedShown(markdown, 0);
    expect(shown).toBe("evil.example/github.com");
    expect(viewerLinkAction(href, shown).kind).toBe("open");
    expect(viewerLinkAction(href, shown, "github.com").kind).toBe("confirm");
  });

  it("opens a link inside a sentence that names no host", () => {
    const { href, shown } = clickedShown("Read [the docs](https://example.com/docs) first.", 0);
    expect(shown).toBe("the docs");
    expect(viewerLinkAction(href, shown).kind).toBe("open");
  });
});

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

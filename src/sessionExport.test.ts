import { describe, expect, it } from "vitest";

import { bufferText, exportEol, exportFileName, htmlDocument, utf8Bytes } from "./sessionExport";

describe("exportEol", () => {
  it("uses CRLF on Windows and LF elsewhere", () => {
    expect(
      exportEol("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/130.0"),
    ).toBe("\r\n");
    expect(exportEol("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15")).toBe(
      "\n",
    );
    expect(exportEol("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15")).toBe("\n");
    expect(exportEol("")).toBe("\n");
  });
});

describe("bufferText", () => {
  it("joins wrapped rows and keeps real line breaks", () => {
    const rows = [
      { text: "first part ", wrapped: false },
      { text: "continues", wrapped: true },
      { text: "second", wrapped: false },
    ];
    expect(bufferText(rows, "\n")).toBe("first part continues\nsecond\n");
  });

  it("trims trailing spaces and empty lines at the end", () => {
    const rows = [
      { text: "a   ", wrapped: false },
      { text: "", wrapped: false },
      { text: "b", wrapped: false },
      { text: "   ", wrapped: false },
      { text: "", wrapped: false },
    ];
    expect(bufferText(rows, "\r\n")).toBe("a\r\n\r\nb\r\n");
  });

  it("is empty for an empty buffer", () => {
    expect(bufferText([], "\n")).toBe("");
    expect(bufferText([{ text: "  ", wrapped: false }], "\n")).toBe("");
  });

  it("treats a wrapped first row as a line of its own", () => {
    expect(bufferText([{ text: "x", wrapped: true }], "\n")).toBe("x\n");
  });
});

describe("exportFileName", () => {
  const date = new Date(2026, 10, 8, 9, 5);

  it("combines title, date and extension", () => {
    expect(exportFileName("Claude Code", date, "text")).toBe("Claude Code 2026-11-08 09-05.txt");
    expect(exportFileName("PowerShell 7 (2)", date, "html")).toBe(
      "PowerShell 7 (2) 2026-11-08 09-05.html",
    );
  });

  it("removes characters that file systems forbid", () => {
    expect(exportFileName('a<b>c:d"e/f\\g|h?i*j', date, "text")).toBe(
      "a b c d e f g h i j 2026-11-08 09-05.txt",
    );
    expect(exportFileName("tab\there\u0007", date, "text")).toBe("tab here 2026-11-08 09-05.txt");
  });

  it("keeps Unicode and falls back to Hangar for empty titles", () => {
    expect(exportFileName("Überblick 日本", date, "text")).toBe(
      "Überblick 日本 2026-11-08 09-05.txt",
    );
    expect(exportFileName(" ::: ", date, "text")).toBe("Hangar 2026-11-08 09-05.txt");
    expect(exportFileName("dots...", date, "text")).toBe("dots 2026-11-08 09-05.txt");
  });

  it("shortens very long titles", () => {
    const name = exportFileName("x".repeat(300), date, "text");
    expect(name.length).toBeLessThanOrEqual(80 + " 2026-11-08 09-05.txt".length);
  });
});

describe("htmlDocument", () => {
  it("turns the clipboard fragment into a file with charset and title", () => {
    const fragment =
      "<html><body><!--StartFragment--><pre>hi</pre><!--EndFragment--></body></html>";
    const html = htmlDocument(fragment, "A <b> & C", "#0e1116");
    expect(html.startsWith("<!DOCTYPE html>")).toBe(true);
    expect(html).toContain('<meta charset="utf-8">');
    expect(html).toContain("<title>A &lt;b&gt; &amp; C</title>");
    expect(html).toContain("<pre>hi</pre>");
    expect(html.match(/<html>/g)).toHaveLength(1);
    expect(html.match(/<\/body>/g)).toHaveLength(1);
  });
});

describe("utf8Bytes", () => {
  it("counts bytes, not characters", () => {
    expect(utf8Bytes("abc")).toBe(3);
    expect(utf8Bytes("ä")).toBe(2);
    expect(utf8Bytes("日")).toBe(3);
  });
});

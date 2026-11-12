import { describe, expect, it } from "vitest";

import { isWebLink, renderMarkdown, renderViewerMarkdown } from "./markdownPreview";

describe("renderViewerMarkdown", () => {
  it("renders HTML in the file as text", () => {
    const html = renderViewerMarkdown('<b class="raw">marker</b>\n\n<img src="x.png">');
    expect(html).not.toContain("<b ");
    expect(html).not.toContain("<img");
    expect(html).toContain("&lt;b class=&quot;raw&quot;&gt;marker");
  });

  it("makes web links openable through data-href only", () => {
    const html = renderViewerMarkdown(
      "[Docs](https://example.com/a?b=1&c=2) and https://tauri.app",
    );
    expect(html).not.toContain("<a");
    expect(html).not.toContain(" href=");
    expect(html).toContain('data-href="https://example.com/a?b=1&amp;c=2"');
    expect(html).toContain('role="link" tabindex="0"');
    expect(html).toContain('data-href="https://tauri.app"');
  });

  it("leaves other links as plain text", () => {
    const relative = renderViewerMarkdown("[Guide](docs/guide.md) [Mail](mailto:a@example.com)");
    expect(relative).not.toContain("data-href");
    expect(relative).toContain('<span class="md-link" title="docs/guide.md">Guide</span>');
    // markdown-it refuses these schemes while parsing: no link at all.
    for (const scheme of ["javascript:marker", "file:///C:/x.txt", "data:text/plain,marker"]) {
      expect(renderViewerMarkdown(`[x](${scheme})`)).not.toContain("md-link");
    }
  });

  it("loads no images, also not remote ones", () => {
    const html = renderViewerMarkdown("![Shot](https://example.com/shot.png) ![Local](a.png)");
    expect(html).not.toContain("<img");
    expect(html).toContain("[Shot]");
    expect(html).toContain("[Local]");
  });

  it("shows tasks without letting them change", () => {
    const html = renderViewerMarkdown("- [x] done");
    expect(html).toContain(
      '<input type="checkbox" class="md-task" data-line="0" checked disabled>',
    );
    expect(renderMarkdown("- [x] done")).not.toContain("disabled");
  });

  it("knows web links", () => {
    expect(isWebLink("https://example.com")).toBe(true);
    expect(isWebLink("HTTP://example.com/x")).toBe(true);
    expect(isWebLink("https://")).toBe(false);
    expect(isWebLink("ftp://example.com")).toBe(false);
    expect(isWebLink("example.com")).toBe(false);
  });
});

describe("renderMarkdown", () => {
  it("understands common Markdown", () => {
    const html = renderMarkdown("# Title\n\n- **bold** and `code`\n- two");
    expect(html).toContain("<h1>Title</h1>");
    expect(html).toContain("<strong>bold</strong>");
    expect(html).toContain("<code>code</code>");
    expect(html).toContain("<li>two</li>");
  });

  it("outputs HTML as text", () => {
    const html = renderMarkdown('<script>alert(1)</script><img src=x onerror="y">');
    expect(html).not.toContain("<script");
    expect(html).not.toContain("<img");
    expect(html).toContain("&lt;script&gt;");
  });

  it("does not turn links and addresses into clickable references", () => {
    const html = renderMarkdown("[Docs](https://example.com/a?b=1&c=2) and https://tauri.app");
    expect(html).not.toContain("<a");
    expect(html).not.toContain("href");
    expect(html).toContain(
      '<span class="md-link" title="https://example.com/a?b=1&amp;c=2">Docs</span>',
    );
    expect(html).toContain('title="https://tauri.app"');
  });

  it("loads no images", () => {
    const html = renderMarkdown('![Logo "x"](https://example.com/logo.png)');
    expect(html).not.toContain("<img");
    expect(html).toContain("[Logo &quot;x&quot;]");
  });

  it("blocks dangerous addresses while parsing", () => {
    // markdown-it leaves the link as plain text; no reference is created.
    const html = renderMarkdown("[x](javascript:alert(1))");
    expect(html).not.toContain("md-link");
    expect(html).toBe("<p>[x](javascript:alert(1))</p>\n");
  });
});

describe("task items", () => {
  it("become checkboxes that know their source line", () => {
    const html = renderMarkdown(
      "# Plan\n\n- [ ] write\n- [x] **done**\n- plain\n\n```\n- [ ] code\n```",
    );
    expect(html).toContain('<input type="checkbox" class="md-task" data-line="2"> write');
    expect(html).toContain(
      '<input type="checkbox" class="md-task" data-line="3" checked> <strong>done</strong>',
    );
    expect(html).toContain("<li>plain</li>");
    expect(html.match(/md-task/g)).toHaveLength(2);
    expect(html).toContain('class="task-item"');
  });

  it("leaves brackets elsewhere alone", () => {
    expect(renderMarkdown("[ ] not a list")).not.toContain("checkbox");
  });
});

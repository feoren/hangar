// Markdown previews: notes (loaded when the note preview is switched on) and the file viewer's
// preview of `.md` files (loaded with the viewer's preview). Both are dynamic imports.
//
// Security: `html: false` renders HTML in the Markdown as text. Links never become <a>, because a
// click would navigate the WebView itself away from Hangar. In notes they stay highlighted,
// copyable text with the address in the tooltip. In the file viewer an http(s) link becomes a
// focusable span with the address in `data-href`; the viewer hands a click to the backend, which
// checks the address again and opens it in the browser. Images are not loaded but shown as their
// alt text; the CSP forbids external images anyway.
//
// Task items (`- [ ]`, `- [x]`) become real checkboxes with `data-line`, the 0-based source line,
// so a click in a note can tick the task in the text (toggleTaskAtLine). The file viewer is
// read-only: its checkboxes are disabled.

import MarkdownIt from "markdown-it";

const TASK = /^\[([ xX])\]\s+/;

/** Addresses the file viewer opens in the browser; everything else stays text. */
export function isWebLink(href: string): boolean {
  return /^https?:\/\/[^/\s]/i.test(href);
}

function createRenderer(options: { openLinks: boolean; editableTasks: boolean }) {
  const md = new MarkdownIt({ html: false, linkify: true, breaks: true });
  const escape = md.utils.escapeHtml;

  md.renderer.rules.link_open = (tokens, index) => {
    const href = String(tokens[index]?.attrGet("href") ?? "");
    if (options.openLinks && isWebLink(href)) {
      return `<span class="md-link md-open" role="link" tabindex="0" data-href="${escape(href)}" title="${escape(href)}">`;
    }
    return `<span class="md-link" title="${escape(href)}">`;
  };

  md.renderer.rules.link_close = () => "</span>";

  md.renderer.rules.image = (tokens, index) => {
    const token = tokens[index];
    const alt = token?.content ?? "";
    const src = String(token?.attrGet("src") ?? "");
    return `<span class="md-image" title="${escape(src)}">[${escape(alt)}]</span>`;
  };

  // After inline parsing: an item whose paragraph starts with "[ ] " gets a checkbox token instead.
  md.core.ruler.after("inline", "task_items", (state) => {
    const tokens = state.tokens;
    for (let index = 2; index < tokens.length; index += 1) {
      const inline = tokens[index];
      const paragraph = tokens[index - 1];
      const item = tokens[index - 2];
      if (inline?.type !== "inline" || paragraph?.type !== "paragraph_open") continue;
      if (item?.type !== "list_item_open") continue;
      const first = inline.children?.[0];
      const match = first?.type === "text" ? TASK.exec(first.content) : null;
      const line = paragraph.map?.[0];
      if (!first || !match || line === undefined) continue;
      first.content = first.content.slice(match[0].length);
      const box = new state.Token("task_box", "input", 0);
      box.meta = { checked: match[1] !== " ", line };
      inline.children?.unshift(box);
      item.attrJoin("class", "task-item");
    }
    return false;
  });

  md.renderer.rules["task_box"] = (tokens, index) => {
    const meta = tokens[index]?.meta;
    const checked = meta?.["checked"] === true;
    const line = typeof meta?.["line"] === "number" ? meta["line"] : -1;
    const disabled = options.editableTasks ? "" : " disabled";
    return `<input type="checkbox" class="md-task" data-line="${String(line)}"${checked ? " checked" : ""}${disabled}> `;
  };

  return md;
}

const notes = createRenderer({ openLinks: false, editableTasks: true });
const viewer = createRenderer({ openLinks: true, editableTasks: false });

/**
 * Markdown as HTML for `dangerouslySetInnerHTML`; contains no links, no images, no raw HTML, only
 * the task checkboxes drawn here.
 */
export function renderMarkdown(text: string): string {
  return notes.render(text);
}

/**
 * Markdown of a file in the viewer, as HTML for `dangerouslySetInnerHTML`: as `renderMarkdown`,
 * except that http(s) links carry their address in `data-href` (never `href`) and checkboxes
 * are disabled.
 */
export function renderViewerMarkdown(text: string): string {
  return viewer.render(text);
}

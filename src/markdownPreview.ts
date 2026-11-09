// Note preview. Loaded only when the preview is switched on (dynamic import).
//
// Security: `html: false` renders HTML in the Markdown as text. Links do not become <a>, because
// a click would navigate the WebView itself away from Hangar; they stay highlighted, copyable
// text with the address in the tooltip. Images are not loaded but shown as their alt text;
// the CSP forbids external images anyway.
//
// Task items (`- [ ]`, `- [x]`) become real checkboxes with `data-line`, the 0-based source line,
// so a click can tick the task in the text (toggleTaskAtLine).

import MarkdownIt from "markdown-it";

const md = new MarkdownIt({ html: false, linkify: true, breaks: true });

md.renderer.rules.link_open = (tokens, index) => {
  const href = String(tokens[index]?.attrGet("href") ?? "");
  return `<span class="md-link" title="${md.utils.escapeHtml(href)}">`;
};

md.renderer.rules.link_close = () => "</span>";

md.renderer.rules.image = (tokens, index) => {
  const token = tokens[index];
  const alt = token?.content ?? "";
  const src = String(token?.attrGet("src") ?? "");
  return `<span class="md-image" title="${md.utils.escapeHtml(src)}">[${md.utils.escapeHtml(alt)}]</span>`;
};

const TASK = /^\[([ xX])\]\s+/;

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
  return `<input type="checkbox" class="md-task" data-line="${String(line)}"${checked ? " checked" : ""}> `;
};

/**
 * Markdown as HTML for `dangerouslySetInnerHTML`; contains no links, no images, no raw HTML, only
 * the task checkboxes drawn here.
 */
export function renderMarkdown(text: string): string {
  return md.render(text);
}

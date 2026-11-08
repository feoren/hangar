// Note preview. Loaded only when the preview is switched on (dynamic import).
//
// Security: `html: false` renders HTML in the Markdown as text. Links do not become <a>, because
// a click would navigate the WebView itself away from Hangar; they stay highlighted, copyable
// text with the address in the tooltip. Images are not loaded but shown as their alt text;
// the CSP forbids external images anyway.

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

/** Markdown as HTML for `dangerouslySetInnerHTML`; contains no links, no images, no raw HTML. */
export function renderMarkdown(text: string): string {
  return md.render(text);
}

/**
 * Fits a text field's height to its content, never below `floor` (a height the person dragged it
 * to). CSS `min-height` and `max-height` still apply; above the maximum the field scrolls.
 *
 * Measured by script rather than with CSS `field-sizing: content`, because WebKit (the webview on
 * macOS and Linux) does not support that everywhere, and one path behaves the same on all three.
 */
export function fitTextareaHeight(field: HTMLTextAreaElement, floor = 0): void {
  // "auto" lets the field shrink for the measurement; that can shorten a scrolled container for a
  // moment and clamp its position, so the positions are put back afterwards.
  const scrolled: [Element, number][] = [];
  for (let node = field.parentElement; node !== null; node = node.parentElement) {
    if (node.scrollTop > 0) scrolled.push([node, node.scrollTop]);
  }
  field.style.height = "auto";
  // scrollHeight leaves out the border; with box-sizing border-box the height includes it.
  const border = field.offsetHeight - field.clientHeight;
  const height = Math.max(Math.ceil(field.scrollHeight + border), Math.ceil(floor));
  field.style.height = `${String(height)}px`;
  for (const [node, top] of scrolled) node.scrollTop = top;
}

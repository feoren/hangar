// The small editor of a queued message (QueueDialog.tsx): what a key does and where the editor
// sits. Pure, so both can be tested.

export interface EditorKey {
  key: string;
  shiftKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  metaKey: boolean;
  /** An input method is composing; its Enter confirms the composition, nothing else. */
  isComposing: boolean;
}

/**
 * Enter queues the message, Shift+Enter makes a new line (the textarea's own behavior), every other
 * key types. Escape is the dialog's: it cancels.
 */
export function editorKeyAction(event: EditorKey): "save" | "type" {
  if (event.key !== "Enter" || event.isComposing) return "type";
  if (event.shiftKey || event.altKey || event.metaKey) return "type";
  return "save";
}

/** Room kept to the window's edges. */
const EDGE = 16;

/**
 * Top left corner of the editor opened at `anchor` (where the session menu was): right there, but
 * moved in so the whole editor stays inside the window. `null` without an anchor: centered.
 */
export function editorPosition(
  anchor: { x: number; y: number } | null,
  size: { width: number; height: number },
  viewport: { width: number; height: number },
): { left: number; top: number } | null {
  if (anchor === null) return null;
  const clampTo = (value: number, extent: number, room: number) =>
    Math.max(EDGE, Math.min(value, room - extent - EDGE));
  return {
    left: clampTo(anchor.x, size.width, viewport.width),
    top: clampTo(anchor.y, size.height, viewport.height),
  };
}

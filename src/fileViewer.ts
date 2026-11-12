// Pure parts of the file viewer window (FileViewerWindow.tsx): CodeMirror's own texts in the
// app's language, and what the window shows for an answer of `file_viewer_content`.

import type { FileContent } from "./api";
import type { MessageKey, Translate } from "./i18n";

/** CodeMirror's phrases (search panel, folding, special characters) and their texts. */
const PHRASES: Record<string, MessageKey> = {
  Find: "cm.find",
  next: "cm.next",
  previous: "cm.previous",
  all: "cm.all",
  "match case": "cm.matchCase",
  regexp: "cm.regexp",
  "by word": "cm.byWord",
  close: "cm.close",
  "current match": "cm.currentMatch",
  "on line": "cm.onLine",
  "Go to line": "cm.gotoLine",
  go: "cm.go",
  "Fold line": "cm.foldLine",
  "Unfold line": "cm.unfoldLine",
  "folded code": "cm.foldedCode",
  unfold: "cm.unfold",
  "Folded lines": "cm.foldedLines",
  "Unfolded lines": "cm.unfoldedLines",
  to: "cm.to",
  "Control character": "cm.controlChar",
};

/** For `EditorState.phrases`: CodeMirror's English phrase to the translated text. */
export function editorPhrases(t: Translate): Record<string, string> {
  return Object.fromEntries(Object.entries(PHRASES).map(([phrase, key]) => [phrase, t(key)]));
}

/** What the window shows below its header. */
export type ViewerBody =
  | { kind: "loading" }
  | { kind: "failed"; message: string }
  | { kind: "binary" }
  | { kind: "text"; text: string; truncated: boolean };

export function viewerBody(content: FileContent | null, error: string | null): ViewerBody {
  if (error !== null) return { kind: "failed", message: error };
  if (content === null) return { kind: "loading" };
  if (content.binary || content.text === null) return { kind: "binary" };
  return { kind: "text", text: content.text, truncated: content.truncated };
}

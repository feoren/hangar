// Pure parts of the file viewer window (FileViewerWindow.tsx): CodeMirror's own texts in the
// app's language, and what the window shows for an answer of `file_viewer_content`.

import type { FileContent, HeadDiff, ImageData } from "./api";
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

/**
 * What the window shows: the file as code, its preview (Markdown rendered, an image drawn) or
 * its changes against the last commit.
 */
export type ViewerMode = "code" | "preview" | "diff";

/** What the window shows below its header. */
export type ViewerBody =
  | { kind: "loading" }
  | { kind: "failed"; message: string }
  | { kind: "binary" }
  | { kind: "text"; text: string; truncated: boolean }
  | { kind: "markdown"; text: string; truncated: boolean }
  | { kind: "image"; src: string }
  | { kind: "imageTooLarge" }
  | { kind: "diff" };

/** Markdown by its name, as editors decide. */
export function isMarkdown(name: string): boolean {
  return /\.(md|markdown|mdown|mkd)$/i.test(name);
}

/** The `src` of an image: a data address, so nothing is fetched and an SVG runs no script. */
export function imageSource(image: ImageData): string {
  return `data:${image.mime};base64,${image.data}`;
}

/**
 * Whether a comparison with the last commit can work: the file lies in a repository with a commit
 * (`null` while that is not known yet, as after the window opened).
 */
export function diffWorks(diff: HeadDiff | null): boolean {
  return diff !== null && diff.state !== "notRepo" && diff.state !== "noHead";
}

/**
 * Which switches the header offers for this file. The diff switch shows only for a text file in a
 * repository with a commit (`canDiff`, from `diffWorks`), so it never leads to "not possible".
 */
export function viewerModes(
  content: FileContent | null,
  canDiff: boolean,
): { preview: boolean; diff: boolean } {
  if (content === null) return { preview: false, diff: false };
  const text = content.text !== null;
  return {
    // A raster image has only its picture; an SVG has picture and source, Markdown both too.
    preview: text && (content.image !== null || isMarkdown(content.name)),
    diff: text && canDiff,
  };
}

/** How often the live refresh asks for the stamp: images change rarely and are large to read. */
export function refreshInterval(content: FileContent | null): number {
  return content?.image != null || content?.imageTooLarge === true ? 4000 : 1000;
}

/** The mode a file opens in: its picture for an image, the code otherwise. */
export function initialMode(content: FileContent): ViewerMode {
  return content.image !== null ? "preview" : "code";
}

export function viewerBody(
  content: FileContent | null,
  error: string | null,
  mode: ViewerMode = "code",
): ViewerBody {
  if (error !== null) return { kind: "failed", message: error };
  if (content === null) return { kind: "loading" };
  const text = content.binary ? null : content.text;
  if (mode === "diff" && text !== null) return { kind: "diff" };
  if (content.image !== null && (mode === "preview" || text === null)) {
    return { kind: "image", src: imageSource(content.image) };
  }
  if (text === null) return content.imageTooLarge ? { kind: "imageTooLarge" } : { kind: "binary" };
  if (mode === "preview" && isMarkdown(content.name)) {
    return { kind: "markdown", text, truncated: content.truncated };
  }
  return { kind: "text", text, truncated: content.truncated };
}

/** The live refresh compares this: the time of the last change and the size. */
export function stampKey(stamp: { modified: number | null; size: number }): string {
  return `${String(stamp.modified)}:${String(stamp.size)}`;
}

/** The stamp of a file that cannot be read (gone, refused). */
export const MISSING_STAMP = "missing";

export interface StampPolling {
  /** The file's stamp now; rejects (counted as missing) when it is gone or refused. */
  read: () => Promise<string>;
  /** The stamp of what the window shows; `null` while nothing is shown yet. */
  shown: () => string | null;
  /** Read the file again: its stamp differed from the shown one twice in a row. */
  reload: () => void;
  /** No asking while this holds (the window is hidden or minimized). */
  paused: () => boolean;
  /** Milliseconds until the next ask; asked anew before each one. */
  intervalMs: () => number;
}

/**
 * Live refresh of the viewer: asks for the file's stamp every `intervalMs()` and reloads once a
 * new stamp held for two asks in a row, so a file still being written is read when it settled.
 * One ask at a time; the returned function stops it, also an ask still under way.
 */
export function pollStamps(polling: StampPolling): () => void {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let pending: string | null = null;

  const schedule = () => {
    if (!stopped) timer = setTimeout(tick, polling.intervalMs());
  };
  const tick = () => {
    timer = null;
    if (polling.paused()) {
      schedule();
      return;
    }
    polling
      .read()
      .catch(() => MISSING_STAMP)
      .then((stamp) => {
        if (stopped) return;
        const shown = polling.shown();
        if (shown === null || stamp === shown) {
          pending = null;
        } else if (pending === stamp) {
          pending = null;
          polling.reload();
        } else {
          pending = stamp;
        }
        schedule();
      })
      .catch(() => {
        schedule();
      });
  };

  schedule();
  return () => {
    stopped = true;
    if (timer !== null) clearTimeout(timer);
    timer = null;
  };
}

/** What the diff mode says instead of a diff (`null`: there is one to show). */
export function diffNote(diff: HeadDiff): MessageKey | null {
  switch (diff.state) {
    case "notRepo":
      return "viewer.diffNotRepo";
    case "noHead":
      return "viewer.diffNoHead";
    case "notInHead":
      return "viewer.diffNotInHead";
    case "same":
      return "viewer.diffSame";
    case "changed":
      return null;
  }
}

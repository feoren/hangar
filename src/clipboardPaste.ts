// What a paste from the clipboard puts into a session. Text goes in as it is. A clipboard with an
// image but no text (a screenshot) has nothing to type, so the backend saves the image as a file
// and its path goes in, quoted like a dropped file: agent CLIs attach image paths they read.

import { askBeforePaste, pasteWarning, secretWarning, type PasteWarning } from "./appPaste";
import { dropText, type ShellKind } from "./pathQuote";
import { sanitizePaste } from "./termSafety";

/** The backend's answer to `paste_clipboard_image`. */
export type ImagePaste = { kind: "saved"; path: string } | { kind: "noImage" } | { kind: "busy" };

export interface ClipboardPasteTarget {
  pasteText: (text: string) => void;
  /** Asked once the path is known, so a session that ended meanwhile still quotes correctly. */
  shellKind: () => ShellKind;
  /**
   * Set while this session waits for an image paste. A held Ctrl+V repeats the paste event many
   * times a second; the repeats are dropped instead of each asking the backend.
   */
  imagePasteInFlight: boolean;
  /** Whether the program in the session has turned on bracketed paste right now. */
  bracketedPaste: () => boolean;
  /** The setting "Ask before a risky paste"; off, only a suspected key still asks. */
  warnsBeforePaste: () => boolean;
  /** Asks the user; resolves to whether the text goes in after all. */
  confirmPaste: (warning: PasteWarning) => Promise<boolean>;
}

/**
 * - `text`: the clipboard's text was pasted.
 * - `image`: the clipboard's image was saved and its path pasted.
 * - `empty`: neither text nor an image; nothing happened.
 * - `busy`: an image paste was still running (here or in another session); dropped quietly.
 * - `failed`: there was no text and saving the image failed.
 * - `declined`: the text would have run lines or hides characters, and the user said no.
 */
export type ClipboardPasteResult = "text" | "image" | "empty" | "busy" | "failed" | "declined";

/**
 * Pastes `text`, or without text the path of the clipboard's image, which `saveImage` saves.
 *
 * Copied text can hold more than it shows (pastejacking: a web page puts a hidden line break and
 * a command into the clipboard). Without bracketed paste each line runs as it arrives, so such
 * text, and text with invisible characters, waits for the user's answer, as Windows Terminal
 * asks before a multi-line paste. Text without anything to warn about goes in synchronously.
 */
export async function pasteFromClipboard(
  target: ClipboardPasteTarget,
  text: string,
  saveImage: () => Promise<ImagePaste>,
): Promise<ClipboardPasteResult> {
  if (sanitizePaste(text) !== "") {
    const check = (pasted: string, bracketedPaste: boolean): PasteWarning | null => {
      const found = pasteWarning(pasted, bracketedPaste);
      return target.warnsBeforePaste() ? found : secretWarning(found);
    };
    const warning = check(text, target.bracketedPaste());
    if (warning !== null) {
      const ask = (shown: PasteWarning) => target.confirmPaste(shown);
      const bracketed = () => target.bracketedPaste();
      if (!(await askBeforePaste(text, warning, bracketed, ask, check))) return "declined";
    }
    target.pasteText(text);
    return "text";
  }
  if (target.imagePasteInFlight) return "busy";
  target.imagePasteInFlight = true;
  let answer: ImagePaste;
  try {
    answer = await saveImage();
  } catch {
    return "failed";
  } finally {
    target.imagePasteInFlight = false;
  }
  switch (answer.kind) {
    case "busy":
      return "busy";
    case "noImage":
      return "empty";
    case "saved":
      if (answer.path === "") return "empty";
      target.pasteText(dropText([answer.path], target.shellKind()));
      return "image";
  }
}

/** Checks the backend's answer; anything unexpected counts as no image. */
export function parseImagePaste(value: unknown): ImagePaste {
  if (typeof value !== "object" || value === null || !("kind" in value)) {
    return { kind: "noImage" };
  }
  if (value.kind === "busy") return { kind: "busy" };
  if (value.kind === "saved" && "path" in value && typeof value.path === "string") {
    return { kind: "saved", path: value.path };
  }
  return { kind: "noImage" };
}

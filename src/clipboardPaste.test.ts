import { describe, expect, it } from "vitest";

import {
  parseImagePaste,
  pasteFromClipboard,
  type ClipboardPasteTarget,
  type ImagePaste,
} from "./clipboardPaste";
import type { ShellKind } from "./pathQuote";

function target(kind: ShellKind = "agent"): ClipboardPasteTarget & { written: string[] } {
  const written: string[] = [];
  return {
    written,
    pasteText: (text) => {
      written.push(text);
    },
    shellKind: () => kind,
    imagePasteInFlight: false,
  };
}

function saved(path: string): () => Promise<ImagePaste> {
  return () => Promise.resolve({ kind: "saved", path });
}

function noImage(): Promise<ImagePaste> {
  return Promise.reject(new Error("must not be asked"));
}

describe("pasteFromClipboard", () => {
  it("pastes text unchanged and never looks for an image", async () => {
    const session = target();
    await expect(pasteFromClipboard(session, "one\ntwo", noImage)).resolves.toBe("text");
    expect(session.written).toEqual(["one\ntwo"]);
  });

  it("pastes the saved image's path when the clipboard has no text", async () => {
    const session = target();
    const path = "C:\\Cache\\pastes\\paste-20261110-142233-1a2b3c4d.png";
    await expect(pasteFromClipboard(session, "", saved(path))).resolves.toBe("image");
    expect(session.written).toEqual([`${path} `]);
    expect(session.imagePasteInFlight).toBe(false);
  });

  it("quotes a path with spaces for the program in the session", async () => {
    const path = "C:\\Users\\Jane Doe\\cache\\paste.png";
    const agent = target("agent");
    await pasteFromClipboard(agent, "", saved(path));
    expect(agent.written).toEqual([`"${path}" `]);
    const powershell = target("powershell");
    await pasteFromClipboard(powershell, "", saved(path));
    expect(powershell.written).toEqual([`'${path}' `]);
    const wsl = target("wsl");
    await pasteFromClipboard(wsl, "", saved(path));
    expect(wsl.written).toEqual(["'/mnt/c/Users/Jane Doe/cache/paste.png' "]);
  });

  it("treats text of control characters only as no text", async () => {
    const session = target();
    await pasteFromClipboard(session, "\u001b", saved("/tmp/a.png"));
    expect(session.written).toEqual(["/tmp/a.png "]);
  });

  it("does nothing when the clipboard holds neither text nor an image", async () => {
    const session = target();
    const result = await pasteFromClipboard(session, "", () =>
      Promise.resolve({ kind: "noImage" }),
    );
    expect(result).toBe("empty");
    expect(session.written).toEqual([]);
  });

  it("drops repeats quietly while an image paste is running", async () => {
    const session = target();
    let finish: (answer: ImagePaste) => void = () => undefined;
    let asked = 0;
    const save = () => {
      asked += 1;
      return new Promise<ImagePaste>((resolve) => {
        finish = resolve;
      });
    };
    const first = pasteFromClipboard(session, "", save);
    await expect(pasteFromClipboard(session, "", save)).resolves.toBe("busy");
    expect(asked).toBe(1);
    // Text still goes in at once.
    await expect(pasteFromClipboard(session, "ls", save)).resolves.toBe("text");
    finish({ kind: "saved", path: "/tmp/a.png" });
    await expect(first).resolves.toBe("image");
    expect(session.written).toEqual(["ls", "/tmp/a.png "]);
    expect(session.imagePasteInFlight).toBe(false);
  });

  it("stays quiet when the backend is busy with another session's paste", async () => {
    const session = target();
    await expect(
      pasteFromClipboard(session, "", () => Promise.resolve({ kind: "busy" })),
    ).resolves.toBe("busy");
    expect(session.written).toEqual([]);
  });

  it("reports a failed save, pastes nothing and allows the next paste", async () => {
    const session = target();
    const result = await pasteFromClipboard(session, "", () =>
      Promise.reject(new Error("disk full")),
    );
    expect(result).toBe("failed");
    expect(session.written).toEqual([]);
    expect(session.imagePasteInFlight).toBe(false);
  });
});

describe("parseImagePaste", () => {
  it("reads the backend's three answers", () => {
    expect(parseImagePaste({ kind: "saved", path: "C:\\a.png" })).toEqual({
      kind: "saved",
      path: "C:\\a.png",
    });
    expect(parseImagePaste({ kind: "noImage" })).toEqual({ kind: "noImage" });
    expect(parseImagePaste({ kind: "busy" })).toEqual({ kind: "busy" });
  });

  it("treats anything else as no image", () => {
    expect(parseImagePaste(null)).toEqual({ kind: "noImage" });
    expect(parseImagePaste("C:\\a.png")).toEqual({ kind: "noImage" });
    expect(parseImagePaste({ kind: "saved", path: 3 })).toEqual({ kind: "noImage" });
  });
});

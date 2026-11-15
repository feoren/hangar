import { describe, expect, it } from "vitest";

import {
  askBeforePaste,
  pasteFromApp,
  pasteWarning,
  type PasteTarget,
  type PasteWarning,
} from "./appPaste";

function target(bracketed: boolean): PasteTarget & { written: string[] } {
  const written: string[] = [];
  return {
    written,
    bracketedPaste: () => bracketed,
    pasteText: (text) => {
      written.push(text);
    },
  };
}

describe("pasteWarning", () => {
  it("is quiet for one line or when the program takes bracketed paste", () => {
    expect(pasteWarning("git status", false)).toBeNull();
    expect(pasteWarning("one\ntwo", true)).toBeNull();
    expect(pasteWarning("tab\there", false)).toBeNull();
  });

  it("warns that each line runs when bracketed paste is off", () => {
    expect(pasteWarning("one\ntwo", false)).toEqual({ runsLines: true, hidden: false });
    expect(pasteWarning("one\r\n", false)).toEqual({ runsLines: true, hidden: false });
  });

  it("warns about bidi and zero-width characters, even with bracketed paste", () => {
    expect(pasteWarning("ls \u202e-la", true)).toEqual({ runsLines: false, hidden: true });
    expect(pasteWarning("a\u200bb\nc", false)).toEqual({ runsLines: true, hidden: true });
  });

  it("leaves emoji sequences joined by U+200D alone", () => {
    expect(pasteWarning("family \u{1F468}\u200d\u{1F469}\u200d\u{1F467}", true)).toBeNull();
    expect(pasteWarning("a\u200db", true)).toEqual({ runsLines: false, hidden: true });
  });

  it("judges the text as it is pasted: stripped control characters do not count", () => {
    expect(pasteWarning("\u0007\u001b", false)).toBeNull();
  });
});

describe("pasteFromApp", () => {
  it("pastes at once when there is nothing to warn about", async () => {
    const session = target(true);
    let asked = false;
    const pasted = await pasteFromApp(session, "one\ntwo", () => {
      asked = true;
      return Promise.resolve(true);
    });
    expect(pasted).toBe(true);
    expect(asked).toBe(false);
    expect(session.written).toEqual(["one\ntwo"]);
  });

  it("writes nothing before a multiline paste without bracketed paste is confirmed", async () => {
    const session = target(false);
    let answer: (yes: boolean) => void = () => undefined;
    let warning: PasteWarning | null = null;
    const pasted = pasteFromApp(session, "rm a\nrm b", (asked) => {
      warning = asked;
      return new Promise<boolean>((resolve) => {
        answer = resolve;
      });
    });
    await Promise.resolve();
    expect(warning).toEqual({ runsLines: true, hidden: false });
    expect(session.written).toEqual([]);
    answer(true);
    await expect(pasted).resolves.toBe(true);
    expect(session.written).toEqual(["rm a\nrm b"]);
  });

  it("writes nothing when the warning is cancelled", async () => {
    const session = target(false);
    const pasted = await pasteFromApp(session, "a\nb", () => Promise.resolve(false));
    expect(pasted).toBe(false);
    expect(session.written).toEqual([]);
  });

  it("does nothing for empty text", async () => {
    const session = target(false);
    await expect(pasteFromApp(session, "", () => Promise.resolve(true))).resolves.toBe(false);
    expect(session.written).toEqual([]);
  });

  it("asks again when the program turned bracketed paste off during the question", async () => {
    // Hidden characters warn even with bracketed paste; the user agreed to that, not to lines
    // that run one by one, which is what the text does once the program turned it off.
    let bracketed = true;
    const written: string[] = [];
    const session: PasteTarget = {
      bracketedPaste: () => bracketed,
      pasteText: (text) => {
        written.push(text);
      },
    };
    const asked: PasteWarning[] = [];
    const pasted = await pasteFromApp(session, "a\u200b\nrm b", (warning) => {
      asked.push(warning);
      bracketed = false;
      return Promise.resolve(asked.length === 1);
    });
    expect(asked).toEqual([
      { runsLines: false, hidden: true },
      { runsLines: true, hidden: true },
    ]);
    expect(pasted).toBe(false);
    expect(written).toEqual([]);
  });

  it("pastes after a yes to the new question", async () => {
    let bracketed = true;
    const answers: PasteWarning[] = [];
    const allowed = await askBeforePaste(
      "a\u200b\nb",
      { runsLines: false, hidden: true },
      () => bracketed,
      (warning) => {
        answers.push(warning);
        bracketed = false;
        return Promise.resolve(true);
      },
    );
    expect(allowed).toBe(true);
    expect(answers).toHaveLength(2);
  });

  it("keeps the yes when the mode changed in the harmless direction", async () => {
    let bracketed = false;
    const written: string[] = [];
    const session: PasteTarget = {
      bracketedPaste: () => bracketed,
      pasteText: (text) => {
        written.push(text);
      },
    };
    let questions = 0;
    await pasteFromApp(session, "a\nb", () => {
      questions += 1;
      bracketed = true;
      return Promise.resolve(true);
    });
    expect(questions).toBe(1);
    expect(written).toEqual(["a\nb"]);
  });
});

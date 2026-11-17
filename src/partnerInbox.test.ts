import { describe, expect, it } from "vitest";

import {
  inboxBySession,
  messageLines,
  parseWaitingMessages,
  unshownMessages,
  type InboxSession,
  type WaitingMessage,
} from "./partnerInbox";
import type { PartnerBook } from "./partners";

const book: PartnerBook = {
  projects: [
    { name: "app", folder: "/work/app" },
    { name: "web", folder: "/work/web" },
  ],
  pairs: [["app", "web"]],
};

const message = (id: number, to: string): WaitingMessage => ({
  id,
  from: to === "web" ? "app" : "web",
  to,
  text: "show the text",
  at: 1,
});

const session = (id: number, cwd: string, extra: Partial<InboxSession> = {}): InboxSession => ({
  id,
  profileId: "claude",
  cwd,
  status: "running",
  ...extra,
});

const commands: Record<string, string> = { claude: "claude", pwsh: "pwsh.exe" };
const commandOf = (profileId: string) => commands[profileId];

describe("parseWaitingMessages", () => {
  it("reads the backend's list and leaves out what is malformed", () => {
    expect(
      parseWaitingMessages([
        { id: 1, from: "app", to: "web", text: "go", at: 5 },
        { id: 0, from: "app", to: "web", text: "go", at: 5 },
        { id: 2, from: "App", to: "web", text: "go", at: 5 },
        { id: 3, from: "app", to: "web", text: "  ", at: 5 },
        { id: 4, from: "app", to: "web", text: "go" },
        null,
      ]),
    ).toEqual([{ id: 1, from: "app", to: "web", text: "go", at: 5 }]);
    expect(parseWaitingMessages({ id: 1 })).toEqual([]);
    expect(parseWaitingMessages(undefined)).toEqual([]);
  });
});

describe("inboxBySession", () => {
  it("shows a message on every running agent session of its project", () => {
    const rows = inboxBySession(
      [message(1, "web"), message(2, "app")],
      book,
      [
        session(10, "/work/app"),
        session(11, "/work/web/src"),
        session(12, "/elsewhere/tree", { worktree: { repoRoot: "/work/web" } }),
        session(13, "/work/web", { status: "exited" }),
        session(14, "/work/web", { profileId: "pwsh" }),
        session(15, "/work/web", { kind: "update" }),
        session(16, "/work/other"),
        session(17, "/work/web", { profileId: "gone" }),
      ],
      commandOf,
    );
    expect([...rows.keys()]).toEqual([10, 11, 12]);
    expect(rows.get(10)?.map((m) => m.id)).toEqual([2]);
    expect(rows.get(11)?.map((m) => m.id)).toEqual([1]);
  });

  it("names the messages no row shows", () => {
    const messages = [message(1, "web"), message(2, "app")];
    const rows = inboxBySession(messages, book, [session(10, "/work/app")], commandOf);
    expect(unshownMessages(messages, rows).map((m) => m.id)).toEqual([1]);
    expect(inboxBySession([], book, [session(10, "/work/app")], commandOf).size).toBe(0);
  });
});

describe("messageLines", () => {
  it("splits the cleaned text at its line breaks", () => {
    expect(messageLines("one\ntwo\n\nfour")).toEqual(["one", "two", "", "four"]);
  });
});

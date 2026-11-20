// Roles and history of link groups (ADR 0014): how the window reads them from the backend.

import { describe, expect, it } from "vitest";

import {
  HISTORY_PREVIEW_CHARS,
  historyPreview,
  leadOf,
  linkMarks,
  nameProblem,
  parseLinkHistory,
  parseLinkState,
  validMemberName,
} from "./links";

const raw = (roles: unknown[]) => ({
  groups: [
    {
      id: 1,
      color: 0,
      askFirst: false,
      braked: false,
      members: roles.map((role, index) => ({
        session: index + 1,
        name: `m${String(index)}`,
        role,
      })),
    },
  ],
  sendable: [],
  unattended: [],
  messages: [],
});

describe("roles", () => {
  it("reads the lead and treats anything else as a member", () => {
    const [group] = parseLinkState(raw(["member", "lead", undefined, "boss"])).groups;
    expect(group?.members.map((m) => m.role)).toEqual(["member", "lead", "member", "member"]);
    expect(group === undefined ? null : leadOf(group)).toBe(2);
  });

  it("keeps at most one lead", () => {
    const [group] = parseLinkState(raw(["lead", "lead"])).groups;
    expect(group?.members.map((m) => m.role)).toEqual(["lead", "member"]);
  });

  it("gives a group without a lead none", () => {
    const [group] = parseLinkState(raw(["member", "member"])).groups;
    expect(group === undefined ? undefined : leadOf(group)).toBeNull();
  });

  it("marks the lead's row", () => {
    const marks = linkMarks(parseLinkState(raw(["member", "lead"])));
    expect(marks.get(2)?.lead).toBe(true);
    expect(marks.get(1)?.lead).toBe(false);
  });

  it("keeps lead free for the alias", () => {
    expect(validMemberName("lead")).toBe(false);
    expect(validMemberName("lead-2")).toBe(true);
    expect(nameProblem(["web", " lead "])).toBe("reserved");
  });
});

describe("parseLinkHistory", () => {
  const entry = {
    at: 5,
    fromName: "app",
    fromRole: "lead",
    toName: "web",
    outcome: "delivered",
    text: "done",
  };

  it("reads the backend's history", () => {
    expect(parseLinkHistory([entry, { ...entry, fromName: null, outcome: "discarded" }])).toEqual([
      {
        at: 5,
        fromName: "app",
        fromRole: "lead",
        toName: "web",
        outcome: "delivered",
        text: "done",
      },
      {
        at: 5,
        fromName: null,
        fromRole: "lead",
        toName: "web",
        outcome: "discarded",
        text: "done",
      },
    ]);
  });

  it("leaves out what is malformed", () => {
    expect(
      parseLinkHistory([
        { ...entry, at: "5" },
        { ...entry, fromName: "Bad Name" },
        { ...entry, toName: "" },
        { ...entry, outcome: "lost" },
        { ...entry, text: 3 },
        null,
      ]),
    ).toEqual([]);
    expect(parseLinkHistory({ entries: [] })).toEqual([]);
    expect(parseLinkHistory([{ ...entry, fromRole: "x" }])[0]?.fromRole).toBe("member");
  });
});

describe("historyPreview", () => {
  it("shows the first line with text, cut", () => {
    expect(historyPreview("\n  \n  first line \nsecond")).toBe("first line");
    expect(historyPreview("")).toBe("");
    const long = historyPreview("x".repeat(HISTORY_PREVIEW_CHARS + 10));
    expect(Array.from(long)).toHaveLength(HISTORY_PREVIEW_CHARS);
    expect(long.endsWith("…")).toBe(true);
  });
});

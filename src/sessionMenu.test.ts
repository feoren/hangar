import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import { queueOffer, sessionMenuEntries, stallOffer, type QueueOffer } from "./sessionMenu";

const t = createTranslator("en");

describe("sessionMenuEntries", () => {
  it("offers backing up with Hangar Drive only where it can take the folder", () => {
    const keys = (drive: boolean | undefined) =>
      sessionMenuEntries({ status: "running" }, t, false, {
        queue: null,
        stall: null,
        ...(drive === undefined ? {} : { drive }),
      }).map((entry) => entry.key);
    expect(keys(undefined)).not.toContain("drive");
    expect(keys(false)).not.toContain("drive");
    expect(keys(true)).toEqual(["rename", "marker", "drive", "sep", "kill", "close"]);
    const german = createTranslator("de");
    const entry = sessionMenuEntries({ status: "exited" }, german, false, {
      queue: null,
      stall: null,
      drive: true,
    })[2];
    expect(entry?.type === "action" ? entry.label : "").toBe("Mit Hangar Drive sichern …");
  });

  it("marks the Hangar Drive entry out with its logo and no other entry", () => {
    const entries = sessionMenuEntries({ status: "running" }, t, false, {
      queue: null,
      stall: null,
      drive: true,
    });
    const icons = entries.flatMap((entry) =>
      entry.type === "action" && entry.icon !== undefined ? [[entry.key, entry.icon]] : [],
    );
    expect(icons).toEqual([["drive", "hangarDrive"]]);
  });

  it("offers end process for a running session", () => {
    const entries = sessionMenuEntries({ status: "running" }, t);
    expect(entries.map((entry) => entry.key)).toEqual(["rename", "marker", "sep", "kill", "close"]);
    const close = entries.at(-1);
    expect(close?.type === "action" ? close.label : "").toBe(t("session.killAndClose"));
  });

  it("offers restart for an ended session", () => {
    const entries = sessionMenuEntries({ status: "exited" }, t);
    expect(entries.map((entry) => entry.key)).toEqual([
      "rename",
      "marker",
      "sep",
      "restart",
      "close",
    ]);
    const close = entries.at(-1);
    expect(close?.type === "action" ? close.label : "").toBe(t("session.close"));
  });

  it("offers the commands entry only when the session has commands", () => {
    const keys = (hasCommands: boolean) =>
      sessionMenuEntries({ status: "running" }, t, false, {
        queue: null,
        stall: null,
        commands: hasCommands,
      }).map((entry) => entry.key);
    expect(keys(false)).not.toContain("commands");
    expect(keys(true)).toEqual(["rename", "marker", "commands", "sep", "kill", "close"]);
    const entry = sessionMenuEntries({ status: "running" }, t, false, {
      queue: null,
      stall: null,
      commands: true,
    })[2];
    expect(entry?.type === "action" ? entry.label : "").toBe("Commands …");
  });

  it("offers the sub-agents only for sessions that have them", () => {
    const keys = (subAgents: boolean | undefined, shown = false) =>
      sessionMenuEntries(
        subAgents === undefined ? { status: "running" } : { status: "running", subAgents },
        t,
        shown,
      ).map((entry) => entry.key);
    expect(keys(undefined)).not.toContain("subAgents");
    expect(keys(false)).not.toContain("subAgents");
    expect(keys(true)).toEqual([
      "rename",
      "marker",
      "report",
      "sep-sub-agents",
      "subAgents",
      "sep",
      "kill",
      "close",
    ]);
    const label = (shown: boolean) => {
      const entry = sessionMenuEntries({ status: "running", subAgents: true }, t, shown).find(
        (item) => item.key === "subAgents",
      );
      return entry?.type === "action" ? entry.label : "";
    };
    expect(label(false)).toBe(t("session.showSubAgents"));
    expect(label(true)).toBe(t("session.hideSubAgents"));
    // An open view can always be closed, even after the session lost its sub-agents.
    expect(keys(false, true)).toContain("subAgents");
  });

  it("shows or hides the crew beside the sub-agents, only for sessions that have them", () => {
    const crew = (subAgents: boolean, crewShown: boolean | undefined) =>
      sessionMenuEntries({ status: "running", subAgents }, t, false, {
        queue: null,
        stall: null,
        ...(crewShown === undefined ? {} : { crewShown }),
      }).find((entry) => entry.key === "crew");
    const label = (entry: ReturnType<typeof crew>) => (entry?.type === "action" ? entry.label : "");
    expect(label(crew(true, false))).toBe(t("crew.show"));
    expect(label(crew(true, true))).toBe(t("crew.hide"));
    expect(crew(false, false)).toBeUndefined();
    expect(crew(true, undefined)).toBeUndefined();
    const keys = sessionMenuEntries({ status: "running", subAgents: true }, t, false, {
      queue: null,
      stall: null,
      crewShown: false,
    }).map((entry) => entry.key);
    expect(keys.indexOf("crew")).toBe(keys.indexOf("subAgents") + 1);
  });

  it("offers the handover only where the caller allows it, after the marker", () => {
    const keys = (handover: boolean) =>
      sessionMenuEntries({ status: "running" }, t, false, {
        queue: null,
        stall: null,
        handover,
      }).map((entry) => entry.key);
    expect(keys(false)).not.toContain("handover");
    expect(keys(true)).toEqual(["rename", "marker", "handover", "sep", "kill", "close"]);
  });

  it("offers sending to a partner only where the project has partners, after the handover", () => {
    const keys = (partner: boolean) =>
      sessionMenuEntries({ status: "running" }, t, false, {
        queue: null,
        stall: null,
        handover: true,
        partner,
      }).map((entry) => entry.key);
    expect(keys(false)).not.toContain("partner");
    expect(keys(true)).toEqual(["rename", "marker", "handover", "partner", "sep", "kill", "close"]);
  });

  it("offers the report only for programs whose transcript Hangar reads", () => {
    const report = (session: { status: "running" | "exited"; subAgents?: boolean }) =>
      sessionMenuEntries(session, t).find((entry) => entry.key === "report");
    // Claude Code sessions with a conversation id, running or ended.
    for (const status of ["running", "exited"] as const) {
      const entry = report({ status, subAgents: true });
      expect(entry?.type === "action" ? entry.label : "").toBe(t("session.exportReport"));
    }
    // Codex, terminals and every other program: hidden, not disabled.
    expect(report({ status: "running" })).toBeUndefined();
    expect(report({ status: "running", subAgents: false })).toBeUndefined();
  });
});

describe("queued message entries", () => {
  const offer = (patch: Partial<Parameters<typeof queueOffer>[0]> = {}) =>
    queueOffer({
      status: "running",
      agent: true,
      attentionEnabled: true,
      state: "working",
      queued: false,
      ...patch,
    });

  it("offers queueing only for a working agent session with attention detection", () => {
    expect(offer()).toBe("offer");
    expect(offer({ agent: false })).toBeNull();
    expect(offer({ attentionEnabled: false })).toBeNull();
    expect(offer({ state: "idle" })).toBeNull();
    expect(offer({ state: "attention" })).toBeNull();
    expect(offer({ status: "exited" })).toBeNull();
    expect(offer({ status: "starting" })).toBeNull();
  });

  it("always lets a queued message be edited or discarded", () => {
    expect(offer({ queued: true, state: "attention" })).toBe("queued");
    expect(offer({ queued: true, status: "exited", agent: false })).toBe("queued");
  });

  it("shows queue, or edit and discard, before ending the process", () => {
    const keys = (queue: QueueOffer) =>
      sessionMenuEntries({ status: "running" }, t, false, { queue, stall: null }).map(
        (entry) => entry.key,
      );
    expect(keys(null)).toEqual(["rename", "marker", "sep", "kill", "close"]);
    expect(keys("offer")).toEqual([
      "rename",
      "marker",
      "sep-queue",
      "queue",
      "sep",
      "kill",
      "close",
    ]);
    expect(keys("queued")).toEqual([
      "rename",
      "marker",
      "sep-queue",
      "queue",
      "discardQueue",
      "sep",
      "kill",
      "close",
    ]);
    const label = (queue: QueueOffer) => {
      const entry = sessionMenuEntries({ status: "running" }, t, false, {
        queue,
        stall: null,
      }).find((item) => item.key === "queue");
      return entry?.type === "action" ? entry.label : "";
    };
    expect(label("offer")).toBe(t("queue.add"));
    expect(label("queued")).toBe(t("queue.edit"));
  });
});

describe("stall hint entries", () => {
  it("appear only while the hint shows", () => {
    expect(stallOffer(false, false)).toBeNull();
    expect(stallOffer(true, false)).toEqual({ peek: true });
  });

  it("offer the last lines only for a session that is not on screen", () => {
    expect(stallOffer(true, true)).toEqual({ peek: false });
    const keys = (onScreen: boolean) =>
      sessionMenuEntries({ status: "running" }, t, false, {
        queue: null,
        stall: stallOffer(true, onScreen),
      }).map((entry) => entry.key);
    expect(keys(false)).toEqual([
      "rename",
      "marker",
      "sep-stall",
      "peek",
      "interrupt",
      "sep",
      "kill",
      "close",
    ]);
    expect(keys(true)).not.toContain("peek");
    expect(keys(true)).toContain("interrupt");
  });

  it("names Ctrl+C in the UI language", () => {
    const entry = sessionMenuEntries({ status: "running" }, createTranslator("de"), false, {
      queue: null,
      stall: { peek: false },
    }).find((item) => item.key === "interrupt");
    expect(entry?.type === "action" ? entry.label : "").toBe("Strg+C senden");
  });

  it("offers linking only where it works, and editing and unlinking to members", () => {
    const keys = (link: "link" | "member" | null | undefined) =>
      sessionMenuEntries({ status: "running" }, t, false, {
        queue: null,
        stall: null,
        partner: true,
        ...(link === undefined ? {} : { link }),
      }).map((entry) => entry.key);
    expect(keys(undefined)).toEqual(["rename", "marker", "partner", "sep", "kill", "close"]);
    expect(keys(null)).toEqual(keys(undefined));
    expect(keys("link")).toEqual(["rename", "marker", "partner", "link", "sep", "kill", "close"]);
    expect(keys("member")).toEqual([
      "rename",
      "marker",
      "partner",
      "linkEdit",
      "linkHistory",
      "unlink",
      "sep",
      "kill",
      "close",
    ]);
    const labels = sessionMenuEntries({ status: "running" }, t, false, {
      queue: null,
      stall: null,
      link: "member",
    }).map((entry) => (entry.type === "action" ? entry.label : ""));
    expect(labels).toContain("Edit link group …");
    expect(labels).toContain("Link group history …");
    expect(labels).toContain("Unlink");
    const german = createTranslator("de");
    const link = sessionMenuEntries({ status: "running" }, german, false, {
      queue: null,
      stall: null,
      link: "link",
    })[2];
    expect(link?.type === "action" ? link.label : "").toBe("Verknüpfen mit …");
    // The quicker way is named beside the entry.
    expect(link?.type === "action" ? link.shortcut : "").toMatch(/^(Strg|⌘)\+Klick$/);
  });

  it("offers a member of a group that can grow Link with … besides editing", () => {
    const keys = sessionMenuEntries({ status: "running" }, t, false, {
      queue: null,
      stall: null,
      link: "grow",
    }).map((entry) => entry.key);
    expect(keys).toEqual([
      "rename",
      "marker",
      "link",
      "linkEdit",
      "linkHistory",
      "unlink",
      "sep",
      "kill",
      "close",
    ]);
  });
});

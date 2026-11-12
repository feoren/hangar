import { describe, expect, it } from "vitest";

import { parseClaimsSnapshot, type ClaimsSnapshot } from "./claims";
import {
  DEFAULT_WINDOW_PREFS,
  EMPTY_PANEL_CONTEXT,
  isSendable,
  panelContextPayload,
  panelSessionIsAgent,
  panelSessionName,
  parsePanelCommand,
  parsePanelContext,
  parsePanelMessage,
  parseWindowPrefs,
  sendableText,
  type PanelContext,
} from "./panelBridge";
import { EMPTY_ACTIVITY, parseTeamActivity, type TeamActivity } from "./team";

describe("parsePanelMessage", () => {
  it("accepts the known kinds", () => {
    expect(parsePanelMessage({ kind: "sendToTerminal", text: "hi\n", sessionId: 3 })).toEqual({
      kind: "sendToTerminal",
      text: "hi\n",
      sessionId: 3,
    });
    expect(parsePanelMessage({ kind: "sendToTerminal", text: "hi" })).toEqual({
      kind: "sendToTerminal",
      text: "hi",
      sessionId: null,
    });
    expect(parsePanelMessage({ kind: "notesChanged", folderKey: "general" })).toEqual({
      kind: "notesChanged",
      folderKey: "general",
    });
    expect(parsePanelMessage({ kind: "showMain" })).toEqual({ kind: "showMain" });
    expect(parsePanelMessage({ kind: "ready" })).toEqual({ kind: "ready" });
    expect(parsePanelMessage({ kind: "showSession", sessionId: 7 })).toEqual({
      kind: "showSession",
      sessionId: 7,
    });
  });

  it("rejects anything the backend would not send", () => {
    expect(parsePanelMessage({ kind: "sendToTerminal", text: "" })).toBeNull();
    expect(parsePanelMessage({ kind: "sendToTerminal", text: "a\u001b[201~b" })).toBeNull();
    expect(parsePanelMessage({ kind: "sendToTerminal", text: "a", sessionId: 1.5 })).toBeNull();
    expect(parsePanelMessage({ kind: "notesChanged", folderKey: "../x" })).toBeNull();
    expect(parsePanelMessage({ kind: "showSession" })).toBeNull();
    expect(parsePanelMessage({ kind: "showSession", sessionId: -1 })).toBeNull();
    expect(parsePanelMessage({ kind: "showSession", sessionId: "1" })).toBeNull();
    expect(parsePanelMessage({ kind: "trayQuit" })).toBeNull();
    expect(parsePanelMessage("showMain")).toBeNull();
    expect(parsePanelMessage(null)).toBeNull();
  });
});

describe("sendable text", () => {
  it("allows tab and line breaks only", () => {
    expect(isSendable("a\tb\r\nc")).toBe(true);
    expect(isSendable("a\u0007")).toBe(false);
    expect(isSendable("a\u009b")).toBe(false);
    expect(isSendable("")).toBe(false);
    expect(sendableText("a\u001b[31mb\tc\n")).toBe("a[31mb\tc\n");
  });
});

describe("parsePanelCommand", () => {
  it("accepts known commands", () => {
    expect(parsePanelCommand({ kind: "quickNote" })).toEqual({ kind: "quickNote" });
    expect(parsePanelCommand({ kind: "flush" })).toEqual({ kind: "flush" });
    expect(parsePanelCommand({ kind: "capture", text: "x", source: "Claude" })).toEqual({
      kind: "capture",
      text: "x",
      source: "Claude",
    });
    expect(parsePanelCommand({ kind: "show", tab: "files" })).toEqual({
      kind: "show",
      tab: "files",
    });
  });

  it("rejects broken ones", () => {
    expect(parsePanelCommand({ kind: "capture", text: "", source: "" })).toBeNull();
    expect(parsePanelCommand({ kind: "show", tab: "settings" })).toBeNull();
    expect(parsePanelCommand({ kind: "show" })).toBeNull();
    expect(parsePanelCommand({ kind: "eval" })).toBeNull();
    // Docking back is the panel window's own close button, never a command.
    expect(parsePanelCommand({ kind: "close" })).toBeNull();
    expect(parsePanelCommand([])).toBeNull();
  });
});

const CLAIMS_RAW = {
  enabled: true,
  sessions: [
    {
      sessionId: 1,
      state: { kind: "ready" },
      root: "C:\\work\\wt a",
      base: "0123456789abcdef0123456789abcdef01234567",
      sharedWith: [2],
      claims: [
        {
          path: "src/x.ts",
          kind: "modified",
          wholeFile: false,
          ranges: [{ start: 10, end: 12 }],
          area: "src",
        },
      ],
      truncated: false,
      level: "overlappingLines",
      overlapCount: 1,
      overlapsTruncated: false,
    },
  ],
  overlaps: [
    {
      sessionId: 1,
      otherId: 2,
      level: "overlappingLines",
      path: "src/x.ts",
      area: null,
      lines: [{ start: 10, end: 15 }],
    },
  ],
};

const ACTIVITY_RAW = {
  linked: [{ root: "a".repeat(40), folder: "c:/src/shop/.git", name: "shop" }],
  peers: [
    {
      member: "bob-0123456789abcdef",
      device: "d1",
      name: "Bob",
      own: false,
      presence: "active",
      sessions: [
        {
          key: "k1",
          agent: "Codex",
          title: "Login",
          state: "working",
          repo: "a".repeat(40),
          files: ["src/auth.rs"],
          moreFiles: 0,
        },
      ],
    },
  ],
  overlaps: [
    {
      sessionId: 1,
      member: "bob-0123456789abcdef",
      device: "d1",
      otherSession: "k1",
      name: "Bob",
      agent: "Codex",
      title: "Login",
      level: "sameFile",
      path: "src/auth.rs",
      lines: [],
      files: 1,
    },
  ],
  divergent: [{ member: "carol-0123456789abcdef", device: "device-0123456789", name: "Carol" }],
  sessions: [1],
  sessionRepos: [{ sessionId: 1, repo: "a".repeat(40) }],
};

function claims(): ClaimsSnapshot {
  const parsed = parseClaimsSnapshot(CLAIMS_RAW);
  if (parsed === null) throw new Error("claims fixture does not parse");
  return parsed;
}

function activity(): TeamActivity {
  const parsed = parseTeamActivity(ACTIVITY_RAW);
  if (parsed === null) throw new Error("activity fixture does not parse");
  return parsed;
}

function context(): PanelContext {
  return {
    activeId: 1,
    cwd: "C:\\work\\wt a",
    trigger: "working",
    sessions: [
      {
        id: 1,
        title: "Claude",
        running: true,
        program: "Claude Code",
        repo: String.raw`C:workshop`,
        agent: true,
      },
      { id: 2, title: "Codex", running: false, program: "", repo: null, agent: true },
      { id: 3, title: "pwsh", running: true, program: "PowerShell", repo: null, agent: false },
    ],
    claims: claims(),
    activities: new Map([["project-1", activity()]]),
  };
}

/** As the event carries it: through JSON, like Tauri's IPC. */
function roundTrip(value: PanelContext): unknown {
  const parsed: unknown = JSON.parse(JSON.stringify(panelContextPayload(value)));
  return parsed;
}

describe("panel context", () => {
  it("survives the trip from the main window", () => {
    const sent = context();
    const received = parsePanelContext(roundTrip(sent));
    expect(received).not.toBeNull();
    expect(received?.activeId).toBe(1);
    expect(received?.cwd).toBe("C:\\work\\wt a");
    expect(received?.trigger).toBe("working");
    expect(received?.sessions).toEqual(sent.sessions);
    expect(received?.claims).toEqual(sent.claims);
  });

  it("trims team activities to what the tabs read", () => {
    const received = parsePanelContext(roundTrip(context()));
    const team = received?.activities.get("project-1");
    expect(team).toEqual({
      ...EMPTY_ACTIVITY,
      peers: activity().peers,
      overlaps: activity().overlaps,
      sessionRepos: activity().sessionRepos,
    });
    // Not needed by the panel, so not sent: linked repositories, other devices, tasks.
    expect(team?.linked).toEqual([]);
    expect(team?.divergent).toEqual([]);
  });

  it("accepts the empty context without a session", () => {
    expect(parsePanelContext(roundTrip(EMPTY_PANEL_CONTEXT))).toEqual(EMPTY_PANEL_CONTEXT);
  });

  it("refuses malformed payloads as a whole", () => {
    const good = panelContextPayload(context());
    const broken: unknown[] = [
      null,
      [],
      "context",
      { ...good, activeId: -1 },
      { ...good, activeId: 1.5 },
      { ...good, cwd: "" },
      { ...good, cwd: 3 },
      { ...good, trigger: "x".repeat(65) },
      { ...good, trigger: undefined },
      { ...good, sessions: [{ id: 1, title: "a" }] },
      {
        ...good,
        sessions: [{ id: "1", title: "a", running: true, program: "", repo: null, agent: true }],
      },
      {
        ...good,
        sessions: [{ id: 1, title: "a", running: true, program: 3, repo: null, agent: true }],
      },
      {
        ...good,
        sessions: [{ id: 1, title: "a", running: true, program: "", repo: 7, agent: true }],
      },
      { ...good, sessions: [{ id: 1, title: "a", running: true, program: "", repo: null }] },
      {
        ...good,
        sessions: [{ id: 1, title: "a", running: true, program: "", repo: null, agent: "yes" }],
      },
      { ...good, sessions: "all" },
      { ...good, claims: { enabled: "yes", sessions: [], overlaps: [] } },
      { ...good, claims: undefined },
      { ...good, teams: [{ projectId: "", activity: ACTIVITY_RAW }] },
      { ...good, teams: [{ projectId: "p", activity: { ...ACTIVITY_RAW, peers: null } }] },
      {
        ...good,
        teams: [
          { projectId: "p", activity: ACTIVITY_RAW },
          { projectId: "p", activity: ACTIVITY_RAW },
        ],
      },
      { ...good, teams: undefined },
    ];
    for (const value of broken) expect(parsePanelContext(value)).toBeNull();
  });

  it("names sessions as the main window does", () => {
    const { sessions } = context();
    expect(panelSessionName(sessions, 2)).toBe("Codex");
    expect(panelSessionName(sessions, 9)).toBe("#9");
  });

  it("tells agent sessions from shells", () => {
    const { sessions } = context();
    expect(panelSessionIsAgent(sessions, 1)).toBe(true);
    expect(panelSessionIsAgent(sessions, 3)).toBe(false);
    expect(panelSessionIsAgent(sessions, 9)).toBe(false);
    expect(panelSessionIsAgent(sessions, null)).toBe(false);
  });
});

describe("window prefs", () => {
  it("reads them defensively", () => {
    expect(parseWindowPrefs(null)).toEqual(DEFAULT_WINDOW_PREFS);
    expect(parseWindowPrefs("{")).toEqual(DEFAULT_WINDOW_PREFS);
    expect(parseWindowPrefs("[]")).toEqual(DEFAULT_WINDOW_PREFS);
    expect(
      parseWindowPrefs(
        JSON.stringify({
          detached: true,
          onTop: true,
          tab: "changes",
          bounds: { x: -900, y: 40, width: 460, height: 680 },
        }),
      ),
    ).toEqual({
      detached: true,
      onTop: true,
      tab: "changes",
      bounds: { x: -900, y: 40, width: 460, height: 680 },
    });
  });

  it("drops broken fields one by one", () => {
    expect(
      parseWindowPrefs(
        JSON.stringify({ detached: "yes", tab: "settings", bounds: { x: 0, y: 0, width: 0 } }),
      ),
    ).toEqual(DEFAULT_WINDOW_PREFS);
    expect(
      parseWindowPrefs(JSON.stringify({ bounds: { x: 0, y: 0, width: -5, height: 10 } })).bounds,
    ).toBeNull();
  });

  it("carries over the notes window prefs from before", () => {
    // `hangar.notesWindow` held { detached, onTop }; the tab and place start fresh.
    expect(parseWindowPrefs(JSON.stringify({ detached: true, onTop: false }))).toEqual({
      ...DEFAULT_WINDOW_PREFS,
      detached: true,
    });
  });
});

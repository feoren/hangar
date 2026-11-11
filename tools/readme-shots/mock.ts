// In-memory stand-in for the Tauri backend, used only for the README screenshots. It answers the
// IPC calls the frontend makes and plays a scripted, fictional scene in the project "acme-shop":
// Claude Code working with sub-agents next to Codex CLI in its own worktree, Aider waiting for an
// answer, a PowerShell, a signed-in account with two teammates online, and a few notes. The
// driver (run.mjs) picks what each screenshot shows by clicking through the real frontend.
// Nothing here talks to a real process, file or network, and every name in it is made up.

declare const __README_SHOTS_VERSION__: string;

interface ReadmeShotsControl {
  /** Set by the driver right before the screenshot: spinners stop and the clock stands still. */
  freeze: boolean;
}

declare global {
  interface Window {
    __readmeShots: ReadmeShotsControl;
    __TAURI_INTERNALS__: unknown;
    __TAURI_EVENT_PLUGIN_INTERNALS__: unknown;
  }
}

type Args = Record<string, unknown>;

const control: ReadmeShotsControl = { freeze: false };
window.__readmeShots = control;

// A fixed clock, so every time the app shows (run times, "since 09:12") is the same in each run:
// it starts at FIXED_START and stands at SHOT_TIME once the driver freezes the scene.
const FIXED_START = Date.UTC(2026, 10, 10, 9, 30, 0);
const SHOT_TIME = FIXED_START + 30_000;
const realNow = Date.now.bind(Date);
const clockOffset = FIXED_START - realNow();
Date.now = () => (control.freeze ? SHOT_TIME : Math.min(realNow() + clockOffset, SHOT_TIME));
/** A moment `ms` before the screenshot. */
const ago = (ms: number) => SHOT_TIME - ms;
const MINUTE = 60_000;

const ROOT = "C:\\src\\acme-shop";
const WORKTREE = "C:\\src\\acme-shop-cart-rounding";
const WORKTREE_BRANCH = "hangar/cart-rounding";

function catalogProfile(id: string, name: string, resumeArgs: string[] | null) {
  return {
    id,
    name,
    command: id,
    args: [],
    cwd: null,
    icon: id,
    source: "catalog",
    installed: true,
    homepage: null,
    installHint: null,
    resumeArgs,
  };
}

const profiles = [
  catalogProfile("claude", "Claude Code", ["--continue"]),
  catalogProfile("codex", "Codex CLI", ["resume", "--last"]),
  catalogProfile("gemini", "Gemini CLI", null),
  catalogProfile("aider", "Aider", null),
  {
    id: "pwsh",
    name: "PowerShell",
    command: "pwsh",
    args: [],
    cwd: null,
    icon: null,
    source: "user",
    installed: true,
    homepage: null,
    installHint: null,
  },
];
const versions = {
  claude: "2.1.319",
  codex: "0.167.0",
  gemini: "0.31.0",
  aider: "0.86.2",
  pwsh: "7.6.9",
};

const uuid = (n: number) => `aaaaaaaa-0000-4000-8000-${String(n).padStart(12, "0")}`;
const workspace = {
  version: 1,
  activeKey: uuid(1),
  sessions: [
    {
      key: uuid(1),
      profileId: "claude",
      cwd: ROOT,
      title: "Claude Code",
      order: 0,
      color: null,
      emoji: null,
      worktree: null,
      cliSessionId: "cccccccc-0000-4000-8000-000000000001",
    },
    {
      key: uuid(2),
      profileId: "codex",
      cwd: WORKTREE,
      title: "Codex CLI",
      order: 1,
      color: null,
      emoji: null,
      worktree: { path: WORKTREE, branch: WORKTREE_BRANCH, repoRoot: ROOT },
    },
    {
      key: uuid(3),
      profileId: "aider",
      cwd: ROOT,
      title: "Aider",
      order: 2,
      color: null,
      emoji: null,
      worktree: null,
    },
    {
      key: uuid(4),
      profileId: "pwsh",
      cwd: ROOT,
      title: "PowerShell",
      order: 3,
      color: null,
      emoji: null,
      worktree: null,
    },
  ],
};
/** Minutes each session has been running, in creation order. */
const AGE_MINUTES = [52, 47, 31, 64];

// ANSI helpers
const E = "\x1b[";
const dim = (s: string) => `${E}2m${s}${E}22m`;
const bold = (s: string) => `${E}1m${s}${E}22m`;
const fg = (n: number, s: string) => `${E}${String(n)}m${s}${E}39m`;
const green = (s: string) => fg(32, s);
const red = (s: string) => fg(31, s);
const cyan = (s: string) => fg(36, s);
const yellow = (s: string) => fg(33, s);
const magenta = (s: string) => fg(35, s);
const lines = (l: string[]) => l.join("\r\n");

const CLEAR = "\x1b[2J\x1b[3J\x1b[H";
const SAVE_CURSOR = "\x1b7";
const RESTORE_CURSOR = "\x1b8";

const CLAUDE_START_SECONDS = 112;
function claudeSpinner(seconds: number, frame: number): string {
  const glyph = ["✻", "✽", "✶", "✳"][frame % 4] ?? "✻";
  const time = `${String(Math.floor(seconds / 60))}m ${String(seconds % 60)}s`;
  return magenta(`${glyph} Waiting for 2 agents…`) + dim(` (${time} · esc to interrupt)`);
}

const claudeOut = lines([
  dim("> ") + "Add retries with backoff to the payment",
  dim("  ") + "webhook and cover them with tests",
  "",
  "● I'll split this up: one agent maps where the",
  "  webhook is called, one writes the tests while",
  "  I change the handler.",
  "",
  green("●") + " " + bold("Task") + "(Map webhook call sites)",
  dim("  ⎿  Done (7 tool uses · 41s)"),
  "",
  green("●") + " " + bold("Update") + "(src/payments/webhook.ts)",
  dim("  ⎿  Updated with 14 additions and 3 removals"),
  dim("      38 ") + red("-  const res = await send(event);"),
  dim("      38 ") + green("+  const delays = [250, 500, 1000, 2000];"),
  dim("      39 ") + green("+  for (const delay of delays) {"),
  dim("      40 ") + green("+    const res = await send(event);"),
  dim("      41 ") + green("+    if (res.ok) return res;"),
  dim("      42 ") + green("+    await sleep(delay);"),
  dim("      43 ") + green("+  }"),
  "",
  magenta("●") + " " + bold("Task") + "(Write retry tests)",
  dim("  ⎿  ") + "Bash(pnpm vitest run src/payments)",
  dim("     +9 more tool uses"),
  "",
  magenta("●") + " " + bold("Task") + "(Check error logging)",
  dim("  ⎿  ") + "Grep(logger\\.error)",
  dim("     +3 more tool uses"),
  "",
  SAVE_CURSOR + claudeSpinner(CLAUDE_START_SECONDS, 0),
  "",
  dim("─".repeat(52)),
  // Like Claude Code itself: the terminal cursor is hidden, the prompt draws its own.
  `> ${E}7m ${E}27m`,
  dim("─".repeat(52)),
  dim("  ? for shortcuts"),
]);
const HIDE_CURSOR = `${E}?25l`;

const codexOut = lines([
  dim("╭" + "─".repeat(42) + "╮"),
  dim("│ ") + bold(">_ Codex") + dim(" (v0.167.0)") + " ".repeat(22) + dim("│"),
  dim("│ ") + "model:     " + cyan("default") + "   " + dim("/model to change") + "    " + dim("│"),
  dim("│ ") + "directory: " + WORKTREE + dim("│"),
  dim("╰" + "─".repeat(42) + "╯"),
  "",
  dim("› ") + "Find out why the cart total is one cent",
  dim("  ") + "off for discounted items",
  "",
  "• " + bold("Explored"),
  dim("  └ ") + "Search " + cyan("discount") + " in src/cart",
  dim("    ") + "Read total.ts, discount.ts, total.test.ts",
  "",
  "• Each line is rounded before summing. With",
  "  three items at 33.33 % off, the rounding",
  "  errors add up to 0.01.",
  "",
  "• " +
    bold("Edited") +
    " src/cart/total.ts " +
    dim("(") +
    green("+5") +
    " " +
    red("-3") +
    dim(")"),
  dim("    21 ") + red("-   sum += round(line.net);"),
  dim("    21 ") + green("+   sum += line.net;"),
  dim("    22 ") + green("+ }"),
  dim("    23 ") + green("+ return round(sum);"),
  "",
  "• " + bold("Ran") + " pnpm test cart",
  dim("  └ ") + green("✓") + " 23 passed " + dim("(1.4 s)"),
  "",
  "• " + bold("Edited") + " src/cart/total.test.ts " + dim("(") + green("+9") + dim(")"),
  dim("    48 ") + green('+ it("sums discounted lines exactly", () => {'),
  dim("    49 ") + green("+   const cart = lines(3, 9.99, 1 / 3);"),
  dim("    50 ") + green("+   expect(total(cart)).toBe(19.98);"),
  dim("    51 ") + green("+ });"),
  "",
  "• " + bold("Running") + " pnpm test cart",
  "",
  SAVE_CURSOR,
]);
const codexFooter = lines([
  "",
  "",
  dim("› ") + dim("Ask for a follow-up change"),
  "",
  dim("  ⏎ send   ⇧⏎ newline   ⌃T transcript"),
]);

const aiderOut = lines([
  dim("Aider v0.86.2"),
  dim("Git repo: .git with 214 files"),
  dim("Repo-map: using 4096 tokens"),
  "",
  green("> Translate the checkout emails to German and French"),
  "",
  "I need to edit these files:",
  "",
  "src/emails/checkout.de.json",
  "src/emails/checkout.fr.json",
  "",
  yellow("Add these files to the chat? (Y)es/(N)o [Yes]: "),
]);

const pwshOut = lines([
  "PowerShell 7.6.9",
  "",
  green(`PS ${ROOT}> `) + "git status --short",
  " M src/payments/webhook.ts",
  " M src/payments/webhook.test.ts",
  green(`PS ${ROOT}> `),
]);

const CODEX_START_SECONDS = 41;
function codexSpinner(seconds: number, frame: number): string {
  const glyph = frame % 2 === 0 ? "◦" : "•";
  return `${glyph} ${bold("Working")}${dim(` (${String(seconds)}s • esc to interrupt)`)}`;
}

const scripts: Record<string, string> = {
  claude: CLEAR + HIDE_CURSOR + claudeOut,
  codex: CLEAR + codexOut + codexSpinner(CODEX_START_SECONDS, 0) + codexFooter,
  aider: CLEAR + aiderOut,
  pwsh: CLEAR + pwshOut,
};

// IPC plumbing: Tauri delivers channel messages and events through registered callbacks.
const callbacks = new Map<number, (value: unknown) => void>();
let nextCallback = 1;
const channelOfSession = new Map<number, number>();
const channelIndex = new Map<number, number>();

function send(channel: number, message: unknown): void {
  const index = channelIndex.get(channel) ?? 0;
  channelIndex.set(channel, index + 1);
  callbacks.get(channel)?.({ message, index });
}

function emit(sessionId: number, event: unknown): void {
  const channel = channelOfSession.get(sessionId);
  if (channel !== undefined) send(channel, event);
}

function output(sessionId: number, data: string): void {
  emit(sessionId, { type: "output", data, bytes: data.length });
}

/** Channel id from the `onEvent` argument: a Channel object (with `id`) or its serialized form. */
function channelId(value: unknown): number | undefined {
  if (typeof value === "number") return value;
  if (typeof value === "string") return Number(value.replace("__CHANNEL__:", ""));
  if (typeof value === "object" && value !== null && "id" in value) {
    return typeof value.id === "number" ? value.id : undefined;
  }
  return undefined;
}

/** A spinner that ticks until the driver freezes the scene, so the session reads as working. */
function keepTicking(sessionId: number, start: number, frameOf: (s: number, f: number) => string) {
  let seconds = start;
  let frame = 0;
  setInterval(() => {
    if (control.freeze) return;
    seconds += 1;
    frame += 1;
    output(sessionId, frameOf(seconds, frame));
  }, 700);
}

const sessions = new Map<number, Record<string, unknown>>();
let nextSessionId = 0;

function createSession(args: Args): unknown {
  const id = ++nextSessionId;
  const profileId = typeof args.profileId === "string" ? args.profileId : "";
  const profile = profiles.find((p) => p.id === profileId);
  const info = {
    id,
    profileId,
    title: profile?.name ?? profileId,
    cwd: typeof args.cwd === "string" ? args.cwd : ROOT,
    status: "running",
    exitCode: null,
    createdAt: ago((AGE_MINUTES[id - 1] ?? 5) * MINUTE),
    resumed: args.resume === true,
    cliSessionId: typeof args.cliSessionId === "string" ? args.cliSessionId : null,
    subAgents: profileId === "claude",
    gate: "none",
  };
  const channel = channelId(args.onEvent);
  if (channel !== undefined) channelOfSession.set(id, channel);
  sessions.set(id, info);
  setTimeout(() => {
    output(id, scripts[profileId] ?? "");
    if (profileId === "codex") {
      keepTicking(
        id,
        CODEX_START_SECONDS,
        (s, f) => `${RESTORE_CURSOR}${E}2K${codexSpinner(s, f)}`,
      );
    }
    if (profileId === "claude") {
      keepTicking(
        id,
        CLAUDE_START_SECONDS,
        (s, f) => `${RESTORE_CURSOR}${E}2K${claudeSpinner(s, f)}`,
      );
    }
    // A bell makes Aider show up as "asks you something" in the sidebar.
    if (profileId === "aider") setTimeout(() => output(id, "\x07"), 300);
  }, 600);
  return info;
}

// Sub-agents of the Claude Code session: one finished, two still running.
const subAgents = [
  {
    id: "agent-map",
    description: "Map webhook call sites",
    agentType: "Explore",
    spawnDepth: 1,
    status: "finished",
    startedAt: ago(3 * MINUTE + 20_000),
    lastActivity: ago(2 * MINUTE + 39_000),
    tool: "Grep",
    excerpt: "Two callers: the order service and the refund job. Both pass the raw event.",
  },
  {
    id: "agent-tests",
    description: "Write retry tests",
    agentType: "general-purpose",
    spawnDepth: 1,
    status: "running",
    startedAt: ago(MINUTE + 52_000),
    lastActivity: ago(2_000),
    tool: "Bash",
    excerpt: "The give-up case expects 4 calls but sees 5.",
  },
  {
    id: "agent-logging",
    description: "Check error logging",
    agentType: "Explore",
    spawnDepth: 1,
    status: "running",
    startedAt: ago(48_000),
    lastActivity: ago(5_000),
    tool: "Grep",
    excerpt: null,
  },
];

const subAgentLogs: Record<string, unknown[]> = {
  "agent-tests": [
    {
      kind: "prompt",
      text:
        "Write Vitest tests for the new retry loop in src/payments/webhook.ts: retry on 503, " +
        "give up after 4 attempts, keep the event id stable. Use fake timers and do not change " +
        "the handler.",
    },
    { kind: "text", text: "I'll read the handler and the existing tests first." },
    { kind: "tool", name: "Read", detail: "src/payments/webhook.ts" },
    { kind: "result", text: 'import { send } from "./client";', moreLines: 83, error: false },
    { kind: "tool", name: "Read", detail: "src/payments/webhook.test.ts" },
    {
      kind: "result",
      text: 'import { describe, expect, it, vi } from "vitest";',
      moreLines: 41,
      error: false,
    },
    { kind: "tool", name: "Grep", detail: "sleep\\( in src/payments" },
    { kind: "result", text: "src/payments/webhook.ts:42", moreLines: 0, error: false },
    {
      kind: "text",
      text:
        "The handler waits with sleep() between attempts, so fake timers can skip the delays. " +
        "I'll add three cases next to the existing ones.",
    },
    { kind: "tool", name: "Edit", detail: "src/payments/webhook.test.ts" },
    { kind: "result", text: "Updated with 38 additions", moreLines: 0, error: false },
    { kind: "tool", name: "Bash", detail: "pnpm vitest run src/payments" },
    {
      kind: "result",
      text: "FAIL  gives up after 4 attempts: expected 4 calls, got 5",
      moreLines: 12,
      error: true,
    },
    {
      kind: "text",
      text: "The test counts the first try as a retry. Fixing the expectation, the handler is right.",
    },
    { kind: "tool", name: "Edit", detail: "src/payments/webhook.test.ts" },
    { kind: "result", text: "Updated with 1 addition and 1 removal", moreLines: 0, error: false },
    { kind: "tool", name: "Bash", detail: "pnpm vitest run src/payments" },
    { kind: "result", text: "Test Files  1 passed (1)", moreLines: 2, error: false },
    { kind: "text", text: "All 17 tests pass. Checking the timing assertions once more." },
    { kind: "tool", name: "Read", detail: "src/payments/webhook.test.ts" },
  ],
};

/** The watch's channel per session, and the sub-agent whose log it streams. */
const subAgentWatch = new Map<number, number>();

function sendSubAgentLog(channel: number, focus: unknown): void {
  if (typeof focus !== "string") return;
  send(channel, { type: "log", agentId: focus, reset: true, entries: subAgentLogs[focus] ?? [] });
}

function watchSubAgents(args: Args): null {
  const sessionId = typeof args.sessionId === "number" ? args.sessionId : -1;
  const channel = channelId(args.onEvent);
  if (channel === undefined) return null;
  subAgentWatch.set(sessionId, channel);
  setTimeout(() => {
    send(channel, { type: "snapshot", located: true, agents: subAgents });
    sendSubAgentLog(channel, args.focus);
  }, 50);
  return null;
}

function focusSubAgent(args: Args): boolean {
  const sessionId = typeof args.sessionId === "number" ? args.sessionId : -1;
  const channel = subAgentWatch.get(sessionId);
  if (channel === undefined) return false;
  sendSubAgentLog(channel, args.agentId);
  return true;
}

// The account and the team: Sam, signed in, shares acme-shop with Mika and Jonas.
const PROJECT = "proj-acme-shop";
const accountView = {
  baseUrl: "https://account.example.com",
  official: true,
  devicesUrl: "https://account.example.com/devices",
  state: {
    kind: "signedIn",
    user: {
      id: "user-sam",
      email: "sam@example.com",
      name: "Sam Carter",
      seatsAccountTotal: 5,
      seatsAccountUsed: 3,
    },
  },
  otherSignIns: [],
};
const projects = [
  {
    id: PROJECT,
    name: "acme-shop",
    role: "owner",
    plan: "Team",
    seatsAccountTotal: 5,
    seatsAccountUsed: 3,
    pendingInvitations: 0,
    canInvite: true,
  },
];
const teamStatus = { state: "connected", epoch: 4, members: 3, devices: 4 };
const ACME_ROOT_COMMIT = "4e1f0c2a9b7d35e8c6a1f0b2d4c6e8a0b1c3d5e7";
const hash = (pair: string) => pair.repeat(32);
const teamSnapshot = {
  teams: [
    {
      projectId: PROJECT,
      status: teamStatus,
      activity: {
        linked: [{ root: ACME_ROOT_COMMIT, folder: `${ROOT}\\.git`, name: "acme-shop" }],
        peers: [
          {
            member: "7c41d9e2b08a4f15",
            device: "dev-mika-1",
            name: "Mika Rossi",
            own: false,
            presence: "active",
            sessions: [
              {
                key: "mika-1",
                agent: "Claude Code",
                title: "Checkout emails",
                state: "working",
                files: ["src/emails/checkout.de.json", "src/emails/checkout.fr.json"],
                moreFiles: 0,
              },
              {
                key: "mika-2",
                agent: "PowerShell",
                title: "Release notes",
                state: "idle",
                files: [],
                moreFiles: 0,
              },
            ],
          },
          {
            member: "b2e86f0a13c947d5",
            device: "dev-jonas-1",
            name: "Jonas Berg",
            own: false,
            presence: "active",
            sessions: [
              {
                key: "jonas-1",
                agent: "Codex CLI",
                title: "Search ranking",
                state: "waiting",
                files: ["src/search/rank.ts"],
                moreFiles: 0,
              },
              {
                key: "jonas-2",
                agent: "Gemini CLI",
                title: "API docs",
                state: "working",
                files: ["docs/api/orders.md", "docs/api/refunds.md", "docs/api/webhooks.md"],
                moreFiles: 0,
              },
            ],
          },
        ],
        overlaps: [],
        divergent: [],
        tasks: [
          {
            task: hash("a1"),
            title: "Webhook retries",
            area: "src/payments",
            member: "user-sam",
            device: "dev-sam-1",
            name: "Sam Carter",
            mine: true,
            own: false,
            since: ago(24 * MINUTE),
            expiresAt: null,
            expiring: false,
            request: null,
          },
          {
            task: hash("b2"),
            title: "Checkout emails",
            area: "src/emails",
            member: "7c41d9e2b08a4f15",
            device: "dev-mika-1",
            name: "Mika Rossi",
            mine: false,
            own: false,
            since: ago(41 * MINUTE),
            expiresAt: null,
            expiring: false,
            request: null,
          },
          {
            task: hash("c3"),
            title: "Search ranking",
            area: "src/search",
            member: "b2e86f0a13c947d5",
            device: "dev-jonas-1",
            name: "Jonas Berg",
            mine: false,
            own: false,
            since: ago(78 * MINUTE),
            expiresAt: null,
            expiring: false,
            request: null,
          },
        ],
        attempts: [],
        notices: [],
        sessions: [1, 2, 3, 4],
      },
    },
  ],
  pending: [],
  repos: [{ root: ACME_ROOT_COMMIT, folder: `${ROOT}\\.git`, name: "acme-shop" }],
};

// Notes of acme-shop: two shared with everyone who has the repository, two only on this computer.
const notes: Record<string, { id: string; title: string; content: string; age: number }[]> = {
  shared: [
    {
      id: "release-checklist",
      title: "Release checklist",
      age: 18,
      content: [
        "# Release checklist",
        "",
        "Before tagging a release of acme-shop:",
        "",
        "- [x] Webhook retries merged",
        "- [x] Cart rounding fix reviewed",
        "- [ ] Checkout emails in German and French",
        "- [ ] Bump the API version in `package.json`",
        "",
        "## Smoke test",
        "",
        "1. Place an order with a test card",
        "2. Check that the webhook arrives exactly once",
        "3. Refund it from the admin page",
        "",
        "Ask **Mika** before touching the email templates.",
        "",
      ].join("\n"),
    },
    {
      id: "provider-quirks",
      title: "Payment provider quirks",
      age: 260,
      content:
        "# Payment provider quirks\n\nThe sandbox answers 503 for a few seconds after midnight UTC.\n",
    },
  ],
  local: [
    {
      id: "cart-ideas",
      title: "Ideas for the cart",
      age: 95,
      content: "# Ideas for the cart\n\n- Save the cart for later\n- Show the discount per line\n",
    },
    {
      id: "standup",
      title: "Standup",
      age: 1500,
      content: "# Standup\n\n- Webhook retries: tests are being written\n- Next: refunds\n",
    },
  ],
};

function notesOf(folder: unknown) {
  if (typeof folder !== "object" || folder === null) return [];
  const path = "path" in folder ? folder.path : null;
  if (path !== ROOT) return [];
  return "shared" in folder && folder.shared === true ? notes.shared : notes.local;
}

function noteList(args: Args): unknown {
  return (notesOf(args.folder) ?? []).map((note, order) => ({
    id: note.id,
    title: note.title,
    updatedAt: ago(note.age * MINUTE),
    order,
  }));
}

function readNote(args: Args): unknown {
  const note = (notesOf(args.folder) ?? []).find((entry) => entry.id === args.id);
  return note?.content ?? null;
}

// Events: the frontend listens through the event plugin; the mock answers some right away.
const listeners = new Map<string, number[]>();
let nextEventId = 1;

function listen(args: Args): number {
  const event = typeof args.event === "string" ? args.event : "";
  const handler = typeof args.handler === "number" ? args.handler : -1;
  listeners.set(event, [...(listeners.get(event) ?? []), handler]);
  const id = nextEventId++;
  if (event === "team-changed") {
    setTimeout(() => {
      callbacks.get(handler)?.({ event, id, payload: teamSnapshot });
    }, 200);
  }
  return id;
}

const handlers: Record<string, (args: Args) => unknown> = {
  list_profiles: () => profiles,
  default_cwd: () => ROOT,
  list_sessions: () => [],
  load_workspace: () => workspace,
  save_workspace: () => null,
  load_scrollback: () => null,
  save_scrollback: () => null,
  detect_versions: () => versions,
  program_icons: () => ({}),
  check_program_updates: () => ({}),
  git_branches: (args) => {
    const paths = Array.isArray(args.paths) ? args.paths : [];
    return Object.fromEntries(
      paths.map((p: unknown) => [p, p === WORKTREE ? WORKTREE_BRANCH : "main"]),
    );
  },
  git_diff: () => ({ files: [] }),
  configure_tray: () => null,
  create_session: createSession,
  rename_session: (args) => {
    const info = typeof args.id === "number" ? sessions.get(args.id) : undefined;
    if (info && typeof args.title === "string") info.title = args.title;
    return info ?? null;
  },
  write_session: () => null,
  resize_session: () => null,
  ack_session: () => null,
  watch_sub_agents: watchSubAgents,
  focus_sub_agent: focusSubAgent,
  unwatch_sub_agents: (args) => {
    const sessionId = typeof args.sessionId === "number" ? args.sessionId : -1;
    return subAgentWatch.delete(sessionId);
  },
  list_note_folders: () => [
    { folderKey: "acme-shop", path: ROOT, noteCount: notes.local?.length ?? 0, updatedAt: ago(0) },
  ],
  list_notes: noteList,
  read_note: readNote,
  write_note: () => null,
  search_notes: () => [],
  list_trash: () => [],
  shared_notes_status: () => ({
    state: "unlocked",
    uncommitted: 0,
    ignored: false,
    unreadable: 0,
    resolved: [],
  }),
  account_status: () => accountView,
  account_projects: () => projects,
  team_status: () => teamStatus,
  team_pending_confirmations: () => [],
  team_session_states: () => undefined,
  // Features these scenes do not show (file claims, gates): nothing to report.
  profile_path: () => null,
  configure_claims: () => null,
  configure_gate: () => null,
  claims_snapshot: () => null,
  gate_snapshot: () => null,
};

function asArgs(value: unknown): Args {
  return typeof value === "object" && value !== null ? { ...value } : {};
}

window.__TAURI_INTERNALS__ = {
  metadata: {
    currentWindow: { label: "main" },
    currentWebview: { label: "main", windowLabel: "main" },
  },
  transformCallback(fn: (value: unknown) => void): number {
    const id = nextCallback++;
    callbacks.set(id, fn);
    return id;
  },
  unregisterCallback(id: number): void {
    callbacks.delete(id);
  },
  convertFileSrc: (path: string) => path,
  invoke(cmd: string, args: unknown): Promise<unknown> {
    if (cmd === "plugin:event|listen") return Promise.resolve(listen(asArgs(args)));
    if (cmd.startsWith("plugin:event|")) return Promise.resolve(null);
    if (cmd.startsWith("plugin:window|") || cmd.startsWith("plugin:webview|")) {
      if (/inner_size|outer_size/.test(cmd)) {
        return Promise.resolve({ width: window.innerWidth, height: window.innerHeight });
      }
      if (cmd.includes("scale_factor")) return Promise.resolve(1);
      if (cmd.includes("is_")) return Promise.resolve(false);
      return Promise.resolve(null);
    }
    if (cmd === "plugin:app|version") return Promise.resolve(__README_SHOTS_VERSION__);
    if (cmd.startsWith("plugin:")) return Promise.resolve(null);
    const handler = handlers[cmd];
    if (!handler) {
      console.warn(`readme-shots: unmocked command ${cmd}`);
      return Promise.resolve(null);
    }
    return Promise.resolve(handler(asArgs(args)));
  },
};
window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined };

export {};

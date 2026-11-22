// In-memory stand-in for the Tauri backend, used only for the README screenshots. It answers the
// IPC calls the frontend makes and plays a scripted, fictional day in the project "acme-shop":
// Claude Code working with a crew of Hangar's own sub-agents next to Codex CLI in its own worktree,
// a second Claude Code session started as the "hangar-security" sub-agent, Gemini CLI done with the
// API docs in the project "acme-docs", a PowerShell running the docs site's dev server and a
// signed-in account with two teammates online. Three of the agents form a link group with a lead.
// Per scene it adds what that scene shows: the sub-agents Hangar ships, messages of the group that
// wait for a click, the light flowing to working sessions, the notes quick access finds, the day's
// costs, a commit held by the secret guard or a long list of sessions on one line each.
// The driver (run.mjs) picks what each screenshot shows by clicking through the real frontend.
// Nothing here talks to a real process, file or network, and every name in it is made up.

declare const __README_SHOTS_VERSION__: string;

interface ReadmeShotsControl {
  /** Set by the driver right before the screenshot: spinners stop and the clock stands still. */
  freeze: boolean;
  /** Set once Gemini CLI has answered; a scene that shows it waits for this. */
  geminiAnswered: boolean;
}

declare global {
  interface Window {
    __readmeShots: ReadmeShotsControl;
    __TAURI_INTERNALS__: unknown;
    __TAURI_EVENT_PLUGIN_INTERNALS__: unknown;
  }
}

type Args = Record<string, unknown>;

const control: ReadmeShotsControl = { freeze: false, geminiAnswered: false };
window.__readmeShots = control;

/**
 * The scene the driver renders (run.mjs stores its name before the app starts). Most data is the
 * same in every scene; the few differences are named where they are made.
 */
const SCENE = ((): string => {
  try {
    const value: unknown = JSON.parse(localStorage.getItem("readmeShots.scene") ?? "null");
    return typeof value === "string" ? value : "overview";
  } catch {
    return "overview";
  }
})();
/** The one-line scene: a long day with many sessions in four projects. */
const MANY = SCENE === "one-line";
/** The scenes whose sidebar rows send output fast enough for the flow to run busy. */
const FAST_OUTPUT = SCENE === "overview" || SCENE === "data-flow";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;

// A fixed clock, so every time the app shows (run times, "since 09:12") is the same in each run:
// it starts at FIXED_START and stands at SHOT_TIME once the driver freezes the scene. In the
// overview it jumps WAIT_JUMP ahead a few seconds after the start, so the session that asked
// something has waited for 12 minutes by the time of the screenshot (waitHint.ts).
const WAIT_JUMP = SCENE === "overview" ? 12 * MINUTE : 0;
const JUMP_AFTER_MS = 4_000;
const FIXED_START = Date.UTC(2026, 10, 10, 9, 30, 0);
const SHOT_TIME = FIXED_START + 30_000 + WAIT_JUMP;
const realNow = Date.now.bind(Date);
const clockOffset = FIXED_START - realNow();
let jumped = false;
Date.now = () => {
  if (control.freeze) return SHOT_TIME;
  return Math.min(realNow() + clockOffset + (jumped ? WAIT_JUMP : 0), SHOT_TIME);
};
// `new Date()` without arguments reads the same clock (clock times and "today" in the app come
// from it); every other form of the constructor and every static method stay real.
window.Date = new Proxy(Date, {
  construct(target, args: unknown[], newTarget) {
    return Reflect.construct(target, args.length === 0 ? [Date.now()] : args, newTarget);
  },
});
if (WAIT_JUMP > 0) {
  window.setTimeout(() => {
    jumped = true;
  }, JUMP_AFTER_MS);
}
/** A moment `ms` before the screenshot. */
const ago = (ms: number) => SHOT_TIME - ms;

const ROOT = "C:\\src\\acme-shop";
const WORKTREE = "C:\\src\\acme-shop-cart-rounding";
const WORKTREE_BRANCH = "hangar/cart-rounding";
const DOCS = "C:\\src\\acme-docs";
const BILLING = "C:\\src\\acme-billing";
const WEB = "C:\\src\\acme-web";

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

const command = (labelKey: string, text: string, confirm = false) => ({
  labelKey,
  text,
  submit: true,
  confirm,
});

const profiles = [
  {
    ...catalogProfile("claude", "Claude Code", ["--continue"]),
    teamCapable: true,
    teamBlock: null,
    // As the agent catalog lists them for Claude Code.
    agentCommands: [
      command("compact", "/compact"),
      command("context", "/context"),
      command("usage", "/usage"),
      command("model", "/model"),
      command("clear", "/clear", true),
    ],
  },
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
const plainSession = (n: number, profileId: string, cwd: string, title: string) => ({
  key: uuid(n),
  profileId,
  cwd,
  title,
  order: n - 1,
  color: null,
  emoji: null,
  worktree: null,
});
/** The sidebar of every scene: five sessions, two of them Claude Code. */
const baseSessions = [
  {
    ...plainSession(1, "claude", ROOT, "Webhook retries"),
    cliSessionId: "cccccccc-0000-4000-8000-000000000001",
  },
  {
    // Renamed after its task, as people do with worktree sessions.
    ...plainSession(2, "codex", WORKTREE, "Cart rounding"),
    worktree: { path: WORKTREE, branch: WORKTREE_BRANCH, repoRoot: ROOT },
  },
  {
    // Claude Code started as the "hangar-security" sub-agent: its figure is the session's avatar,
    // its name the session's title.
    ...plainSession(3, "claude", ROOT, "hangar-security"),
    subAgent: "hangar-security",
    cliSessionId: "cccccccc-0000-4000-8000-000000000003",
  },
  plainSession(4, "gemini", DOCS, "Refund docs"),
  plainSession(5, "pwsh", DOCS, "Docs server"),
];
/** The one-line scene's busy day: ten more sessions in two more projects. */
const moreSessions = [
  plainSession(6, "claude", ROOT, "Search ranking"),
  plainSession(7, "codex", BILLING, "Invoice PDFs"),
  plainSession(8, "aider", DOCS, "Typo sweep"),
  plainSession(9, "claude", WEB, "Mobile layout"),
  plainSession(10, "gemini", BILLING, "Tax rules"),
  plainSession(11, "codex", WEB, "Flaky e2e tests"),
  plainSession(12, "pwsh", WEB, "Web dev server"),
  plainSession(13, "aider", BILLING, "CSV export"),
  plainSession(14, "claude", WEB, "Dark mode"),
  plainSession(15, "gemini", WEB, "Lighthouse audit"),
];
const mainSessions = MANY ? [...baseSessions, ...moreSessions] : baseSessions;
const workspace = { version: 1, activeKey: uuid(1), sessions: mainSessions };
/** Minutes each session has been running, in creation order. */
const AGE_MINUTES = [52, 47, 38, 31, 64, 26, 73, 12, 44, 19, 35, 88, 9, 57, 22];

// ANSI helpers
const E = "\x1b[";
const dim = (s: string) => `${E}2m${s}${E}22m`;
const bold = (s: string) => `${E}1m${s}${E}22m`;
const fg = (n: number, s: string) => `${E}${String(n)}m${s}${E}39m`;
const green = (s: string) => fg(32, s);
const red = (s: string) => fg(31, s);
const cyan = (s: string) => fg(36, s);
const magenta = (s: string) => fg(35, s);
const lines = (l: string[]) => l.join("\r\n");

const CLEAR = "\x1b[2J\x1b[3J\x1b[H";
const SAVE_CURSOR = "\x1b7";
const RESTORE_CURSOR = "\x1b8";
const HIDE_CURSOR = `${E}?25l`;
const rule = (width: number) => dim("─".repeat(width));

// Claude Code's own screen: a spinner line that ticks while it waits for its agents, the prompt
// between two rules and the hint under it. The terminal cursor is hidden, the prompt draws its own.
const CLAUDE_START_SECONDS = 112;
function claudeSpinner(seconds: number, frame: number): string {
  const glyph = ["✻", "✽", "✶", "✳"][frame % 4] ?? "✻";
  const time = `${String(Math.floor(seconds / 60))}m ${String(seconds % 60)}s`;
  return magenta(`${glyph} Working…`) + dim(` (${time} · esc to interrupt)`);
}
const claudePrompt = (width = 46) => [
  rule(width),
  `> ${E}7m ${E}27m`,
  rule(width),
  dim("  ? for shortcuts"),
];
const task = (agent: string, what: string) => `${green("●")} ${bold(agent)}(${what})`;

/** The lead of the link group at work: three sub-agents run while it waits for them. */
const claudeCrewOut = lines([
  dim("> ") + "Add retries with backoff to the payment",
  dim("  ") + "webhook and cover them with tests",
  "",
  "● I'll hand the handler to the backend,",
  "  the banner to the frontend and the tests",
  "  to the tester.",
  "",
  task("hangar-backend", "Retries with backoff"),
  dim("  ⎿  ") + "Update(src/payments/webhook.ts)",
  dim("     Updated with 14 additions, 3 removals"),
  dim("      38 ") + red("-  const res = await send(event);"),
  dim("      38 ") + green("+  const delays = [250, 500, 1000];"),
  dim("      39 ") + green("+  for (const delay of delays) {"),
  dim("      40 ") + green("+    const res = await send(event);"),
  dim("      41 ") + green("+    if (res.ok) return res;"),
  dim("      42 ") + green("+    await sleep(delay);"),
  dim("      43 ") + green("+  }"),
  "",
  task("hangar-frontend", "Retry banner"),
  dim("  ⎿  ") + "Edit(src/ui/RetryBanner.tsx)",
  "",
  task("hangar-tester", "Write retry tests"),
  dim("  ⎿  ") + "Bash(pnpm test payments)",
  dim("     +9 more tool uses"),
  "",
  SAVE_CURSOR + claudeSpinner(CLAUDE_START_SECONDS, 0),
  "",
  ...claudePrompt(),
]);

/** The second Claude Code session, started as the security agent, asks before it runs a command. */
const claudeAskOut = lines([
  dim("> ") + "Check the payment client for known advisories",
  "",
  "● I'll audit the production dependencies first.",
  "",
  dim("╭" + "─".repeat(44) + "╮"),
  dim("│ ") + bold("Bash command") + " ".repeat(31) + dim("│"),
  dim("│ ") + "  pnpm audit --prod" + " ".repeat(25) + dim("│"),
  dim("│ ") + dim("  Audit the production dependencies") + " ".repeat(8) + dim("│"),
  dim("│") + " ".repeat(44) + dim("│"),
  dim("│ ") + "Do you want to proceed?" + " ".repeat(20) + dim("│"),
  dim("│ ") + cyan("❯ 1. Yes") + " ".repeat(35) + dim("│"),
  dim("│ ") + "  2. Yes, and don't ask again for pnpm audit" + dim("│"),
  dim("│ ") + "  3. No, and tell Claude what to do instead" + " " + dim("│"),
  dim("╰" + "─".repeat(44) + "╯"),
]);

const codexOut = lines([
  dim("╭" + "─".repeat(34) + "╮"),
  dim("│ ") + bold(">_ Codex") + dim(" (v0.167.0)") + " ".repeat(14) + dim("│"),
  dim("│ ") + "model:  " + cyan("default") + "  " + dim("/model") + " ".repeat(10) + dim("│"),
  dim("╰" + "─".repeat(34) + "╯"),
  "",
  dim("› ") + "Find out why the cart total is one",
  dim("  ") + "cent off for discounted items",
  "",
  "• " + bold("Explored"),
  dim("  └ ") + "Search " + cyan("discount") + " in src/cart",
  dim("    ") + "Read total.ts, discount.ts",
  "",
  "• Each line is rounded before summing.",
  "  With three items at 33.33 % off, the",
  "  rounding errors add up to 0.01.",
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

// Gemini CLI works on the refund page for a while, then answers and rests. Output that ran this
// long counts as finished work, so the sidebar shows it "done" (attention.ts `sustainedWorkMs`).
const geminiBox = (mark: string, text: string, width = 40) => {
  return lines([
    dim("╭" + "─".repeat(width) + "╮"),
    dim("│ ") + mark + " " + text + " ".repeat(Math.max(0, width - 3 - text.length)) + dim("│"),
    dim("╰" + "─".repeat(width) + "╯"),
  ]);
};
const GEMINI_WORK_MS = 14_000;
const geminiSpinner = (seconds: number, frame: number) =>
  `${["⠋", "⠙", "⠹", "⠸"][frame % 4] ?? "⠋"} ${magenta("Thinking…")}${dim(` (esc to cancel, ${String(seconds)}s)`)}`;
const geminiOut = [
  lines([
    dim("Tips: ask questions, edit files, run commands."),
    "",
    cyan("> ") + "Document the refund endpoints in docs/api",
    "",
    magenta("✦") + " I'll read the OpenAPI spec and the order",
    "  pages first, then write the refund page.",
    "",
    geminiBox(green("✔"), "ReadFile openapi/refunds.yaml"),
    geminiBox(green("✔"), "ReadFile docs/api/orders.md"),
    "",
    SAVE_CURSOR + geminiSpinner(0, 0),
  ]),
  lines([
    geminiBox(green("✔"), "WriteFile docs/api/refunds.md"),
    "",
    magenta("✦") + " Done. docs/api/refunds.md covers create,",
    "  list and cancel, each with a curl example",
    "  and the error codes from the spec.",
    "",
    dim("╭" + "─".repeat(40) + "╮"),
    dim("│ ") + cyan(">") + dim("   Type your message or @path/to/file") + dim(" │"),
    dim("╰" + "─".repeat(40) + "╯"),
    dim("~\\src\\acme-docs (main*)") + "     " + dim("gemini-2.5-pro"),
  ]),
];

// The docs site's dev server: its address puts a globe on the row and on the project's card.
const pwshOut = (folder: string, project: string, port: number) =>
  lines([
    "PowerShell 7.6.9",
    "",
    green(`PS ${folder}> `) + "pnpm dev",
    "",
    dim(`> ${project}@1.4.0 dev ${folder}`),
    dim("> vite"),
    "",
    `  ${green("➜")}  ${bold("Local")}:   ${cyan(`http://localhost:${String(port)}/`)}`,
    `  ${green("➜")}  ${dim("Network: use --host to expose")}`,
    "",
  ]);

const CODEX_START_SECONDS = 41;
function codexSpinner(seconds: number, frame: number): string {
  const glyph = frame % 2 === 0 ? "◦" : "•";
  return `${glyph} ${bold("Working")}${dim(` (${String(seconds)}s • esc to interrupt)`)}`;
}

/** A short screen for the extra sessions of the one-line scene, which no pane shows. */
const promptOut = (text: string) => lines([dim("> ") + text, "", SAVE_CURSOR]);

/** The first screen of each session, by session number. */
const mainScripts: Record<number, string> = {
  1: CLEAR + HIDE_CURSOR + claudeCrewOut,
  2: CLEAR + codexOut + codexSpinner(CODEX_START_SECONDS, 0) + codexFooter,
  3: CLEAR + HIDE_CURSOR + claudeAskOut,
  4: CLEAR + HIDE_CURSOR + (geminiOut[0] ?? ""),
  5: CLEAR + pwshOut(DOCS, "acme-docs", 5173),
  6: CLEAR + promptOut("Rank exact title matches above partial ones"),
  7: CLEAR + promptOut("Render the invoice PDFs with the new logo"),
  8: CLEAR + promptOut("Fix the typos in docs/guides"),
  9: CLEAR + promptOut("The checkout breaks below 360 px, fix it"),
  10: CLEAR + promptOut("Which VAT rate applies to digital goods in the EU?"),
  11: CLEAR + promptOut("Find out why checkout.spec.ts fails every third run"),
  12: CLEAR + pwshOut(WEB, "acme-web", 5174),
  13: CLEAR + promptOut("Export the refunds as CSV for the accountant"),
  14: CLEAR + promptOut("Add a dark mode that follows the system"),
  15: CLEAR + promptOut("Run a Lighthouse audit on the landing page"),
};

/** Sessions that keep working with a spinner: Claude Code, Codex CLI and, in the long list, more. */
const WORKING = new Set(MANY ? [1, 2, 7, 9, 11, 14] : [1, 2]);
/** Sessions that ring the bell, so the sidebar shows them asking. */
const ASKING = new Set(MANY ? [3, 10] : [3]);
/** Sessions that work a little, then answer and rest: the sidebar shows them done. */
const FINISHING = new Set(MANY ? [4, 6, 15] : [4]);

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

/** Output for the terminal; `bytes` is what the tree flow counts (the PTY's chunk size). */
function output(sessionId: number, data: string, bytes = data.length): void {
  emit(sessionId, { type: "output", data, bytes });
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

/**
 * Bytes per spinner tick in the scenes that show the flow: a busy agent redraws much more than
 * its spinner line, so the rows show a rate in kilobytes per second.
 */
const TICK_BYTES: Record<number, number> = { 1: 13_300, 2: 4_100 };
const TICK_MS = 700;

/** A spinner that ticks until the driver freezes the scene, so the session reads as working. */
function keepTicking(
  sessionId: number,
  start: number,
  frameOf: (s: number, f: number) => string,
  until: number = Infinity,
): void {
  let seconds = start;
  let frame = 0;
  const timer = window.setInterval(() => {
    if (control.freeze) return;
    seconds += 1;
    frame += 1;
    if (frame * TICK_MS >= until) {
      window.clearInterval(timer);
      return;
    }
    const data = frameOf(seconds, frame);
    output(sessionId, data, FAST_OUTPUT ? (TICK_BYTES[sessionId] ?? data.length) : data.length);
  }, TICK_MS);
}

const sessions = new Map<number, Record<string, unknown>>();
let nextSessionId = 0;
const takenIds = new Set<number>();

/**
 * The backend id of a new session: its number in the workspace, so the script, the crew and the
 * meters stay with their session whichever the app starts first (it starts the active one
 * first); otherwise the next free number.
 */
function sessionId(profileId: string, cwd: string, subAgent: string | null): number {
  const index = mainSessions.findIndex(
    (entry, i) =>
      !takenIds.has(i + 1) &&
      entry.profileId === profileId &&
      entry.cwd === cwd &&
      ("subAgent" in entry ? entry.subAgent : null) === subAgent,
  );
  if (index >= 0) {
    takenIds.add(index + 1);
    return index + 1;
  }
  do nextSessionId += 1;
  while (takenIds.has(nextSessionId));
  takenIds.add(nextSessionId);
  return nextSessionId;
}

/** CPU share and resident memory (MB) per program, for the session rows. */
const RESOURCE_CPU: Record<string, number> = { claude: 12, codex: 9, gemini: 4, aider: 2 };
const RESOURCE_MB: Record<string, number> = { claude: 412, codex: 286, gemini: 238, aider: 164 };

function spinnerOf(id: number): ((s: number, f: number) => string) | null {
  const info = sessions.get(id);
  if (info?.profileId === "claude") return claudeSpinner;
  if (info?.profileId === "codex") return codexSpinner;
  return null;
}

function createSession(args: Args): unknown {
  const profileId = typeof args.profileId === "string" ? args.profileId : "";
  const profile = profiles.find((p) => p.id === profileId);
  const cwd = typeof args.cwd === "string" ? args.cwd : ROOT;
  const id = sessionId(profileId, cwd, typeof args.subAgent === "string" ? args.subAgent : null);
  const info = {
    id,
    profileId,
    title: profile?.name ?? profileId,
    cwd,
    status: "running",
    exitCode: null,
    createdAt: ago((AGE_MINUTES[id - 1] ?? 5) * MINUTE),
    resumed: args.resume === true,
    cliSessionId: typeof args.cliSessionId === "string" ? args.cliSessionId : null,
    // Only the first Claude Code session has sub-agents to show.
    subAgents: profileId === "claude",
    gate: profileId === "claude" ? "active" : "none",
    mode: args.mode === "team" ? "team" : "private",
    // The sub-agent definition the session runs as, kept by the backend across restarts.
    subAgent: typeof args.subAgent === "string" ? args.subAgent : null,
  };
  const channel = channelId(args.onEvent);
  if (channel !== undefined) channelOfSession.set(id, channel);
  sessions.set(id, info);
  setTimeout(() => {
    output(id, mainScripts[id] ?? "");
    const spinner = spinnerOf(id);
    if (WORKING.has(id) && spinner !== null) {
      const start = id === 2 ? CODEX_START_SECONDS : id === 1 ? CLAUDE_START_SECONDS : 23 + id;
      keepTicking(id, start, (s, f) => `${RESTORE_CURSOR}${E}2K${spinner(s, f)}`);
    }
    // Works long enough for its quiet end to count as done, answers, then waits.
    if (FINISHING.has(id)) {
      keepTicking(id, 0, (s, f) => `${RESTORE_CURSOR}${E}2K${geminiSpinner(s, f)}`, GEMINI_WORK_MS);
      setTimeout(() => {
        output(id, `${RESTORE_CURSOR}${E}2K${id === 4 ? (geminiOut[1] ?? "") : "Done."}`);
        if (id === 4) control.geminiAnswered = true;
      }, GEMINI_WORK_MS + 300);
    }
    // A bell makes the session show up as "asks you something" in the sidebar.
    if (ASKING.has(id)) {
      setTimeout(() => output(id, "\x07"), 300);
    }
  }, 600);
  return info;
}

// The sub-agents of the first Claude Code session, named by their definition (the crew column
// gives each its figure), with what they do right now and their tokens.
const subAgent = (
  id: string,
  description: string,
  agentType: string,
  startedAgo: number,
  lastActivityAgo: number,
  tool: string,
  target: string,
  tokens: number,
) => ({
  id: `agent-${id}`,
  description,
  agentType,
  spawnDepth: 1,
  status: "running",
  startedAt: ago(startedAgo),
  lastActivity: ago(lastActivityAgo),
  tool,
  target,
  note: null,
  tokens,
  excerpt: null,
});
const subAgents = [
  subAgent(
    "backend",
    "Retries with backoff",
    "hangar-backend",
    MINUTE + 38_000,
    2_000,
    "Edit",
    "src/payments/webhook.ts",
    52_310,
  ),
  subAgent(
    "frontend",
    "Retry banner",
    "hangar-frontend",
    54_000,
    1_500,
    "Edit",
    "src/ui/RetryBanner.tsx",
    38_420,
  ),
  subAgent(
    "tester",
    "Write retry tests",
    "hangar-tester",
    MINUTE + 12_000,
    3_000,
    "Bash",
    "pnpm test payments",
    61_870,
  ),
];

/** The watch's channel per session, and the sub-agent whose log it streams. */
const subAgentWatch = new Map<number, number>();

/** No scene opens a sub-agent's log: an empty one for whichever is asked. */
function sendSubAgentLog(channel: number, focus: unknown): void {
  if (typeof focus !== "string") return;
  send(channel, { type: "log", agentId: focus, reset: true, entries: [], toolUses: 0 });
}

function watchSubAgents(args: Args): null {
  const sessionId = typeof args.sessionId === "number" ? args.sessionId : -1;
  const channel = channelId(args.onEvent);
  if (channel === undefined) return null;
  subAgentWatch.set(sessionId, channel);
  setTimeout(() => {
    const agents = sessionId === 1 ? subAgents : [];
    send(channel, { type: "snapshot", located: true, agents });
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
const ACCOUNT_BASE = "https://account.example.com";
const accountView = {
  baseUrl: ACCOUNT_BASE,
  official: true,
  devicesUrl: `${ACCOUNT_BASE}/devices`,
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
const teamTask = (
  pair: string,
  title: string,
  area: string,
  member: string,
  name: string,
  sinceMinutes: number,
) => ({
  task: hash(pair),
  title,
  area,
  member,
  device: `dev-${name.split(" ")[0]?.toLowerCase() ?? "x"}-1`,
  name,
  mine: member === "user-sam",
  own: false,
  since: ago(sinceMinutes * MINUTE),
  expiresAt: null,
  expiring: false,
  request: null,
});
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
                repo: ACME_ROOT_COMMIT,
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
                repo: ACME_ROOT_COMMIT,
              },
            ],
          },
        ],
        overlaps: [],
        divergent: [],
        tasks: [
          teamTask("a1", "Webhook retries", "src/payments", "user-sam", "Sam Carter", 24),
          teamTask("b2", "Checkout emails", "src/emails", "7c41d9e2b08a4f15", "Mika Rossi", 41),
          teamTask("c3", "Search ranking", "src/search", "b2e86f0a13c947d5", "Jonas Berg", 78),
        ],
        attempts: [],
        notices: [],
        // Only the acme-shop sessions are shared with the team.
        sessions: [1, 2, 3],
        sessionRepos: [1, 2, 3].map((sessionId) => ({
          sessionId,
          repo: ACME_ROOT_COMMIT,
        })),
      },
    },
  ],
  pending: [],
  repos: [{ root: ACME_ROOT_COMMIT, folder: `${ROOT}\\.git`, name: "acme-shop" }],
};

// What this device's sessions changed: the Claude Code session in acme-shop and Codex CLI in its
// worktree, with no collision between them.
const claimsSession = (sessionId: number, root: string, claims: [string, string][]) => ({
  sessionId,
  state: { kind: "ready" },
  root: root.replace(/\\/g, "/"),
  base: "4e1f0c2",
  sharedWith: [],
  claims: claims.map(([path, kind]) => ({
    path,
    kind,
    wholeFile: kind !== "modified",
    ranges: kind === "modified" ? [{ start: 38, end: 44 }] : [],
    area: path.split("/").slice(0, 2).join("/"),
  })),
  truncated: false,
  level: "none",
  overlapCount: 0,
  overlapsTruncated: false,
});
const claimsSnapshot = {
  enabled: true,
  sessions: [
    claimsSession(1, ROOT, [
      ["src/payments/webhook.ts", "modified"],
      ["src/payments/webhook.test.ts", "modified"],
      ["src/payments/retry.ts", "untracked"],
    ]),
    claimsSession(2, WORKTREE, [
      ["src/cart/total.ts", "modified"],
      ["src/cart/total.test.ts", "modified"],
    ]),
  ],
  overlaps: [],
};

// How full each agent's context window is (the meter in its sidebar row); the backend reads it
// from the agents' transcripts.
const contextUsage = {
  sessions: [
    { sessionId: 1, used: 123_000, window: 1_000_000 },
    { sessionId: 3, used: 41_000, window: 200_000 },
    { sessionId: 6, used: 88_000, window: 200_000 },
    { sessionId: 9, used: 152_000, window: 200_000 },
  ],
};

// Git: where each folder's repository is and how many files are changed per checkout.
const repoOf = (cwd: string) => ([DOCS, BILLING, WEB].includes(cwd) ? cwd : ROOT);
/** Changed files per checkout, as `git status` would count them. */
const changedFiles: Record<string, number> = {
  [ROOT]: 4,
  [WORKTREE]: 2,
  [DOCS]: 1,
  [BILLING]: 3,
  [WEB]: 2,
};

function sessionCwd(args: Args): string {
  const info = typeof args.id === "number" ? sessions.get(args.id) : undefined;
  return typeof info?.cwd === "string" ? info.cwd : ROOT;
}

// Sub-agent definitions: the fifteen Hangar ships, installed as Claude Code's own and unchanged,
// read from the files the app bundles (src-tauri/bundled-agents) so the scenes list them as they
// ship. Each wears the figure its role points to (subAgentLibrary.ts `defaultFigure`).
const BUNDLED_FILES = import.meta.glob<string>("../../src-tauri/bundled-agents/*.md", {
  query: "?raw",
  import: "default",
  eager: true,
});
/** The order the sub-agents scene lists them in: the developer roles first. */
const BUNDLED_ROLES = [
  "backend",
  "frontend",
  "tester",
  "security",
  "developer",
  "manager",
  "researcher",
  "sysadmin",
  "marketing",
  "sales",
  "social-media",
  "finance",
  "contracts",
  "n8n",
  "comfyui",
];
/** The `key: value` lines of a definition's front matter. */
function frontMatter(text: string): Map<string, string> {
  const head = /^---\r?\n([\s\S]*?)\r?\n---/.exec(text)?.[1] ?? "";
  const fields = new Map<string, string>();
  for (const line of head.split(/\r?\n/)) {
    const match = /^([a-z]+):\s*(.*)$/.exec(line);
    if (match?.[1] !== undefined && match[2] !== undefined) fields.set(match[1], match[2].trim());
  }
  return fields;
}
function bundledDefinition(role: string) {
  const file = `hangar-${role}.md`;
  const text = BUNDLED_FILES[`../../src-tauri/bundled-agents/${file}`];
  if (text === undefined) throw new Error(`readme-shots: no bundled sub-agent ${file}`);
  const fields = frontMatter(text);
  const tools = fields.get("tools");
  return {
    name: fields.get("name") ?? `hangar-${role}`,
    description: fields.get("description") ?? "",
    model: fields.get("model") ?? null,
    color: fields.get("color") ?? null,
    // No list: every tool, as Claude Code reads a definition without one.
    tools: tools === undefined ? [] : tools.split(",").map((tool) => tool.trim()),
    extras: [],
    scope: "global",
    folder: null,
    file,
  };
}
const bundledDefs = BUNDLED_ROLES.map(bundledDefinition);
const subAgentLibrary = {
  defs: bundledDefs,
  codexDefs: [],
  problems: [],
  installed: ["claude", "codex"],
  targets: [],
  codexModels: [],
  syncIssues: [],
  bundled: bundledDefs.map((def) => def.name),
};
/** No figure picked by hand: each agent wears the one its role points to. */
const subAgentFigures = {};

// The day so far in tokens and estimated dollars (cost_service.rs): Claude Code's two sessions
// and Codex CLI. The costs scene has a daily budget of $20, so the early warning at 80 % shows.
const COST_DAY = "2026-11-10";
const tokens = (input: number, output: number, cacheRead: number) => ({
  input,
  output,
  cacheWrite5m: Math.round(input / 4),
  cacheWrite1h: 0,
  cacheRead,
});
const costAmount = (input: number, output: number, cacheRead: number, costUsd: number) => {
  const counts = tokens(input, output, cacheRead);
  return {
    tokens: counts,
    total: counts.input + counts.output + counts.cacheWrite5m + counts.cacheRead,
    costUsd,
    partial: false,
  };
};
const sessionCosts = [
  // Only the sessions the scene has: the day in the five-session scenes is 82 % of the budget.
  { sessionId: 1, ...costAmount(48_200, 21_400, 1_240_000, 9.06) },
  { sessionId: 2, ...costAmount(61_300, 18_900, 820_000, 6.12) },
  { sessionId: 3, ...costAmount(12_800, 3_100, 210_000, 1.37) },
  { sessionId: 6, ...costAmount(22_100, 7_400, 380_000, 2.86) },
  { sessionId: 9, ...costAmount(30_700, 9_900, 450_000, 3.48) },
].filter((entry) => entry.sessionId <= mainSessions.length);
const costTitles: Record<number, [string, string]> = {
  1: ["Webhook retries", "Claude Code"],
  2: ["Cart rounding", "Codex CLI"],
  3: ["hangar-security", "Claude Code"],
  6: ["Search ranking", "Claude Code"],
  9: ["Mobile layout", "Claude Code"],
};
const sum = (pick: (entry: (typeof sessionCosts)[number]) => number) =>
  sessionCosts.reduce((total, entry) => total + pick(entry), 0);
const costReport = {
  today: COST_DAY,
  sessions: sessionCosts,
  days: [
    {
      day: COST_DAY,
      tokens: {
        input: sum((entry) => entry.tokens.input),
        output: sum((entry) => entry.tokens.output),
        cacheWrite5m: sum((entry) => entry.tokens.cacheWrite5m),
        cacheWrite1h: 0,
        cacheRead: sum((entry) => entry.tokens.cacheRead),
      },
      total: sum((entry) => entry.total),
      costUsd: Math.round(sum((entry) => entry.costUsd) * 100) / 100,
      partial: false,
      sessions: sessionCosts.map((entry) => ({
        ...entry,
        title: costTitles[entry.sessionId]?.[0] ?? "",
        program: costTitles[entry.sessionId]?.[1] ?? "",
      })),
    },
  ],
  priceSource: "Published list prices",
  priceDate: "2026-11-01",
};

// The preferences the backend keeps. The costs scene has a daily budget, so the early warning
// shows; yesterday's summary was offered already, so it stays out of the way.
const mainPrefs = {
  startMode: "private",
  lastProgram: null,
  favorites: [],
  recent: [],
  restoreSessions: true,
  attentionEnabled: true,
  taskbarFlash: true,
  notifyDesktop: true,
  chime: true,
  quietHours: { enabled: false, from: 22 * 60, to: 7 * 60 },
  dailyBudget: SCENE === "costs" ? 20 : null,
  budgetEarlyPercent: 80,
  budgetWarned: null,
  morningReview: true,
  morningOfferedDay: "2026-11-09",
  dictationEnabled: false,
};

// Link groups (ADR 0014): the lead in acme-shop hands out tasks to Codex CLI in its worktree and
// to Gemini CLI in the docs project, and collects their results.
/** The waiting-message scene: the group asks first, so the members' reports wait for a click. */
const ASK_FIRST = SCENE === "waiting-message";
const linkGroup = {
  id: 1,
  color: 1,
  askFirst: ASK_FIRST,
  braked: false,
  members: [
    { session: 1, name: "shop", role: "lead" },
    { session: 2, name: "cart", role: "member" },
    { session: 4, name: "docs", role: "member" },
  ],
};
const linkMessage = (id: number, fromName: string, minutesAgo: number, text: string) => ({
  id,
  group: linkGroup.id,
  fromName,
  to: 1,
  text,
  at: ago(minutesAgo * MINUTE),
  auto: false,
  claimed: false,
});
/**
 * The reports that wait below the lead's row, oldest first: the driver sends the second, which
 * then waits for the lead to pause, and leaves the first with its buttons.
 */
const heldMessages = [
  linkMessage(
    1,
    "cart",
    4,
    [
      "Found it: each cart line was rounded before summing, so three lines at 33.33 % off " +
        "lost a cent. src/cart/total.ts now rounds once at the end.",
      "",
      "- Added a test with three discounted lines",
      "- pnpm test cart: 24 passed",
      "- Branch hangar/cart-rounding is ready for review",
    ].join("\n"),
  ),
  linkMessage(
    2,
    "docs",
    1,
    "Refund page done: docs/api/refunds.md covers create, list and cancel, each with a curl " +
      "example.",
  ),
];
const linkState = {
  groups: [linkGroup],
  sendable: [...mainSessions.keys()].map((index) => index + 1).filter((id) => id !== 5),
  unattended: [],
  messages: ASK_FIRST ? heldMessages : [],
};

// Quick access: the notes whose text matches the query, as the backend's search finds them.
const noteHits = [
  {
    folderKey: "acme-shop",
    path: ROOT,
    id: "note-refunds",
    title: "Refund flow",
    excerpt: "Refunds above 500 EUR need a second approval before the payment provider is called.",
    updatedAt: ago(2 * HOUR),
  },
  {
    folderKey: "acme-docs",
    path: DOCS,
    id: "note-api-style",
    title: "API docs style",
    excerpt: "Every refund endpoint gets a curl example and its error codes in a table.",
    updatedAt: ago(26 * HOUR),
  },
  {
    folderKey: "general",
    path: null,
    id: "note-release",
    title: "Release checklist",
    excerpt: "Check the refund e-mails in all languages before the release.",
    updatedAt: ago(3 * 24 * HOUR),
  },
];

// The secret guard: three files are staged in acme-shop; two of them seem to hold a key. The
// mock names only the path and the kind, as the backend does; no key appears anywhere.
const STAGED_DIFF = [
  "diff --git a/src/payments/webhook.ts b/src/payments/webhook.ts",
  "index 3f2a1c4..8b7e2d1 100644",
  "--- a/src/payments/webhook.ts",
  "+++ b/src/payments/webhook.ts",
  "@@ -35,9 +35,15 @@ export async function deliver(event: WebhookEvent) {",
  "   const url = endpointFor(event);",
  "   log.debug(`delivering ${event.id}`);",
  "-  const res = await send(event);",
  "-  if (!res.ok) throw new DeliveryError(res.status);",
  "-  return res;",
  "+  const delays = [250, 500, 1000];",
  "+  for (const delay of delays) {",
  "+    const res = await send(event);",
  "+    if (res.ok) return res;",
  "+    await sleep(delay);",
  "+  }",
  '+  throw new DeliveryError("gave up after three retries");',
  " }",
  "diff --git a/src/payments/client.ts b/src/payments/client.ts",
  "index 9c4d2e7..1a5b6f3 100644",
  "--- a/src/payments/client.ts",
  "+++ b/src/payments/client.ts",
  '@@ -8,4 +8,5 @@ import { config } from "../config";',
  " export const payments = createClient({",
  "   baseUrl: config.paymentsUrl,",
  "+  timeoutMs: 5000,",
  " });",
  "diff --git a/.env.production b/.env.production",
  "new file mode 100644",
  "index 0000000..4d3c2b1",
  "--- /dev/null",
  "+++ b/.env.production",
  "@@ -0,0 +1,2 @@",
  "+PAYMENTS_URL=https://payments.example.com",
  "+LOG_LEVEL=info",
].join("\n");
const stagedChanges = {
  isRepo: true,
  files: [
    { path: "src/payments/webhook.ts", status: "M ", additions: 7, deletions: 3 },
    { path: "src/payments/client.ts", status: "M ", additions: 1, deletions: 0 },
    { path: ".env.production", status: "A ", additions: 2, deletions: 0 },
  ],
  diff: STAGED_DIFF,
  truncated: false,
};
const secretOutcome = {
  kind: "secrets",
  findings: [
    { path: "src/payments/client.ts", kinds: ["stripeKey"] },
    { path: ".env.production", kinds: ["secretFile"] },
  ],
  more: 0,
  complete: true,
  tree: "5d41402abc4b2a76b9719d911017c592ae1f7e2b",
};

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
  last_exit_was_clean: () => true,
  // Answered already, so the usage statistics question never covers a scene.
  telemetry_status: () => false,
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
  git_diff: (args) =>
    SCENE === "secret-guard" && sessionCwd(args) === ROOT
      ? stagedChanges
      : { isRepo: true, files: [], diff: "", truncated: false },
  // "Commit" in the changes panel: the secret guard holds it and names what it found.
  git_commit: () => secretOutcome,
  // Quick access asks for the active folder's files and commits; no scene shows them.
  list_session_paths: () => null,
  git_log: () => null,
  // No scene opens the file tree.
  list_session_folder: (args) => ({
    root: sessionCwd(args),
    repoRoot: repoOf(sessionCwd(args)),
    entries: [],
    truncated: false,
  }),
  context_usage: () => contextUsage,
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
  list_note_folders: () => [],
  list_notes: () => [],
  read_note: () => null,
  write_note: () => null,
  search_notes: () => noteHits,
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
  configure_claims: () => null,
  claims_snapshot: () => claimsSnapshot,
  // Steady load numbers so every session row shows CPU, RAM and, for Codex, the GPU.
  session_resources: () =>
    [...sessions.entries()].map(([id, info]) => {
      const program = typeof info.profileId === "string" ? info.profileId : "";
      return {
        id,
        cpu: RESOURCE_CPU[program] ?? 3,
        memory: (RESOURCE_MB[program] ?? 180) * 1024 * 1024,
        gpu: program === "codex" ? 14 : null,
        gpuMemory: program === "codex" ? 820 * 1024 * 1024 : null,
        outlier: false,
      };
    }),
  system_usage: () => ({
    cpu: 23,
    memory: 58,
    gpu: 17,
    memoryUsed: 18.6 * 1024 ** 3,
    memoryTotal: 32 * 1024 ** 3,
    gpuMemory: 2.4 * 1024 ** 3,
  }),
  team_snapshot: () => teamSnapshot,
  power_state: () => "ac",
  // Settings kept by the backend.
  security_switches: () => null,
  save_security_switches: () => null,
  protected_settings: () => null,
  save_protected_settings: () => null,
  main_prefs: () => mainPrefs,
  save_main_prefs: () => null,
  load_templates: () => null,
  save_templates: () => null,
  load_message_queue: () => ({}),
  save_message_queue: () => null,
  log_work_phase: () => null,
  git_repo_places: (args) => {
    const paths = Array.isArray(args.paths) ? args.paths : [];
    return Object.fromEntries(
      paths.map((p: unknown) => {
        const path = typeof p === "string" ? p : "";
        const root = repoOf(path);
        return [path, { root, workTree: path === WORKTREE ? WORKTREE : root }];
      }),
    );
  },
  git_changed_count: (args) => changedFiles[sessionCwd(args)] ?? 0,
  profile_path: () => null,
  configure_gate: () => null,
  configure_command_guard: () => null,
  command_guard_info: () => ({
    enabled: true,
    file: "",
    rules: [],
    problems: [],
    defaults: true,
    defaultCount: 0,
  }),
  gate_rules: () => [],
  gate_snapshot: () => ({ enabled: true, pending: [], log: [] }),
  permission_waiting: () => [],
  // Sub-agent definitions and their figures.
  sub_agent_library: () => subAgentLibrary,
  sub_agent_figures: () => subAgentFigures,
  sub_agent_settings: () => ({ models: {}, provideOff: [] }),
  sync_sub_agents: () => null,
  // Every session binds its work tree snapshots; no scene shows them.
  snapshot_bind: () => null,
  snapshot_take: () => null,
  cost_report: () => costReport,
  // No scene shows the morning summary.
  daily_review: () => null,
  // Partner projects: none in these scenes.
  partner_projects: () => ({ projects: [], pairs: [] }),
  waiting_messages: () => [],
  // Link groups and their waiting messages; no scene opens the group's history.
  link_state: () => linkState,
  link_history: () => [],
  // Dictation stays off; the window still asks for its state and stops it on a blur.
  dictation_stop: () => null,
  dictation_take_text: () => [],
  dictation_status: () => ({
    state: { kind: "ready" },
    models: [{ id: "base", bytes: 148_000_000, installed: true }],
    source: "huggingface.co",
    microphone: true,
  }),
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

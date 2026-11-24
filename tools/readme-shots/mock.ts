// In-memory stand-in for the Tauri backend, used only for the README screenshots. It answers the
// IPC calls the frontend makes and plays a scripted, fictional day in the project "acme-shop":
// Claude Code working with a crew of Hangar's own sub-agents next to Codex CLI in its own worktree,
// a second Claude Code session started as the "hangar-security" sub-agent, Gemini CLI done with the
// API docs in the project "acme-docs", a PowerShell running the docs site's dev server and a
// signed-in account with two teammates online. Three of the agents form a link group with a lead.
// Per scene it adds what that scene shows: the sessions opened recently, the team's shared notes,
// a crew of reviewers with two visitors that are not Hangar's own, the home view's glyph field,
// messages of the group that wait for a click or a commit held by the secret guard.
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
/** The scene whose sidebar rows send output fast enough for the flow to run busy. */
const FAST_OUTPUT = SCENE === "overview";
/** The sub-agents scene: the lead reviews with a different crew. */
const REVIEW = SCENE === "sub-agents";
/** The team notes scene: an admin shares the project's notes with the team. */
const SHARE_WITH_TEAM = SCENE === "team-notes";
/** The home view scene: its glyph field holds still for the screenshot. */
const HOME_GLYPHS = SCENE === "home-glyphs";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

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
// The home view's glyph field (GlyphBackground.tsx) draws from Math.random and fades by
// performance.now. In its scene both are pinned: a seeded random, so the field and its pulses come
// out alike in each run, and a clock that stops with the driver's freeze, so the patch the pointer
// just uncovered and the blot pulsing up stay as they are until the screenshot is taken.
if (HOME_GLYPHS) {
  let seed = 0x2f6e2b1;
  Math.random = () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let value = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    value = (value + Math.imul(value ^ (value >>> 7), 61 | value)) ^ value;
    return ((value ^ (value >>> 14)) >>> 0) / 4294967296;
  };
  const realPerformanceNow = performance.now.bind(performance);
  let frozenAt: number | null = null;
  performance.now = () => {
    if (!control.freeze) return realPerformanceNow();
    frozenAt ??= realPerformanceNow();
    return frozenAt;
  };
}

/** A moment `ms` before the screenshot. */
const ago = (ms: number) => SHOT_TIME - ms;

const ROOT = "C:\\src\\acme-shop";
const WORKTREE = "C:\\src\\acme-shop-cart-rounding";
const WORKTREE_BRANCH = "hangar/cart-rounding";
const DOCS = "C:\\src\\acme-docs";
const BILLING = "C:\\src\\acme-billing";
const WEB = "C:\\src\\acme-web";
const MOBILE = "C:\\src\\acme-mobile";
const INFRA = "C:\\src\\acme-infra";

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
/**
 * The sessions of the scene. A list keeps the order of the base list, so each session keeps its
 * number (and its script): the overview and the sub-agents scene leave out the docs server, so
 * their sidebars, with the lead's crew in a tree, show every row whole.
 */
const mainSessions = SCENE === "overview" || REVIEW ? baseSessions.slice(0, 4) : baseSessions;
const workspace = { version: 1, activeKey: uuid(1), sessions: mainSessions };
/** Minutes each session has been running, in creation order. */
const AGE_MINUTES = [52, 47, 38, 31, 64];

// "Recently opened" (recentlyOpened.ts): programs started in a folder before, none of them open
// now, newest first. The overview shows two (its sidebar holds the link group's bracket and the
// lead's crew above them), the others the first three (folded away in most scenes). Times count
// back from the clock's start, so the ages read the same however long the scene takes to settle.
const recent = (
  profileId: string,
  cwd: string,
  title: string,
  usedAgo: number,
  conversation: number | null,
) => ({
  profileId,
  cwd,
  title,
  color: null,
  emoji: null,
  cliSessionId:
    conversation === null
      ? null
      : `dddddddd-0000-4000-8000-${String(conversation).padStart(12, "0")}`,
  closedAt: FIXED_START - usedAgo,
});
const recentlyOpened = [
  recent("claude", BILLING, "Invoice PDFs", 5 * MINUTE, 1),
  recent("codex", WEB, "Flaky e2e tests", 2 * HOUR, 2),
  recent("gemini", ROOT, "Search ranking", DAY + 3 * HOUR, 3),
  recent("pwsh", INFRA, "acme-infra", 3 * DAY, null),
  recent("claude", MOBILE, "Push notifications", 6 * DAY, 4),
].slice(0, SCENE === "overview" ? 2 : 3);

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

/**
 * The sub-agents scene: the lead has the retries reviewed before the merge, by two of Hangar's
 * agents and two visitors, the team's own code-reviewer and load-tester. The manager finished a
 * while ago (the sidebar keeps it, the stage no longer), the security agent searches, the
 * code-reviewer reads and the load-tester has just started.
 */
const claudeReviewOut = lines([
  dim("> ") + "Review the webhook retries before we merge",
  "",
  "● I'll have the diff reviewed, the payment",
  "  code audited, the style checked and the",
  "  retries tried under load.",
  "",
  task("hangar-manager", "Review the diff"),
  dim("  ⎿  ") + "Done (12 tool uses · 18.4k tokens · 48s)",
  "",
  task("hangar-security", "Audit retry handling"),
  dim("  ⎿  ") + "Grep(apiKey|secret, src/payments)",
  dim("     Found 3 matches in 2 files"),
  "",
  task("code-reviewer", "Check naming and style"),
  dim("  ⎿  ") + "Read(src/payments/webhook.ts)",
  dim("     Read 86 lines"),
  "",
  task("load-tester", "Retries under load"),
  dim("  ⎿  ") + dim("Starting…"),
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

/** The first screen of each session, by session number. */
const mainScripts: Record<number, string> = {
  1: CLEAR + HIDE_CURSOR + (REVIEW ? claudeReviewOut : claudeCrewOut),
  2: CLEAR + codexOut + codexSpinner(CODEX_START_SECONDS, 0) + codexFooter,
  3: CLEAR + HIDE_CURSOR + claudeAskOut,
  4: CLEAR + HIDE_CURSOR + (geminiOut[0] ?? ""),
  5: CLEAR + pwshOut(DOCS, "acme-docs", 5173),
};

/** Sessions that keep working with a spinner: Claude Code and Codex CLI. */
const WORKING = new Set([1, 2]);
/** Sessions that ring the bell, so the sidebar shows them asking. */
const ASKING = new Set([3]);
/** Sessions that work a little, then answer and rest: the sidebar shows them done. */
const FINISHING = new Set([4]);

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
// gives each its figure), with what they do right now and their tokens. One without a tool call
// yet has just started: its speech bubble types dots. A finished one stands on a green ring for a
// few seconds (crew.ts `DONE_LINGER_MS`), which the frozen clock keeps.
const subAgent = (
  id: string,
  description: string,
  agentType: string,
  startedAgo: number,
  lastActivityAgo: number,
  tool: string | null,
  target: string | null,
  tokens: number,
  status: "running" | "finished" = "running",
) => ({
  id: `agent-${id}`,
  description,
  agentType,
  spawnDepth: 1,
  status,
  startedAt: ago(startedAgo),
  lastActivity: ago(lastActivityAgo),
  tool,
  target,
  note: null,
  tokens,
  excerpt: null,
});
const buildCrew = [
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
const reviewCrew = [
  subAgent(
    "manager",
    "Review the diff",
    "hangar-manager",
    2 * MINUTE,
    50_000,
    "Read",
    "src/payments/webhook.ts",
    18_420,
    "finished",
  ),
  subAgent(
    "security",
    "Audit retry handling",
    "hangar-security",
    52_000,
    1_000,
    "Grep",
    "apiKey|secret",
    27_310,
  ),
  // Two visitors, the person's own definitions: each wears the drawn guest figure in its color,
  // its initial on the badge.
  subAgent(
    "reviewer",
    "Check naming and style",
    "code-reviewer",
    34_000,
    1_500,
    "Read",
    "src/payments/webhook.ts",
    11_640,
  ),
  subAgent("load", "Retries under load", "load-tester", 6_000, 6_000, null, null, 2_140),
];
const subAgents = REVIEW ? reviewCrew : buildCrew;

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

// The account and the team: Sam, signed in on Free, works in acme-shop, a Team project Mika owns,
// with Mika and Jonas. Free keeps the support badge in the bar above the work area
// (teamPlan.ts `showsSupportLink`); the team project keeps the team functions.
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
      seatsAccountTotal: 1,
      seatsAccountUsed: 1,
    },
  },
  otherSignIns: [],
};
const projects = [
  {
    id: PROJECT,
    name: "acme-shop",
    role: "member",
    plan: "Team",
    seatsAccountTotal: 5,
    seatsAccountUsed: 3,
    pendingInvitations: 0,
    canInvite: false,
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
  ],
};

// Git: where each folder's repository is and how many files are changed per checkout.
const repoOf = (cwd: string) => (cwd === DOCS ? cwd : ROOT);
/** Changed files per checkout, as `git status` would count them. */
const changedFiles: Record<string, number> = {
  [ROOT]: 4,
  [WORKTREE]: 2,
  [DOCS]: 1,
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
/** The person's own definitions next to Hangar's: visitors, drawn in their frontmatter color. */
const ownDefinition = (name: string, description: string, color: string, tools: string[]) => ({
  name,
  description,
  model: null,
  color,
  tools,
  extras: [],
  scope: "global",
  folder: null,
  file: `${name}.md`,
});
const ownDefs = [
  ownDefinition(
    "code-reviewer",
    "Reviews a diff for naming, structure and readability before a merge.",
    "green",
    ["Read", "Grep", "Glob"],
  ),
  ownDefinition(
    "load-tester",
    "Runs load tests against a local build and reports latency and errors.",
    "orange",
    ["Read", "Bash"],
  ),
];
const subAgentLibrary = {
  defs: [...bundledDefs, ...ownDefs],
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
// and Codex CLI.
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
  // Only the sessions the scene has.
  { sessionId: 1, ...costAmount(48_200, 21_400, 1_240_000, 9.06) },
  { sessionId: 2, ...costAmount(61_300, 18_900, 820_000, 6.12) },
  { sessionId: 3, ...costAmount(12_800, 3_100, 210_000, 1.37) },
].filter((entry) => entry.sessionId <= mainSessions.length);
const costTitles: Record<number, [string, string]> = {
  1: ["Webhook retries", "Claude Code"],
  2: ["Cart rounding", "Codex CLI"],
  3: ["hangar-security", "Claude Code"],
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

// The preferences the backend keeps. Yesterday's summary was offered already, so it stays out of
// the way.
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
  dailyBudget: null,
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
const linkMessage = (id: number, fromName: string | null, minutesAgo: number, text: string) => ({
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
 * Hangar's introduction of the group to its lead, as links.rs `introduction` writes it for the
 * agent; its card shows people whom the session works with instead (linkIntro.ts).
 */
const LEAD_INTRO = [
  'You are linked with other agents in Hangar. They work in parallel in their own sessions. Your member name is "shop". The other members:',
  '- "cart": Codex CLI in acme-shop-cart-rounding',
  '- "docs": Gemini CLI in acme-docs',
  "You lead this group: split the work into tasks, hand them to the members with `hangar-send <name>`, collect their results and keep the overall picture. " +
    'To write to one of them, run `hangar-send <name> "<message>"`. No answer is needed to this introduction.',
].join("\n");
/**
 * What waits below the lead's row, oldest first: Hangar's introduction of the group, which the
 * driver sends (it then waits for the busy lead to pause), and a member's report with its buttons.
 */
const heldMessages = [
  linkMessage(1, null, 6, LEAD_INTRO),
  linkMessage(
    2,
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
];
const linkState = {
  groups: [linkGroup],
  sendable: [...mainSessions.keys()].map((index) => index + 1).filter((id) => id !== 5),
  unattended: [],
  messages: ASK_FIRST ? heldMessages : [],
};

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

// Notes: three of the person's own in acme-shop, one in General and four shared ones of acme-shop,
// encrypted in the repository. The overview shows the private ones in the right panel; the team
// notes scene the shared ones, with their excerpts and checklist progress, behind the dialog that
// shares them with the whole team (`shared_notes_team`).
const NOTE_TEXT: Record<string, string> = {
  "note-retries": [
    "# Webhook retries",
    "",
    "Failed deliveries retry with backoff, then give up and show the support team a banner.",
    "",
    "- [x] Back off 250, 500 and 1000 ms",
    "- [x] Give up after three tries",
    "- [x] Log each failed delivery",
    "- [ ] Banner when a delivery failed",
    "- [ ] Tests for timeouts",
  ].join("\n"),
  "note-refunds": [
    "# Refund flow",
    "",
    "Refunds above 500 EUR need a second approval before the payment provider is called.",
  ].join("\n"),
  "note-rounding": [
    "# Cart rounding",
    "",
    "Round once at the end, not per line. Check the tax rounding with the accountant.",
  ].join("\n"),
  "note-oncall": [
    "# On-call handover",
    "",
    "Mika has payments until Friday, Jonas takes the weekend. Failed webhooks above 2 % page the on-call.",
  ].join("\n"),
  "note-release": [
    "# Release 2.4 checklist",
    "",
    "Freeze on Thursday, ship on Monday morning.",
    "",
    "- [x] Changelog",
    "- [x] Webhook retries merged",
    "- [ ] Check the refund e-mails in all languages",
    "- [ ] Tag the release",
  ].join("\n"),
  "note-review": [
    "# Review rules",
    "",
    "Two approvals for anything under src/payments. Small pull requests, one topic each.",
  ].join("\n"),
  "note-sandbox": [
    "# Payment sandbox",
    "",
    "The sandbox resets every Monday. Test cards live in the provider's dashboard, never in the repo.",
  ].join("\n"),
  "note-ideas": [
    "# Ideas",
    "",
    "A status page for webhook deliveries, so support sees failures first.",
  ].join("\n"),
};
const noteInfo = (id: string, title: string, updatedAgo: number, order: number) => ({
  id,
  title,
  updatedAt: ago(updatedAgo),
  order,
});
/** The `path` of a notes call's `folder` argument; `null` for General. */
function folderPath(args: Args): unknown {
  const folder = typeof args.folder === "object" && args.folder !== null ? args.folder : {};
  return "path" in folder ? folder.path : null;
}
function listNotes(args: Args): unknown[] {
  const folder = typeof args.folder === "object" && args.folder !== null ? args.folder : {};
  const path = "path" in folder ? folder.path : null;
  const shared = "shared" in folder && folder.shared === true;
  if (shared && path === ROOT) {
    return [
      noteInfo("note-oncall", "On-call handover", 18 * MINUTE, 0),
      noteInfo("note-release", "Release 2.4 checklist", 2 * HOUR, 1),
      noteInfo("note-review", "Review rules", DAY + 4 * HOUR, 2),
      noteInfo("note-sandbox", "Payment sandbox", 3 * DAY, 3),
    ];
  }
  if (shared) return [];
  if (path === ROOT) {
    return [
      noteInfo("note-retries", "Webhook retries", 6 * MINUTE, 0),
      noteInfo("note-refunds", "Refund flow", 2 * HOUR, 1),
      noteInfo("note-rounding", "Cart rounding", 5 * HOUR, 2),
    ];
  }
  if (path === null) return [noteInfo("note-ideas", "Ideas", 2 * DAY, 0)];
  return [];
}

// The activity calendar on the home view: Sam's own commits of the last year, as author times in
// seconds. Weekdays are busier than weekends, the last months busier than the first, and a quiet
// stretch in August is a holiday. A fixed formula, so the calendar is the same in every run.
const gitActivity = (() => {
  const commits: number[] = [];
  const today = Math.floor(FIXED_START / DAY) * DAY;
  for (let back = 0; back < 365; back++) {
    const day = today - back * DAY;
    const weekday = new Date(day).getUTCDay();
    const holiday = back > 95 && back < 112;
    const weekend = weekday === 0 || weekday === 6;
    const wave = (Math.sin(back * 1.7) + Math.sin(back * 0.43 + 1)) / 2;
    const busy = (1 - back / 520) * (weekend ? 0.25 : 1) * (0.55 + 0.45 * wave);
    const count = holiday ? 0 : Math.max(0, Math.round(busy * 9 - 1));
    for (let n = 0; n < count; n++)
      commits.push(Math.floor((day + 9 * HOUR + n * 37 * MINUTE) / 1000));
  }
  return { commits };
})();

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
  list_notes: listNotes,
  read_note: (args) => (typeof args.id === "string" ? (NOTE_TEXT[args.id] ?? null) : null),
  write_note: () => null,
  search_notes: () => [],
  list_trash: () => [],
  // Shared notes in acme-shop, open on this computer; the docs project has none. In the team
  // notes scene Sam, an admin of the team, still has them on a code of their own: the panel offers
  // to share them with the team, after which the key arrives through the team.
  shared_notes_status: (args) =>
    folderPath(args) === ROOT
      ? {
          state: "unlocked",
          uncommitted: 0,
          ignored: false,
          unreadable: 0,
          resolved: [],
          team: !SHARE_WITH_TEAM,
        }
      : { state: "unavailable" },
  // The team of acme-shop, which links this clone and whose server holds team keys.
  shared_notes_team: (args) =>
    folderPath(args) === ROOT
      ? { project: PROJECT, manager: SHARE_WITH_TEAM, speaks: true, linked: true, exists: true }
      : null,
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
  // "Recently opened" in the sidebar; reopening is not part of any scene.
  load_recently_closed: () => recentlyOpened,
  save_recently_closed: () => null,
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
  git_activity: () => gitActivity,
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

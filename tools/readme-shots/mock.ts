// In-memory stand-in for the Tauri backend, used only for the README screenshots. It answers the
// IPC calls the frontend makes and plays a scripted, fictional day in the project "acme-shop":
// Claude Code working with a crew of sub-agents next to Codex CLI in its own worktree, a second
// Claude Code session started as the "security" sub-agent, Gemini CLI done with the API docs in
// the partner project "acme-docs", a PowerShell running the docs site's dev server, a signed-in
// account with two teammates online, and, per scene, what that scene shows: a message to the
// partner project, a message waiting from it, the sub-agent definitions and their figures, a
// dictation or the connection test of a self-hosted server.
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
/** Claude Code still works in every scene but the dictation, where it waits at its prompt. */
const CLAUDE_BUSY = SCENE !== "dictation";
/** The messages scene: an agent of acme-shop sent the docs project a message with hangar-send. */
const MESSAGE_WAITING = SCENE === "messages";
/** The partner scenes look at the docs project: its Gemini CLI session is the active one. */
const DOCS_ACTIVE = SCENE === "partners" || SCENE === "messages";
/** The crew scene: the command guard holds a command of the security sub-agent. */
const GATE_PENDING = SCENE === "crew";
/** The server check scene: the account runs on the team's own server, not the official one. */
const SELF_HOSTED = SCENE === "server-check";

// A fixed clock, so every time the app shows (run times, "since 09:12") is the same in each run:
// it starts at FIXED_START and stands at SHOT_TIME once the driver freezes the scene.
const FIXED_START = Date.UTC(2026, 10, 10, 9, 30, 0);
const SHOT_TIME = FIXED_START + 30_000;
const realNow = Date.now.bind(Date);
const clockOffset = FIXED_START - realNow();
Date.now = () => {
  if (control.freeze) return SHOT_TIME;
  return Math.min(realNow() + clockOffset, SHOT_TIME);
};
// `new Date()` without arguments reads the same clock (clock times and "today" in the app come
// from it); every other form of the constructor and every static method stay real.
window.Date = new Proxy(Date, {
  construct(target, args: unknown[], newTarget) {
    return Reflect.construct(target, args.length === 0 ? [Date.now()] : args, newTarget);
  },
});
/** A moment `ms` before the screenshot. */
const ago = (ms: number) => SHOT_TIME - ms;
const MINUTE = 60_000;
const HOUR = 60 * MINUTE;

const ROOT = "C:\\src\\acme-shop";
const WORKTREE = "C:\\src\\acme-shop-cart-rounding";
const WORKTREE_BRANCH = "hangar/cart-rounding";
const DOCS = "C:\\src\\acme-docs";
/**
 * The crew scene: the command guard only watches team sessions, so there the Claude Code session
 * runs as one, in its own worktree.
 */
const TEAM_CLAUDE = GATE_PENDING;
const TEAM_WORKTREE = "C:\\src\\acme-shop-webhook-retries";
const TEAM_BRANCH = "hangar/webhook-retries";

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
const mainSessions = [
  {
    ...plainSession(1, "claude", TEAM_CLAUDE ? TEAM_WORKTREE : ROOT, "Webhook retries"),
    worktree: TEAM_CLAUDE ? { path: TEAM_WORKTREE, branch: TEAM_BRANCH, repoRoot: ROOT } : null,
    cliSessionId: "cccccccc-0000-4000-8000-000000000001",
  },
  {
    // Renamed after its task, as people do with worktree sessions.
    ...plainSession(2, "codex", WORKTREE, "Cart rounding"),
    worktree: { path: WORKTREE, branch: WORKTREE_BRANCH, repoRoot: ROOT },
  },
  {
    // Claude Code started as the "security" sub-agent: its figure is the session's avatar.
    ...plainSession(3, "claude", ROOT, "security"),
    subAgent: "security",
    cliSessionId: "cccccccc-0000-4000-8000-000000000003",
  },
  plainSession(4, "gemini", DOCS, "Refund docs"),
  plainSession(5, "pwsh", DOCS, "Docs server"),
];
const workspace = {
  version: 1,
  activeKey: uuid(DOCS_ACTIVE ? 4 : 1),
  sessions: mainSessions,
};
/** Minutes each session has been running, in creation order. */
const AGE_MINUTES = [52, 47, 38, 31, 64];

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
const claudePrompt = (text: string[] = [], width = 46) => [
  rule(width),
  ...(text.length === 0
    ? [`> ${E}7m ${E}27m`]
    : text.map((line, index) => (index === 0 ? `> ${line}` : `  ${line}`))),
  rule(width),
  dim("  ? for shortcuts"),
];
const task = (agent: string, what: string) => `${green("●")} ${bold(agent)}(${what})`;

/** The overview: two sub-agents at work while Claude Code waits for them. */
const claudeCrewOut = lines([
  dim("> ") + "Add retries with backoff to the payment",
  dim("  ") + "webhook and cover them with tests",
  "",
  "● I'll hand the handler to the backend",
  "  agent and the tests to the tester.",
  "",
  task("backend", "Retries with backoff"),
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
  task("tester", "Write retry tests"),
  dim("  ⎿  ") + "Bash(pnpm test payments)",
  dim("     +9 more tool uses"),
  "",
  SAVE_CURSOR + claudeSpinner(CLAUDE_START_SECONDS, 0),
  "",
  ...claudePrompt(),
]);

/** The crew scene: three agents, the last one held at the command guard. */
const GUARD_CARD_LINES = 11;
const GUARDED_COMMAND = "git clean -fdx";
const claudeGuardOut = lines([
  // Room for the guard's card at the top of the pane, so it covers no output.
  ...Array<string>(GUARD_CARD_LINES).fill(""),
  dim("> ") + "Add retries with backoff to the payment webhook, cover them with tests and",
  dim("  ") + "check the new code for security problems",
  "",
  "● I'll run three agents side by side: backend changes the handler, tester covers",
  "  it with tests and security checks the result.",
  "",
  task("backend", "Retries with backoff"),
  dim("  ⎿  ") + "Update(src/payments/webhook.ts)",
  dim("     Updated with 14 additions and 3 removals"),
  "",
  task("tester", "Write retry tests"),
  dim("  ⎿  ") + "Bash(pnpm test payments)",
  dim("     +9 more tool uses"),
  "",
  `${yellow("●")} ${bold("security")}(Check the new code)`,
  dim("  ⎿  ") + `Bash(${GUARDED_COMMAND})`,
  dim("     Running PreToolUse hook…"),
  "",
  ...claudePrompt([], 82),
]);

/** The dictation scene: a sentence spoken a moment ago stands in the prompt. */
const DICTATED = [
  "Now log every failed delivery attempt together with the order",
  "number and add a test for it.",
];
const claudeDictationOut = lines([
  dim("> ") + "Make the retry delays configurable and add a test for it",
  "",
  "● I'll read the delays from the settings and keep the old ones as the default.",
  "",
  green("●") + " " + bold("Read") + "(src/payments/retry.ts)",
  dim("  ⎿  Read 48 lines"),
  "",
  green("●") + " " + bold("Update") + "(src/payments/retry.ts)",
  dim("  ⎿  Updated with 9 additions and 2 removals"),
  dim("      12 ") + red("-  const DELAYS = [250, 500, 1000];"),
  dim("      12 ") + green("+  const DELAYS = settings.delays ?? [250, 500, 1000];"),
  "",
  green("●") + " " + bold("Update") + "(src/payments/retry.test.ts)",
  dim("  ⎿  Updated with 14 additions"),
  "",
  green("●") + " " + bold("Bash") + "(pnpm test payments)",
  dim("  ⎿  Test Files  2 passed (2)"),
  dim("          Tests  21 passed (21)"),
  "",
  "● Done. The delays now come from the settings and keep their old default. The tests",
  "  cover the default and a custom list.",
  "",
  ...claudePrompt(
    DICTATED.map((line, i) => (i === DICTATED.length - 1 ? `${line}${E}7m ${E}27m` : line)),
    82,
  ),
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
  "• " + bold("Ran") + " pnpm test cart",
  dim("  └ ") + green("✓") + " 23 passed " + dim("(1.4 s)"),
  "",
  "• " + bold("Edited") + " src/cart/total.test.ts " + dim("(") + green("+9") + dim(")"),
  dim("    48 ") + green('+ it("sums discounts", () => {'),
  dim("    49 ") + green("+   const c = lines(3, 9.99);"),
  dim("    50 ") + green("+   expect(total(c)).toBe(19.98);"),
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
const pwshOut = lines([
  "PowerShell 7.6.9",
  "",
  green(`PS ${DOCS}> `) + "pnpm docs:dev",
  "",
  dim("> acme-docs@1.4.0 docs:dev C:\\src\\acme-docs"),
  dim("> vitepress dev"),
  "",
  `  ${green("vitepress")} ${green("v1.6.3")}`,
  "",
  `  ${green("➜")}  ${bold("Local")}:   ${cyan("http://localhost:5173/")}`,
  `  ${green("➜")}  ${dim("Network: use --host to expose")}`,
  "",
]);

const CODEX_START_SECONDS = 41;
function codexSpinner(seconds: number, frame: number): string {
  const glyph = frame % 2 === 0 ? "◦" : "•";
  return `${glyph} ${bold("Working")}${dim(` (${String(seconds)}s • esc to interrupt)`)}`;
}

/** The first screen of each session of the main scenes, by session number. */
const mainScripts: Record<number, string> = {
  1: CLEAR + HIDE_CURSOR + sceneClaudeScreen(),
  2: CLEAR + codexOut + codexSpinner(CODEX_START_SECONDS, 0) + codexFooter,
  3: CLEAR + HIDE_CURSOR + claudeAskOut,
  4: CLEAR + HIDE_CURSOR + (geminiOut[0] ?? ""),
  5: CLEAR + pwshOut,
};

function sceneClaudeScreen(): string {
  if (SCENE === "crew") return claudeGuardOut;
  if (SCENE === "dictation") return claudeDictationOut;
  return claudeCrewOut;
}

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
    if (frame * 700 >= until) {
      window.clearInterval(timer);
      return;
    }
    output(sessionId, frameOf(seconds, frame));
  }, 700);
}

const sessions = new Map<number, Record<string, unknown>>();
let nextSessionId = 0;
const takenIds = new Set<number>();

/**
 * The backend id of a new session: its number in the workspace (1 to 5), so
 * the script, the crew and the meters stay with their session whichever the app starts first
 * (it starts the active one first); otherwise the next free number.
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
    if (id === 2) {
      keepTicking(
        id,
        CODEX_START_SECONDS,
        (s, f) => `${RESTORE_CURSOR}${E}2K${codexSpinner(s, f)}`,
      );
    }
    if (id === 1 && CLAUDE_BUSY && !GATE_PENDING) {
      keepTicking(
        id,
        CLAUDE_START_SECONDS,
        (s, f) => `${RESTORE_CURSOR}${E}2K${claudeSpinner(s, f)}`,
      );
    }
    // The held command: the guard's decision reaches the window once the session is there.
    if (id === 1 && GATE_PENDING) {
      setTimeout(() => {
        emitEvent("gate-changed", null);
      }, 300);
    }
    // Gemini works long enough for its quiet end to count as done, answers, then waits.
    if (id === 4) {
      keepTicking(id, 0, (s, f) => `${RESTORE_CURSOR}${E}2K${geminiSpinner(s, f)}`, GEMINI_WORK_MS);
      setTimeout(() => {
        output(id, `${RESTORE_CURSOR}${E}2K${geminiOut[1] ?? ""}`);
        control.geminiAnswered = true;
      }, GEMINI_WORK_MS + 300);
    }
    // A bell makes the session show up as "asks you something" in the sidebar.
    if (id === 3) {
      setTimeout(() => output(id, "\x07"), 300);
    }
  }, 600);
  return info;
}

// The sub-agents of the first Claude Code session, named by their definition (the crew column
// gives each its figure), with what they do right now and their tokens. The security agent of the
// crew scene stopped before the guard's decision came up, so the decision is its.
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
const BACKEND_AGENT = subAgent(
  "backend",
  "Retries with backoff",
  "backend",
  MINUTE + 38_000,
  2_000,
  "Edit",
  "src/payments/webhook.ts",
  52_310,
);
const TESTER_AGENT = subAgent(
  "tester",
  "Write retry tests",
  "tester",
  MINUTE + 12_000,
  3_000,
  "Bash",
  "pnpm test payments",
  61_870,
);
const SECURITY_AGENT = subAgent(
  "security",
  "Check the new code",
  "security",
  MINUTE + 4_000,
  40_000,
  "Bash",
  GUARDED_COMMAND,
  24_150,
);
const subAgents =
  SCENE === "crew"
    ? [BACKEND_AGENT, TESTER_AGENT, SECURITY_AGENT]
    : CLAUDE_BUSY
      ? [BACKEND_AGENT, TESTER_AGENT]
      : [];

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
const ACCOUNT_BASE = SELF_HOSTED
  ? "https://team.example.com/api/v1"
  : "https://account.example.com";
const accountView = {
  baseUrl: ACCOUNT_BASE,
  official: !SELF_HOSTED,
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
/**
 * "Test connection" on the team's own server: the account API answers, but the web server has no
 * rule that forwards the relay's paths to the container, so its health check gets the proxy's
 * 404 and the live connection waits for the relay.
 */
const serverCheck = {
  baseUrl: ACCOUNT_BASE,
  api: { kind: "ok" },
  relay: { kind: "noProxy", status: 404 },
  live: { kind: "skipped" },
  relayHost: "team.example.com",
  relaySource: "derived",
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

// How full the agent's context window is (the meter in its sidebar row); the backend reads it
// from Claude Code's transcript only.
const contextUsage = {
  sessions: [{ sessionId: 1, used: 123_000, window: 1_000_000 }],
};

// Git: where each folder's repository is and how many files are changed per checkout.
const repoOf = (cwd: string) => (cwd === DOCS ? DOCS : ROOT);
/** Changed files per checkout, as `git status` would count them. */
const changedFiles: Record<string, number> = {
  [ROOT]: 4,
  [WORKTREE]: 2,
  [TEAM_WORKTREE]: 4,
  [DOCS]: 1,
};

function sessionCwd(args: Args): string {
  const info = typeof args.id === "number" ? sessions.get(args.id) : undefined;
  return typeof info?.cwd === "string" ? info.cwd : ROOT;
}

// Sub-agent definitions: Claude Code's own (the ones that start as an agent and wear a figure),
// and Hangar's copies of them for Codex CLI. Figures not chosen here come from the agent's name.
const definition = (name: string, description: string, model: string | null, color: string) => ({
  name,
  description,
  model,
  color,
  tools: ["Read", "Grep", "Glob", "Bash"],
  extras: [],
  scope: "global",
  folder: null,
  file: `${name}.md`,
});
const definitions = [
  definition(
    "backend",
    "Changes server code, APIs and database queries. Use it for handlers, services and migrations.",
    "sonnet",
    "blue",
  ),
  definition(
    "developer",
    "Senior developer for work that touches several layers or belongs to no single role.",
    "opus",
    "purple",
  ),
  definition(
    "frontend",
    "Builds and polishes what people see: components, styles, texts and accessibility.",
    "sonnet",
    "pink",
  ),
  definition(
    "manager",
    "Reviews finished work against the task and the project rules before anything is committed.",
    "opus",
    "orange",
  ),
  definition(
    "researcher",
    "Finds answers in the code and the docs before anything is built. Reads only.",
    "haiku",
    "cyan",
  ),
  definition(
    "security",
    "Checks code, configuration and commands for vulnerabilities and unsafe changes.",
    null,
    "red",
  ),
  definition(
    "tester",
    "Runs the checks, writes missing tests and reproduces reported bugs.",
    "haiku",
    "green",
  ),
];
const CODEX_CHOICES: Record<string, [string, string]> = {
  backend: ["gpt-example", "medium"],
  developer: ["gpt-example", "high"],
  frontend: ["gpt-example", "medium"],
  manager: ["gpt-example", "high"],
  researcher: ["gpt-example-mini", "low"],
  security: ["gpt-example", "high"],
  tester: ["gpt-example-mini", "medium"],
};
const codexCopies = definitions.map((entry) => ({
  name: entry.name,
  description: entry.description,
  model: CODEX_CHOICES[entry.name]?.[0] ?? null,
  effort: CODEX_CHOICES[entry.name]?.[1] ?? null,
  scope: "global",
  folder: null,
  file: `${entry.name}.toml`,
  managed: true,
  backup: null,
  backups: [],
}));
const subAgentLibrary = {
  defs: definitions,
  codexDefs: codexCopies,
  problems: [],
  installed: ["claude", "codex"],
  targets: [{ program: "codex", enabled: true }],
  codexModels: ["gpt-example", "gpt-example-mini"],
  syncIssues: [],
};
const subAgentSettings = {
  models: {
    claude: { model: "sonnet", effort: null },
    codex: { model: "gpt-example", effort: "medium" },
  },
  provideOff: [],
};
/** The figures the user picked: one of the newest of each role. */
const subAgentFigures = {
  backend: { kind: "builtin", id: "backend-03" },
  developer: { kind: "builtin", id: "developer-03" },
  frontend: { kind: "builtin", id: "frontend-04" },
  manager: { kind: "builtin", id: "manager-04" },
  researcher: { kind: "builtin", id: "researcher-03" },
  security: { kind: "builtin", id: "security-03" },
  tester: { kind: "builtin", id: "tester-02" },
};

// The preferences the backend keeps; the dictation scene has its key on.
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
  budgetWarnedDay: null,
  dictationEnabled: SCENE === "dictation",
};

// Dictation: the model is installed; holding Ctrl+Shift+Space opens the microphone for the active
// session. The input level stays at a fixed value, so the meter reads the same in every run.
let dictationState: Record<string, unknown> = { kind: "ready" };
const dictationStatus = () => ({
  state: dictationState,
  models: [
    { id: "base", bytes: 148_000_000, installed: true },
    { id: "small", bytes: 488_000_000, installed: false },
  ],
  source: "huggingface.co",
  microphone: true,
});
function startDictation(args: Args): null {
  const session = typeof args.id === "number" ? args.id : 1;
  dictationState = { kind: "recording", session };
  emitEvent("dictation-status", null);
  const level = () => {
    emitEvent("dictation-level", 0.58);
  };
  level();
  window.setTimeout(level, 150);
  return null;
}

// Partner projects (ADR 0012): the shop and its docs site may send each other messages, under
// the short names "shop" and "docs". acme-shop's worktrees belong to it too.
const partnerBook = {
  projects: [
    { name: "shop", folder: ROOT },
    { name: "docs", folder: DOCS },
  ],
  pairs: [["shop", "docs"]],
};

/** The partner project of a session and its sessions there, oldest first. */
function partnerTargets(args: Args): unknown {
  const partner = partnerBook.projects.find((project) => project.name === args.partner);
  const sender = partnerBook.projects.find((project) => project.name !== args.partner);
  if (partner === undefined || sender === undefined) return null;
  return {
    sender: sender.name,
    partner,
    sessions: [...sessions.entries()]
      .filter(
        ([, info]) => repoOf(typeof info.cwd === "string" ? info.cwd : ROOT) === partner.folder,
      )
      .map(([id, info]) => ({ id, profileId: info.profileId, running: info.status === "running" })),
  };
}

/**
 * The messages scene: the shop's Claude Code told the docs project about the new retries with
 * hangar-send. It waits on the docs project's agent session until the person pastes it.
 */
const waitingMessages = MESSAGE_WAITING
  ? [
      {
        id: 1,
        from: "shop",
        to: "docs",
        text:
          "The payment webhook now retries a failed delivery three times, after 250, 500 " +
          "and 1000 ms. Please add a Retries section to docs/api/webhooks.md.",
        at: ago(2 * MINUTE),
      },
    ]
  : [];

// The crew scene: the command guard holds the security agent's command until the person answers.
const gateSnapshot = GATE_PENDING
  ? {
      enabled: true,
      pending: [
        {
          id: 1,
          sessionId: 1,
          tool: "Bash",
          path: "",
          otherSessions: [],
          teammates: [],
          reserved: [],
          level: "sameFile",
          lines: [],
          createdAt: ago(9_000),
          command: {
            rules: [
              {
                id: "git-clean-force",
                description: "git clean -fdx deletes every untracked and ignored file",
                builtin: true,
              },
            ],
            call: GUARDED_COMMAND,
            text: GUARDED_COMMAND,
            hidden: 0,
            rememberable: true,
          },
        },
      ],
      log: [],
    }
  : { enabled: true, pending: [], log: [] };

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

/** Sends a backend event to every listener of `event`. */
function emitEvent(event: string, payload: unknown): void {
  for (const handler of listeners.get(event) ?? []) {
    callbacks.get(handler)?.({ event, id: 0, payload });
  }
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
      paths.map((p: unknown) => [
        p,
        p === WORKTREE ? WORKTREE_BRANCH : p === TEAM_WORKTREE ? TEAM_BRANCH : "main",
      ]),
    );
  },
  git_diff: () => ({ files: [] }),
  // No scene opens the file tree or the notes.
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
  // The test takes a moment, as it does against a real server.
  account_check_server: () =>
    new Promise((resolve) => {
      setTimeout(() => {
        resolve(serverCheck);
      }, 400);
    }),
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
        return [path, { root, workTree: path === DOCS ? DOCS : root }];
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
  gate_snapshot: () => gateSnapshot,
  // Sub-agent definitions, their figures and models.
  sub_agent_library: () => subAgentLibrary,
  sub_agent_figures: () => subAgentFigures,
  sub_agent_settings: () => subAgentSettings,
  sync_sub_agents: () => null,
  // Every session binds its work tree snapshots; no scene shows them, nor the cost report.
  snapshot_bind: () => null,
  snapshot_take: () => null,
  cost_report: () => null,
  // Partner projects and the messages waiting from them.
  partner_projects: () => partnerBook,
  link_partner: () => partnerBook,
  unlink_partner: () => partnerBook,
  partner_targets: partnerTargets,
  deliver_partner_message: (args) => (typeof args.text === "string" ? args.text : ""),
  waiting_messages: () => waitingMessages,
  waiting_message_text: (args) =>
    waitingMessages.find((message) => message.id === args.id)?.text ?? "",
  close_waiting_message: () => null,
  // Dictation.
  dictation_status: dictationStatus,
  dictation_start: startDictation,
  dictation_stop: () => null,
  dictation_take_text: () => [],
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

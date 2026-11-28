// In-memory stand-in for the Tauri backend, used only for the README screenshots. It answers the
// IPC calls the frontend makes and plays a scripted, fictional day in the project "acme-shop":
// Claude Code working with a crew of Hangar's own sub-agents next to Codex CLI in its own worktree,
// a second Claude Code session started as the "hangar-security" sub-agent, Gemini CLI done with the
// API docs in the project "acme-docs", a PowerShell running the docs site's dev server and a
// signed-in account in a team of four. Three of the agents form a link group with a lead; each
// agent started with a model of its own, and Ollama and LM Studio run on the computer.
// Per scene it adds what that scene shows: Hangar's status line in Claude Code, the team's
// decisions and a request that waits for an answer, two link groups to merge, a crew of reviewers
// with two visitors that are not Hangar's own.
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
/** The link groups scene: two groups, one member of each picked with Ctrl+click to merge them. */
const TWO_GROUPS = SCENE === "link-groups";
/**
 * The status line, model menu and link groups scenes: the lead works alone, without a crew, so its
 * status line has the room of a whole pane and the sidebar room for what the scene shows below the
 * rows ("New session", the bar of a selection).
 */
const SOLO = SCENE === "status-line" || SCENE === "model-menu" || TWO_GROUPS;

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

// A fixed clock, so every time the app shows (run times, "since 09:12") is the same in each run:
// it starts at FIXED_START and stands at SHOT_TIME once the driver freezes the scene. In the
// overview it jumps WAIT_JUMP ahead a few seconds after a session asked something, so that session
// has waited for 12 minutes by the time of the screenshot (waitHint.ts).
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
// Timers of a minute or more (the wait hint's, waitHint.ts) run on that clock too: set before the
// jump, they fire once the jumped clock has reached them, as they would after real minutes.
const longTimers = new Map<number, { run: () => void; due: number }>();
const realSetTimeout = window.setTimeout.bind(window);
function clockTimeout(handler: TimerHandler, ms?: number, ...rest: unknown[]): number {
  if (typeof handler !== "function" || ms === undefined || ms < MINUTE) {
    return realSetTimeout(handler, ms, ...rest);
  }
  const run = () => {
    Reflect.apply(handler, window, rest);
  };
  const id = realSetTimeout(() => {
    longTimers.delete(id);
    run();
  }, ms);
  longTimers.set(id, { run, due: Date.now() + ms });
  return id;
}
if (WAIT_JUMP > 0) Reflect.set(window, "setTimeout", clockTimeout);
/** Moves the clock WAIT_JUMP ahead and lets the long timers that are due by then fire. */
function jumpClock(): void {
  jumped = true;
  for (const [id, timer] of longTimers) {
    window.clearTimeout(id);
    longTimers.delete(id);
    realSetTimeout(timer.run, Math.max(0, timer.due - Date.now()));
  }
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

// The models "New session" offers per program, as the agent catalog lists them
// (src-tauri/src/agent_catalog.toml): the vendor's, with their reasoning efforts, and the local
// servers the program can run against.
const model = (id: string, label: string, efforts?: string[]) =>
  efforts === undefined ? { id, label } : { id, label, efforts };
const CLAUDE_EFFORTS = ["low", "medium", "high", "xhigh", "max"];
const CODEX_EFFORTS = ["low", "medium", "high", "xhigh"];
const LOCAL = ["ollama", "lmstudio"];

const profiles = [
  {
    ...catalogProfile("claude", "Claude Code", ["--continue"]),
    models: [
      model("fable", "Fable", CLAUDE_EFFORTS),
      model("opus", "Opus", CLAUDE_EFFORTS),
      model("sonnet", "Sonnet", CLAUDE_EFFORTS),
      model("haiku", "Haiku"),
    ],
    localSources: LOCAL,
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
  {
    ...catalogProfile("codex", "Codex CLI", ["resume", "--last"]),
    // Codex CLI's efforts per model come from its own model list (codex_models.rs).
    models: [
      model("gpt-6.2-sol", "GPT-6.2 Sol", CODEX_EFFORTS),
      model("gpt-6.2-astra", "GPT-6.2 Astra", CODEX_EFFORTS),
      model("gpt-6.1-terra", "GPT-6.1 Terra", CODEX_EFFORTS),
      model("gpt-6-luna", "GPT-6 Luna"),
    ],
    localSources: LOCAL,
  },
  {
    ...catalogProfile("gemini", "Gemini CLI", null),
    models: [
      model("auto", "Auto"),
      model("pro", "Pro"),
      model("flash", "Flash"),
      model("flash-lite", "Flash Lite"),
    ],
  },
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
/** The model a session was started with (workspace.ts `model`): its pill on the row. */
const started = (id: string, effort?: string) =>
  effort === undefined ? { source: "provider", id } : { source: "provider", id, effort };
/** The sidebar of every scene: five sessions, two of them Claude Code. */
const baseSessions = [
  {
    ...plainSession(1, "claude", ROOT, "Webhook retries"),
    cliSessionId: "cccccccc-0000-4000-8000-000000000001",
    model: started("opus", "high"),
  },
  {
    // Renamed after its task, as people do with worktree sessions.
    ...plainSession(2, "codex", WORKTREE, "Cart rounding"),
    worktree: { path: WORKTREE, branch: WORKTREE_BRANCH, repoRoot: ROOT },
    model: started("gpt-6.2-sol"),
  },
  {
    // Claude Code started as the "hangar-security" sub-agent: its figure is the session's avatar,
    // its name the session's title.
    ...plainSession(3, "claude", ROOT, "hangar-security"),
    subAgent: "hangar-security",
    cliSessionId: "cccccccc-0000-4000-8000-000000000003",
    model: started("sonnet"),
  },
  { ...plainSession(4, "gemini", DOCS, "Refund docs"), model: started("pro") },
  plainSession(5, "pwsh", DOCS, "Docs server"),
];
const [lead, cart, security, refundDocs] = baseSessions;
/**
 * The sessions of the scene. Each keeps its number in the base list (and with it its script),
 * whatever the order (`sessionId`). The overview and the sub-agents scene leave out the docs
 * server, so their sidebars, with the lead's crew in a tree or the bar of a selection below the
 * rows, show every row whole; the model menu
 * keeps only the acme-shop sessions, so "New session" has room below them. The overview
 * lists Gemini CLI second, so it fills the second pane beside the lead (split.ts `fillSlots`).
 */
const mainSessions =
  SCENE === "overview"
    ? [lead, refundDocs, cart, security].filter((entry) => entry !== undefined)
    : REVIEW || TWO_GROUPS
      ? baseSessions.slice(0, 4)
      : SCENE === "model-menu"
        ? baseSessions.slice(0, 3)
        : baseSessions;
// The sidebar keeps the workspace's order, and the panes fill in it (split.ts `fillSlots`).
const workspace = {
  version: 1,
  activeKey: uuid(1),
  sessions: mainSessions.map((entry, order) => ({ ...entry, order })),
};
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
const yellow = (s: string) => fg(33, s);
const grey = (s: string) => fg(90, s);
const lines = (l: string[]) => l.join("\r\n");

const CLEAR = "\x1b[2J\x1b[3J\x1b[H";
const SAVE_CURSOR = "\x1b7";
const RESTORE_CURSOR = "\x1b8";
const HIDE_CURSOR = `${E}?25l`;
const rule = (width: number) => dim("─".repeat(width));

// Hangar's status line, as `hangar status-line claude` prints it below Claude Code's prompt
// (src-tauri/src/status_line.rs): where the session works on the first line, how it goes on the
// second, each segment with a priority. As there, the least important segments give way until a
// line fits the terminal's width less four columns.
/** A segment: its priority (0 matters most), its text and whether it is glued to the one before. */
type Segment = [number, string, boolean];
/** Columns a text takes: no SGR codes, emoji two columns, an emoji selector one more. */
function columns(text: string): number {
  let width = 0;
  for (const char of text.replace(/\x1b\[[0-9;]*m/g, "")) {
    const code = char.codePointAt(0) ?? 0;
    width += code === 0x26a1 || code >= 0x1f300 ? 2 : 1;
  }
  return width;
}
const joined = (items: readonly Segment[]) =>
  items
    .map(([, text, glue], index) => (index === 0 ? text : `${glue ? " " : " | "}${text}`))
    .join("");
function fitLine(segments: readonly Segment[], max: number): string {
  const items = [...segments];
  while (items.length > 1 && columns(joined(items)) > max) {
    let index = 0;
    items.forEach(([prio], i) => {
      if (prio >= (items[index]?.[0] ?? 0)) index = i;
    });
    let end = index + 1;
    if (items[index]?.[2] === false) while (items[end]?.[2] === true) end += 1;
    const next = items[end];
    if (index === 0 && next !== undefined) items[end] = [next[0], next[1], false];
    items.splice(index, end - index);
  }
  return joined(items);
}
/** The lead's two lines in acme-shop: Opus at high effort, 52 minutes in, 12 % of its context. */
function statusLines(cols: number): string[] {
  const where: Segment[] = [
    [0, cyan("[Opus]"), false],
    [3, yellow("high"), true],
    [9, grey("v2.1.319"), true],
    [5, "👤 Sam Carter", false],
    [8, grey("Max"), true],
    [1, "📁 acme-shop", false],
    [2, `🌿 main ${cyan("↑1")} ${green("+2")} ${yellow("~3")} ${grey("?1")}`, false],
    [4, yellow("⇄ #128 …"), false],
  ];
  const how: Segment[] = [
    [0, `${green("█")}${grey("░".repeat(9))} ${green("12%")}`, false],
    [6, grey("(123k/1M)"), true],
    [4, "⚡ 94%", false],
    [6, `✏️ ${green("+142")} ${red("-37")}`, false],
    [1, yellow("💰 $9.06"), false],
    [5, grey("($10.45/h)"), true],
    [3, "⏱️ 52m 04s", false],
    [1, `🔋 5h ${green("34%")}`, false],
    [5, grey("↻ 2h13m"), true],
    [2, `📅 7d ${green("61%")}`, false],
    [6, grey("↻ 3d4h"), true],
  ];
  const max = Math.max(10, cols - 4);
  return [fitLine(where, max), fitLine(how, max)];
}

// Claude Code's own screen: what it did so far at the top; at the bottom a spinner line that ticks
// while it works, the prompt between two rules and, below it, Hangar's status line. The terminal
// cursor is hidden, the prompt draws its own.
const CLAUDE_START_SECONDS = 112;
function claudeSpinner(seconds: number, frame: number): string {
  const glyph = ["✻", "✽", "✶", "✳"][frame % 4] ?? "✻";
  const time = `${String(Math.floor(seconds / 60))}m ${String(seconds % 60)}s`;
  return magenta(`${glyph} Working…`) + dim(` (${time} · esc to interrupt)`);
}
/** The screen of size `cols` x `rows`: `body` from the top, the prompt and its status line below. */
function claudeScreen(body: readonly string[], cols: number, rows: number): string {
  const width = Math.max(20, cols - 2);
  const foot = [
    SAVE_CURSOR + claudeSpinner(CLAUDE_START_SECONDS, 0),
    "",
    rule(width),
    `> ${E}7m ${E}27m`,
    rule(width),
    ...statusLines(cols),
  ];
  // The foot sits on the last rows, as once Claude Code's output has filled the screen.
  const gap = Math.max(1, rows - body.length - foot.length);
  return CLEAR + HIDE_CURSOR + lines([...body, ...Array<string>(gap).fill("")]) + lines(foot);
}
const task = (agent: string, what: string) => `${green("●")} ${bold(agent)}(${what})`;

/** The lead of the link group at work: three sub-agents run while it waits for them. */
const claudeCrewOut = [
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
  dim("  ⎿  ") + "Edit(src/ui/Banner.tsx)",
  "",
  task("hangar-tester", "Write retry tests"),
  dim("  ⎿  ") + "Bash(vitest)",
  dim("     +9 more tool uses"),
];

/**
 * The status line scene: the lead works on the retries alone, so its terminal has the whole pane
 * and the status line its full width.
 */
const claudeSoloOut = [
  dim("> ") + "Add retries with backoff to the payment webhook and cover them with tests",
  "",
  "● I'll wrap the delivery in a retry loop with three delays, then add tests for the",
  "  success after a retry and for giving up.",
  "",
  `${green("●")} ${bold("Update")}(src/payments/webhook.ts)`,
  dim("  ⎿  ") + "Updated src/payments/webhook.ts with 14 additions and 3 removals",
  dim("      36 ") + "   const url = endpointFor(event);",
  dim("      37 ") + "   log.debug(`delivering ${event.id}`);",
  dim("      38 ") + red("-  const res = await send(event);"),
  dim("      38 ") + green("+  const delays = [250, 500, 1000];"),
  dim("      39 ") + green("+  for (const delay of delays) {"),
  dim("      40 ") + green("+    const res = await send(event);"),
  dim("      41 ") + green("+    if (res.ok) return res;"),
  dim("      42 ") + green("+    await sleep(delay);"),
  dim("      43 ") + green("+  }"),
  dim("      44 ") + green('+  throw new DeliveryError("gave up after three retries");'),
  "",
  `${green("●")} ${bold("Write")}(src/payments/webhook.test.ts)`,
  dim("  ⎿  ") + "Wrote 58 lines to src/payments/webhook.test.ts",
  "",
  `${green("●")} ${bold("Bash")}(pnpm test payments)`,
  dim("  ⎿  ") + green("✓") + " src/payments/webhook.test.ts (6 tests) 412ms",
  dim("     ") + "Test Files  " + green("1 passed") + " (1)",
  dim("     ") + "     Tests  " + green("6 passed") + " (6)",
  "",
  "● All six tests pass. Next I'll show a banner in the support view when a delivery",
  "  finally failed.",
];

/**
 * The sub-agents scene: the lead has the retries reviewed before the merge, by two of Hangar's
 * agents and two visitors, the team's own code-reviewer and load-tester. The manager finished a
 * while ago (the sidebar keeps it, the stage no longer), the security agent searches, the
 * code-reviewer reads and the load-tester has just started.
 */
const claudeReviewOut = [
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
];

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
const geminiBox = (mark: string, text: string, width = 36) => {
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
    dim("Tips: ask, edit files, run commands."),
    "",
    cyan("> ") + "Document the refund endpoints",
    "",
    magenta("✦") + " I'll read the OpenAPI spec and the",
    "  order pages, then write the refund",
    "  page.",
    "",
    geminiBox(green("✔"), "ReadFile openapi/refunds.yaml"),
    geminiBox(green("✔"), "ReadFile docs/api/orders.md"),
    "",
    SAVE_CURSOR + geminiSpinner(0, 0),
  ]),
  lines([
    geminiBox(green("✔"), "WriteFile docs/api/refunds.md"),
    "",
    magenta("✦") + " Done. docs/api/refunds.md covers",
    "  create, list and cancel, each with",
    "  a curl example and the error codes.",
    "",
    dim("╭" + "─".repeat(36) + "╮"),
    dim("│ ") + cyan(">") + dim("   Type your message or @path") + dim("     │"),
    dim("╰" + "─".repeat(36) + "╯"),
    dim("~\\src\\acme-docs (main*)") + "  " + dim("gemini-2.5-pro"),
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

/** The first screen of each session, by session number, for a terminal of `cols` x `rows`. */
const mainScripts: Record<number, (cols: number, rows: number) => string> = {
  1: (cols, rows) =>
    claudeScreen(REVIEW ? claudeReviewOut : SOLO ? claudeSoloOut : claudeCrewOut, cols, rows),
  2: () => CLEAR + codexOut + codexSpinner(CODEX_START_SECONDS, 0) + codexFooter,
  3: () => CLEAR + HIDE_CURSOR + claudeAskOut,
  4: () => CLEAR + HIDE_CURSOR + (geminiOut[0] ?? ""),
  5: () => CLEAR + pwshOut(DOCS, "acme-docs", 5173),
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
 * The backend id of a new session: its number in the base list, so the script, the crew and the
 * meters stay with their session whichever the app starts first (it starts the active one
 * first); otherwise the next free number.
 */
function sessionId(profileId: string, cwd: string, subAgent: string | null): number {
  const entry = mainSessions.find(
    (candidate) =>
      !takenIds.has(baseSessions.indexOf(candidate) + 1) &&
      candidate.profileId === profileId &&
      candidate.cwd === cwd &&
      ("subAgent" in candidate ? candidate.subAgent : null) === subAgent,
  );
  if (entry !== undefined) {
    const id = baseSessions.indexOf(entry) + 1;
    takenIds.add(id);
    return id;
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

/** The terminal size of each session, from its start and its last resize. */
const termSizes = new Map<number, { cols: number; rows: number }>();
/** Sessions whose first screen is out; a resize draws a Claude Code screen again to fit. */
const drawn = new Set<number>();
function draw(id: number): void {
  const size = termSizes.get(id) ?? { cols: 80, rows: 24 };
  output(id, mainScripts[id]?.(size.cols, size.rows) ?? "");
}
function resizeSession(args: Args): null {
  const id = typeof args.id === "number" ? args.id : -1;
  const size = typeof args.size === "object" && args.size !== null ? args.size : {};
  const cols = "cols" in size && typeof size.cols === "number" ? size.cols : null;
  const rows = "rows" in size && typeof size.rows === "number" ? size.rows : null;
  const before = termSizes.get(id);
  if (cols === null || rows === null || (before?.cols === cols && before.rows === rows)) {
    return null;
  }
  termSizes.set(id, { cols, rows });
  if (drawn.has(id) && sessions.get(id)?.profileId === "claude" && id !== 3) draw(id);
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
    // The model it started with, as the backend keeps it (rename_session answers with it too).
    model: typeof args.model === "object" ? args.model : null,
    // Claude Code and Codex CLI draw Hangar's status line themselves: no Hangar bar above them.
    statusLine: profileId === "claude" || profileId === "codex",
  };
  const channel = channelId(args.onEvent);
  if (channel !== undefined) channelOfSession.set(id, channel);
  sessions.set(id, info);
  const size = typeof args.size === "object" && args.size !== null ? args.size : {};
  termSizes.set(id, {
    cols: "cols" in size && typeof size.cols === "number" ? size.cols : 80,
    rows: "rows" in size && typeof size.rows === "number" ? size.rows : 24,
  });
  setTimeout(() => {
    drawn.add(id);
    draw(id);
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
      if (WAIT_JUMP > 0) setTimeout(jumpClock, 300 + JUMP_AFTER_MS);
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
    "src/ui/Banner.tsx",
    38_420,
  ),
  subAgent(
    "tester",
    "Write retry tests",
    "hangar-tester",
    MINUTE + 12_000,
    3_000,
    "Bash",
    "vitest",
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
const subAgents = REVIEW ? reviewCrew : SOLO ? [] : buildCrew;

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
// with Mika, Jonas and Priya. Free keeps the support badge in the bar above the work area
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
    seatsAccountUsed: 4,
    pendingInvitations: 0,
    canInvite: false,
  },
];
const teamStatus = { state: "connected", epoch: 4, members: 4, devices: 5 };
const MIKA = "7c41d9e2b08a4f15";
const JONAS = "b2e86f0a13c947d5";
const PRIYA = "e5a1c7b39d02f468";
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
  // In the team scene Mika asks Sam to hand the retries over: it waits for Sam's answer.
  request:
    SCENE === "team" && member === "user-sam"
      ? { member: MIKA, name: "Mika Rossi", mine: false, confirmed: true }
      : null,
});
// The team's decisions, newest first (team_decisions.rs): what was decided and why. Sam wrote the
// newest, so Sam may withdraw it.
const decision = (
  n: number,
  text: string,
  reason: string,
  member: string,
  name: string,
  daysAgo: number,
) => ({
  blob: `blob-${String(n)}`,
  id: `decision-${String(n)}`,
  decision: text,
  reason,
  member,
  name,
  at: ago(daysAgo * DAY + 3 * HOUR),
  mine: member === "user-sam",
  former: false,
  canWithdraw: member === "user-sam",
});
const decisions = [
  decision(
    1,
    "Three webhook retries, then stop",
    "The provider resends for 24 hours anyway.",
    "user-sam",
    "Sam Carter",
    0,
  ),
  decision(
    2,
    "Round cart totals once, at the end",
    "Rounding each line lost a cent on discounts.",
    MIKA,
    "Mika Rossi",
    2,
  ),
  decision(
    3,
    "Refunds over 500 EUR need two approvals",
    "Two refunds went out by mistake in October.",
    JONAS,
    "Jonas Berg",
    6,
  ),
  decision(
    4,
    "No test card numbers in the repository",
    "They live in the provider's sandbox dashboard.",
    PRIYA,
    "Priya Nair",
    13,
  ),
  // The team scene shows the request on top: the three newest fit below it.
].slice(0, SCENE === "team" ? 3 : 4);
const teamSnapshot = {
  teams: [
    {
      projectId: PROJECT,
      status: teamStatus,
      activity: {
        linked: [{ root: ACME_ROOT_COMMIT, folder: `${ROOT}\\.git`, name: "acme-shop" }],
        peers: [
          {
            member: MIKA,
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
            member: JONAS,
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
          {
            member: PRIYA,
            device: "dev-priya-1",
            name: "Priya Nair",
            own: false,
            presence: "away",
            sessions: [],
          },
        ],
        overlaps: [],
        divergent: [],
        tasks: [
          teamTask("a1", "Webhook retries", "src/payments", "user-sam", "Sam Carter", 24),
          teamTask("b2", "Checkout emails", "src/emails", MIKA, "Mika Rossi", 41),
          teamTask("c3", "Search ranking", "src/search", JONAS, "Jonas Berg", 78),
          teamTask("d4", "Sandbox reset script", "scripts", PRIYA, "Priya Nair", 190),
        ],
        attempts: [],
        notices: [],
        // Only the acme-shop sessions are shared with the team.
        sessions: [1, 2, 3],
        sessionRepos: [1, 2, 3].map((sessionId) => ({
          sessionId,
          repo: ACME_ROOT_COMMIT,
        })),
        decisions,
        decisionsState: "ready",
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

// How full each agent's context window is (the meter in its sidebar row) and how fast Claude Code
// writes ("84 tok/s"); the backend reads both from the agents' transcripts. The lead writes right
// now, the security agent's last response keeps its rate, dimmed.
const contextUsage = {
  sessions: [
    { sessionId: 1, used: 123_000, window: 1_000_000, outputRate: { value: 84, live: true } },
    { sessionId: 3, used: 41_000, window: 200_000, outputRate: { value: 61, live: false } },
  ],
};

// Ollama and LM Studio run on this computer (local_llm.rs): the pills in the top bar count their
// models, and "New session" offers them beside the vendors' own.
const localModel = (id: string, label: string, loaded = false) => ({ id, label, loaded });
const localLlm = {
  ollama: {
    running: true,
    models: [
      localModel("qwen3-coder:30b", "qwen3-coder:30b", true),
      localModel("gpt-oss:20b", "gpt-oss:20b"),
      localModel("devstral:24b", "devstral:24b"),
    ],
  },
  lmstudio: {
    running: true,
    models: [localModel("openai/gpt-oss-120b", "gpt-oss-120b")],
  },
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
// to Gemini CLI in the docs project, and collects their results. In the link groups scene the
// security agent leads a second group with Gemini CLI instead, so the two can be merged.
const linkGroups = TWO_GROUPS
  ? [
      {
        id: 1,
        color: 1,
        askFirst: false,
        braked: false,
        members: [
          { session: 1, name: "shop", role: "lead" },
          { session: 2, name: "cart", role: "member" },
        ],
      },
      {
        id: 2,
        color: 3,
        askFirst: false,
        braked: false,
        members: [
          { session: 3, name: "audit", role: "lead" },
          { session: 4, name: "docs", role: "member" },
        ],
      },
    ]
  : [
      {
        id: 1,
        color: 1,
        askFirst: false,
        braked: false,
        members: [
          { session: 1, name: "shop", role: "lead" },
          { session: 2, name: "cart", role: "member" },
          { session: 4, name: "docs", role: "member" },
        ],
      },
    ];
/** The numbers of the scene's sessions; a group keeps only those of its members. */
const present = new Set(mainSessions.map((entry) => baseSessions.indexOf(entry) + 1));
const linkState = {
  groups: linkGroups
    .map((group) => ({
      ...group,
      members: group.members.filter((member) => present.has(member.session)),
    }))
    .filter((group) => group.members.length >= 2),
  sendable: [...present].filter((id) => id !== 5),
  unattended: [],
  messages: [],
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
  // No scene opens the changes panel.
  git_diff: () => ({ isRepo: true, files: [], diff: "", truncated: false }),
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
  local_llm_status: () => localLlm,
  local_llm_refresh: () => localLlm,
  // Running sub-agents per session, for the red count on its row (subagent_counts.rs).
  sub_agent_counts: () => ({
    sessions: [{ sessionId: 1, running: subAgents.filter((a) => a.status === "running").length }],
  }),
  configure_tray: () => null,
  create_session: createSession,
  rename_session: (args) => {
    const info = typeof args.id === "number" ? sessions.get(args.id) : undefined;
    if (info && typeof args.title === "string") info.title = args.title;
    return info ?? null;
  },
  write_session: () => null,
  resize_session: resizeSession,
  ack_session: () => null,
  watch_sub_agents: watchSubAgents,
  focus_sub_agent: focusSubAgent,
  unwatch_sub_agents: (args) => {
    const sessionId = typeof args.sessionId === "number" ? args.sessionId : -1;
    return subAgentWatch.delete(sessionId);
  },
  // No scene opens the notes.
  list_note_folders: () => [],
  list_notes: () => [],
  read_note: () => null,
  write_note: () => null,
  search_notes: () => [],
  list_trash: () => [],
  shared_notes_status: () => ({ state: "unavailable" }),
  shared_notes_team: () => null,
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
  // No group waits to come back after a restart.
  link_offers: () => [],
  // No phone push is set up: none is due when a session changes its state.
  push_session: () => false,
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

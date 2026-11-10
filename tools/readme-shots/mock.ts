// In-memory stand-in for the Tauri backend, used only for the README screenshots. It answers the
// IPC calls the frontend makes at startup and plays a scripted, fictional scene: Claude Code and
// Codex CLI side by side in the project "acme-shop", Aider waiting for an answer, a PowerShell.
// Nothing here talks to a real process, file or network.

declare const __README_SHOTS_VERSION__: string;

interface ReadmeShotsControl {
  /** Set by the driver right before the screenshot so the spinner stops on a stable frame. */
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
const lines = (l: string[]) => l.join("\r\n");

const CLEAR = "\x1b[2J\x1b[3J\x1b[H";
const SAVE_CURSOR = "\x1b7";
const RESTORE_CURSOR = "\x1b8";

const claudeOut =
  lines([
    dim("> ") + "What does the payment webhook do on errors?",
    "",
    "● It gives up after the first failed call and",
    '  logs "webhook failed". Events are lost when',
    "  the provider is briefly unreachable.",
    "",
    dim("> ") + "Add retries with backoff to the payment",
    dim("  ") + "webhook and cover them with tests",
    "",
    "● I'll start with the webhook handler.",
    "",
    green("●") + " " + bold("Read") + "(src/payments/webhook.ts)",
    dim("  ⎿  Read 84 lines"),
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
    green("●") + " " + bold("Bash") + "(pnpm test payments)",
    dim("  ⎿  ") + green("✓") + " retries on 503 " + dim("(12 ms)"),
    dim("     ") + green("✓") + " gives up after 4 attempts " + dim("(9 ms)"),
    dim("     ") + green("✓") + " keeps the event id stable " + dim("(4 ms)"),
    dim("     Tests  ") + green("17 passed") + dim(" (17)"),
    "",
    "● Done. The webhook now retries up to four",
    "  times with exponential backoff (250 ms to",
    "  2 s) and logs each attempt. Three new",
    "  tests cover retry, give-up and idempotency.",
    "",
    dim("─".repeat(52)),
    "> ",
    dim("─".repeat(52)),
    dim("  ? for shortcuts"),
  ]) +
  // Put the cursor back into the prompt line, two rows up.
  `${E}2A${E}3G`;

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
  claude: CLEAR + claudeOut,
  codex: CLEAR + codexOut + codexSpinner(CODEX_START_SECONDS, 0) + codexFooter,
  aider: CLEAR + aiderOut,
  pwsh: CLEAR + pwshOut,
};

// IPC plumbing: Tauri delivers channel messages through registered callbacks.
const callbacks = new Map<number, (value: unknown) => void>();
let nextCallback = 1;
const channelOfSession = new Map<number, number>();
const channelIndex = new Map<number, number>();

function emit(sessionId: number, event: unknown): void {
  const channel = channelOfSession.get(sessionId);
  if (channel === undefined) return;
  const index = channelIndex.get(channel) ?? 0;
  channelIndex.set(channel, index + 1);
  callbacks.get(channel)?.({ message: event, index });
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

/** Codex keeps working: its spinner line ticks until the driver freezes the scene. */
function keepCodexWorking(sessionId: number): void {
  let seconds = CODEX_START_SECONDS;
  let frame = 0;
  setInterval(() => {
    if (control.freeze) return;
    seconds += 1;
    frame += 1;
    output(sessionId, `${RESTORE_CURSOR}${E}2K${codexSpinner(seconds, frame)}`);
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
    createdAt: Date.now() - (AGE_MINUTES[id - 1] ?? 5) * 60_000,
    resumed: args.resume === true,
    cliSessionId: typeof args.cliSessionId === "string" ? args.cliSessionId : null,
    subAgents: false,
    gate: "none",
  };
  const channel = channelId(args.onEvent);
  if (channel !== undefined) channelOfSession.set(id, channel);
  sessions.set(id, info);
  setTimeout(() => {
    output(id, scripts[profileId] ?? "");
    if (profileId === "codex") keepCodexWorking(id);
    // A bell makes Aider show up as "asks you something" in the sidebar.
    if (profileId === "aider") setTimeout(() => output(id, "\x07"), 300);
  }, 600);
  return info;
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
  list_note_folders: () => [],
  list_notes: () => [],
  create_session: createSession,
  rename_session: (args) => {
    const info = typeof args.id === "number" ? sessions.get(args.id) : undefined;
    if (info && typeof args.title === "string") info.title = args.title;
    return info ?? null;
  },
  write_session: () => null,
  resize_session: () => null,
  ack_session: () => null,
  // Features this scene does not show (account, team, file claims, gates): nothing to report.
  account_status: () => null,
  profile_path: () => null,
  configure_claims: () => null,
  configure_gate: () => null,
  claims_snapshot: () => null,
  gate_snapshot: () => null,
  team_pending_confirmations: () => [],
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
    if (cmd.startsWith("plugin:event|listen")) return Promise.resolve(nextCallback++);
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

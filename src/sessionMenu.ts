// Session menu (right click or context menu key on a session in the sidebar, the rail or a pane
// header). Pure: the entries depend only on the session's state; the caller attaches what each
// action does.

import type { AttentionState } from "./attention";
import type { Translate } from "./i18n";
import type { SessionView } from "./sessions";
import { canExportReport } from "./sessionReport";

export type SessionAction =
  | "rename"
  | "marker"
  | "commands"
  | "report"
  | "subAgents"
  | "queue"
  | "discardQueue"
  | "peek"
  | "interrupt"
  | "restart"
  | "kill"
  | "close";

export type SessionMenuEntry =
  { type: "action"; key: SessionAction; label: string } | { type: "separator"; key: string };

/** What the queued-message entries offer: queue one, edit or discard it, or nothing. */
export type QueueOffer = "offer" | "queued" | null;

/**
 * "Queue message" only where Hangar can tell when the session is done: a running agent session
 * (no plain shell) with attention detection on, while it works. A queued message can always be
 * edited or discarded, also once the session waits, asks or ended.
 */
export function queueOffer(input: {
  status: SessionView["status"];
  agent: boolean;
  attentionEnabled: boolean;
  state: AttentionState;
  queued: boolean;
}): QueueOffer {
  if (input.queued) return "queued";
  const detected = input.agent && input.attentionEnabled && input.status === "running";
  return detected && input.state === "working" ? "offer" : null;
}

/** The stall hint's entries; `peek` only where the hover peek can show lines (not on screen). */
export interface StallOffer {
  peek: boolean;
}

/**
 * The stall hint's entries while the hint shows; `null` otherwise. The last lines only for a
 * session that is not on screen anyway (its own terminal shows more).
 */
export function stallOffer(stalled: boolean, onScreen: boolean): StallOffer | null {
  return stalled ? { peek: !onScreen } : null;
}

export interface SessionMenuExtras {
  queue: QueueOffer;
  stall: StallOffer | null;
  /** The session offers program commands (agentCommands.ts), so "Commands" opens them. */
  commands?: boolean;
}

const NO_EXTRAS: SessionMenuExtras = { queue: null, stall: null };

/**
 * Rename, marker, then the program's commands (only while the session offers some,
 * agentCommands.ts), export as report (only for programs whose transcript Hangar reads), then show
 * or hide the sub-agents (only for sessions that have them), then the stall hint's help (last
 * lines, Ctrl+C) and the queued message, then restart (ended) or end process (running), then
 * close. `subAgentsShown`: the sub-agent view of this session is open, so the entry hides it.
 */
export function sessionMenuEntries(
  session: Pick<SessionView, "status" | "subAgents">,
  t: Translate,
  subAgentsShown = false,
  extras: SessionMenuExtras = NO_EXTRAS,
): SessionMenuEntry[] {
  const exited = session.status === "exited";
  const subAgents: SessionMenuEntry[] =
    session.subAgents === true || subAgentsShown
      ? [
          { type: "separator", key: "sep-sub-agents" },
          {
            type: "action",
            key: "subAgents",
            label: subAgentsShown ? t("session.hideSubAgents") : t("session.showSubAgents"),
          },
        ]
      : [];
  const stall: SessionMenuEntry[] = [];
  if (extras.stall !== null) {
    stall.push({ type: "separator", key: "sep-stall" });
    if (extras.stall.peek) stall.push({ type: "action", key: "peek", label: t("stall.peek") });
    stall.push({
      type: "action",
      key: "interrupt",
      label: t("stall.interrupt", { keys: `${t("key.ctrl")}+C` }),
    });
  }
  const queue: SessionMenuEntry[] =
    extras.queue === null
      ? []
      : extras.queue === "offer"
        ? [
            { type: "separator", key: "sep-queue" },
            { type: "action", key: "queue", label: t("queue.add") },
          ]
        : [
            { type: "separator", key: "sep-queue" },
            { type: "action", key: "queue", label: t("queue.edit") },
            { type: "action", key: "discardQueue", label: t("queue.discard") },
          ];
  return [
    { type: "action", key: "rename", label: t("session.rename") },
    { type: "action", key: "marker", label: t("menu.marker") },
    ...(extras.commands === true
      ? [{ type: "action", key: "commands", label: t("agentCommand.menu") } as const]
      : []),
    ...(canExportReport(session)
      ? [{ type: "action", key: "report", label: t("session.exportReport") } as const]
      : []),
    ...subAgents,
    ...stall,
    ...queue,
    { type: "separator", key: "sep" },
    exited
      ? { type: "action", key: "restart", label: t("session.restart") }
      : { type: "action", key: "kill", label: t("session.kill") },
    {
      type: "action",
      key: "close",
      label: exited ? t("session.close") : t("session.killAndClose"),
    },
  ];
}

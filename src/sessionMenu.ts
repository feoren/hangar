// Session menu (right click or context menu key on a session in the sidebar, the rail or a pane
// header). Pure: the entries depend only on the session's state; the caller attaches what each
// action does.

import type { Translate } from "./i18n";
import type { SessionView } from "./sessions";

export type SessionAction =
  "rename" | "marker" | "handover" | "subAgents" | "restart" | "kill" | "close";

export type SessionMenuEntry =
  { type: "action"; key: SessionAction; label: string } | { type: "separator"; key: string };

/**
 * Rename, marker, hand over (only agent sessions, `handover`), then show or hide the sub-agents
 * (only for sessions that have them), then restart (ended) or end process (running), then close.
 * `subAgentsShown`: the sub-agent view of this session is open, so the entry hides it.
 */
export function sessionMenuEntries(
  session: Pick<SessionView, "status" | "subAgents">,
  t: Translate,
  subAgentsShown = false,
  handover = false,
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
  return [
    { type: "action", key: "rename", label: t("session.rename") },
    { type: "action", key: "marker", label: t("menu.marker") },
    ...(handover ? [{ type: "action", key: "handover", label: t("handover.menu") } as const] : []),
    ...subAgents,
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

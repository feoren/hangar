// Session menu (right click or context menu key on a session in the sidebar, the rail or a pane
// header, and the "More actions" button of a sidebar row). Pure: the entries depend only on the
// session's state; the caller attaches what each action does.

import type { Translate } from "./i18n";
import type { SessionView } from "./sessions";

export type SessionAction =
  "rename" | "marker" | "files" | "subAgents" | "restart" | "kill" | "close";

export type SessionMenuEntry =
  | {
      type: "action";
      key: SessionAction;
      label: string;
      /** A toggle that is on right now (the file tree of this session shows); read as "checked". */
      current?: boolean;
    }
  | { type: "separator"; key: string };

export interface SessionMenuView {
  /** The sub-agent view of this session is open, so the entry hides it. */
  subAgentsShown?: boolean;
  /** The right panel shows this session's project folder, so the entry hides it. */
  filesShown?: boolean;
}

/**
 * Rename, marker, then what can be shown beside the terminal (the project folder for every
 * session, the sub-agents only for sessions that have them), then restart (ended) or end process
 * (running), then close.
 */
export function sessionMenuEntries(
  session: Pick<SessionView, "status" | "subAgents">,
  t: Translate,
  view: SessionMenuView = {},
): SessionMenuEntry[] {
  const exited = session.status === "exited";
  const subAgentsShown = view.subAgentsShown === true;
  const subAgents: SessionMenuEntry[] =
    session.subAgents === true || subAgentsShown
      ? [
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
    { type: "separator", key: "sep-view" },
    { type: "action", key: "files", label: t("files.show"), current: view.filesShown === true },
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

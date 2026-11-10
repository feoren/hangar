// The active session's sub-agents as a small tree below its row in the sidebar: one line each,
// a click opens the sub-agent view with that sub-agent highlighted. Which rows show is decided by
// subAgentTree in subAgents.ts; this component only renders them.

import { useT } from "./i18n";
import { agentTitle, elapsedMs, formatElapsed, STATUS_LABELS, subAgentTree } from "./subAgents";
import { useClock } from "./useClock";
import { useSubAgentView } from "./useSubAgents";

/** Running times count in seconds, like in the view. */
const TICK_MS = 1000;

interface Props {
  sessionId: number;
  sessionTitle: string;
  /** The watch to read (useSubAgentWatch); its snapshots re-render this tree only. */
  watchKey: string;
  /** Sub-agent highlighted in the open view, `null` when the view is closed or none is picked. */
  picked: string | null;
  /** `null` agent: the "+N more" row (opens the view without a pick). */
  onShow: (sessionId: number, agentId: string | null, byKeyboard: boolean) => void;
}

export function SubAgentTree({ sessionId, sessionTitle, watchKey, picked, onShow }: Props) {
  const t = useT();
  const now = useClock(TICK_MS);
  const tree = subAgentTree(useSubAgentView(watchKey).view.agents);
  const hint = t("subAgents.treeHint");
  return (
    <ul className="subagent-tree" aria-label={t("subAgents.region", { title: sessionTitle })}>
      {tree.agents.map((agent) => {
        const name = agentTitle(agent, t);
        const status = t(STATUS_LABELS[agent.status]);
        const ms = elapsedMs(agent, now);
        const time = ms === null ? null : formatElapsed(ms);
        const isPicked = agent.id === picked;
        return (
          <li key={agent.id} className="subagent-tree-item">
            <button
              type="button"
              className={isPicked ? "subagent-node picked" : "subagent-node"}
              title={`${name}\n${status}${time === null ? "" : ` · ${time}`}\n${hint}`}
              aria-label={[name, status, time === null ? "" : `${t("subAgents.elapsed")} ${time}`]
                .filter(Boolean)
                .join(", ")}
              aria-current={isPicked ? "true" : undefined}
              onClick={(event) => {
                onShow(sessionId, agent.id, event.detail === 0);
              }}
            >
              <span className={`subagent-dot ${agent.status}`} aria-hidden="true" />
              <span className="subagent-node-name">{name}</span>
              {time !== null && (
                <span className="subagent-node-time" aria-hidden="true">
                  {time}
                </span>
              )}
            </button>
          </li>
        );
      })}
      {tree.more > 0 && (
        <li className="subagent-tree-item">
          <button
            type="button"
            className="subagent-node more"
            title={t("session.showSubAgents")}
            onClick={(event) => {
              onShow(sessionId, null, event.detail === 0);
            }}
          >
            <span className="subagent-node-name">{t("subAgents.more", { count: tree.more })}</span>
          </button>
        </li>
      )}
    </ul>
  );
}

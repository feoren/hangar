// The active session's sub-agents as a small tree below its row in the sidebar: two lines each
// (name, run time and tokens; what it does right now), a click shows that sub-agent's activity
// beside the terminal. Finished rows carry a remove
// button (on hover or keyboard focus), and with several finished ones a last row removes them
// all. Which rows show is decided by subAgentTree in subAgents.ts; this component renders them.

import { useEffect, useRef } from "react";

import { useT } from "./i18n";
import { CloseIcon } from "./icons";
import {
  activityText,
  agentName,
  agentTitle,
  elapsedMs,
  finishedIds,
  formatElapsed,
  formatTokens,
  STATUS_LABELS,
  subAgentTree,
  visibleAgents,
} from "./subAgents";
import { useClock } from "./useClock";
import { useSubAgentView } from "./useSubAgents";

/** Running times count in seconds, like in the view. */
const TICK_MS = 1000;

interface Props {
  sessionId: number;
  sessionTitle: string;
  /** The watch to read (useSubAgentWatch); its snapshots re-render this tree only. */
  watchKey: string;
  /** Sub-agent shown in the open view, `null` when the view is closed or shows the list. */
  picked: string | null;
  /** Sub-agents removed from the tree (finished ones only). */
  hidden: ReadonlySet<string>;
  /** `null` agent: the "+N more" row (opens the list of all of them). */
  onShow: (sessionId: number, agentId: string | null, byKeyboard: boolean) => void;
  /**
   * Removes finished sub-agents from the tree and the list; their transcripts stay. By mouse the
   * terminal gets the keyboard back, by keyboard it stays in the tree.
   */
  onDismiss: (ids: readonly string[], byKeyboard: boolean) => void;
}

export function SubAgentTree({
  sessionId,
  sessionTitle,
  watchKey,
  picked,
  hidden,
  onShow,
  onDismiss,
}: Props) {
  const t = useT();
  const now = useClock(TICK_MS);
  const agents = visibleAgents(useSubAgentView(watchKey).view.agents, hidden);
  const tree = subAgentTree(agents, undefined, picked);
  const finished = finishedIds(agents);
  const hint = t("subAgents.treeHint");
  const list = useRef<HTMLUListElement>(null);
  /** Row that gets the keyboard focus once a removal has re-rendered the tree. */
  const focusAfter = useRef<string | null>(null);
  useEffect(() => {
    const id = focusAfter.current;
    if (id === null) return;
    focusAfter.current = null;
    for (const row of list.current?.querySelectorAll<HTMLElement>("[data-agent]") ?? []) {
      if (row.dataset.agent === id) row.querySelector<HTMLElement>(".subagent-node")?.focus();
    }
  });

  const dismissOne = (id: string, focused: boolean) => {
    if (focused) {
      // The keyboard stays in the tree: on the next row, else the one before.
      const index = tree.agents.findIndex((agent) => agent.id === id);
      const neighbour = tree.agents[index + 1] ?? tree.agents[index - 1];
      focusAfter.current = neighbour?.id ?? null;
    }
    onDismiss([id], focused);
  };

  return (
    <ul
      className="subagent-tree"
      aria-label={t("subAgents.region", { title: sessionTitle })}
      ref={list}
    >
      {tree.agents.map((agent) => {
        // Like Claude Code's list of background agents: the type as the name, what it does
        // now below it, run time and tokens on the right. The task's description is the title.
        const name = agentName(agent, t);
        const title = agentTitle(agent, t);
        const activity = activityText(agent, t);
        const status = t(STATUS_LABELS[agent.status]);
        const ms = elapsedMs(agent, now);
        const time = ms === null ? null : formatElapsed(ms);
        const tokens = agent.tokens === null ? null : formatTokens(agent.tokens);
        const tokensLabel = tokens === null ? null : t("subAgents.tokens", { tokens });
        const isPicked = agent.id === picked;
        const removable = agent.status === "finished";
        const details = [
          name === title ? null : title,
          // A finished one's activity line repeats its status.
          removable ? null : activity,
          [status, time, tokensLabel].filter((part) => part !== null).join(" · "),
        ].filter((part) => part !== null);
        return (
          <li
            key={agent.id}
            className={removable ? "subagent-tree-item removable" : "subagent-tree-item"}
            data-agent={agent.id}
          >
            <button
              type="button"
              className={`subagent-node agent ${agent.status}${isPicked ? " picked" : ""}`}
              title={[name, ...details, hint].join("\n")}
              aria-label={[
                name,
                ...details.slice(0, -1),
                status,
                time === null ? null : `${t("subAgents.elapsed")} ${time}`,
                tokensLabel,
              ]
                .filter((part) => part !== null)
                .join(", ")}
              aria-current={isPicked ? "true" : undefined}
              onClick={(event) => {
                onShow(sessionId, agent.id, event.detail === 0);
              }}
            >
              <span className={`subagent-dot ${agent.status}`} aria-hidden="true" />
              <span className="subagent-node-main" aria-hidden="true">
                <span className="subagent-node-head">
                  <span className="subagent-node-name">{name}</span>
                  {(time !== null || tokens !== null) && (
                    <span className="subagent-node-time">
                      {time}
                      {tokens !== null && (
                        <span className="subagent-node-tokens">
                          {time === null ? "" : " · "}
                          {`↓ ${tokens}`}
                        </span>
                      )}
                    </span>
                  )}
                </span>
                <span className="subagent-node-activity">{activity}</span>
              </span>
            </button>
            {removable && (
              <button
                type="button"
                className="icon-button subagent-dismiss"
                aria-label={t("subAgents.dismiss", { name })}
                title={`${t("subAgents.dismiss", { name })}\n${t("subAgents.dismissHelp")}`}
                onClick={(event) => {
                  dismissOne(agent.id, event.detail === 0);
                }}
              >
                <CloseIcon size={12} />
              </button>
            )}
          </li>
        );
      })}
      {tree.more > 0 && (
        <li className="subagent-tree-item">
          <button
            type="button"
            className="subagent-node more"
            title={t("subAgents.showAll")}
            onClick={(event) => {
              onShow(sessionId, null, event.detail === 0);
            }}
          >
            <span className="subagent-node-name">{t("subAgents.more", { count: tree.more })}</span>
          </button>
        </li>
      )}
      {finished.length >= 2 && (
        <li className="subagent-tree-item">
          <button
            type="button"
            className="subagent-node clear"
            title={t("subAgents.dismissHelp")}
            onClick={(event) => {
              if (event.detail === 0) {
                focusAfter.current =
                  tree.agents.find((agent) => agent.status !== "finished")?.id ?? null;
              }
              onDismiss(finished, event.detail === 0);
            }}
          >
            <span className="subagent-node-name">{t("subAgents.dismissFinished")}</span>
            <span className="subagent-node-time" aria-hidden="true">
              {finished.length}
            </span>
          </button>
        </li>
      )}
    </ul>
  );
}

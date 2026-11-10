// The sub-agent view's panes: one read-only pane per sub-agent beside the session's terminal,
// placed in the zones of a snap layout (planSubAgentPanes in subAgents.ts). Every text from the
// transcript is rendered as plain React text, never as HTML, Markdown or a link.

import { useEffect, useRef, type KeyboardEvent } from "react";

import { useT } from "./i18n";
import { CloseIcon } from "./icons";
import {
  agentTitle,
  countsText,
  elapsedMs,
  formatElapsed,
  planSubAgentPanes,
  STATUS_LABELS,
  type SubAgent,
} from "./subAgents";
import { paneStyle } from "./TerminalPane";
import { useClock } from "./useClock";
import { useSubAgentView } from "./useSubAgents";

/** Running times count in seconds. */
const TICK_MS = 1000;

interface Props {
  /** Title of the session the sub-agents belong to. */
  title: string;
  /** The watch to read (useSubAgentWatch); its snapshots re-render these panes only. */
  watchKey: string | null;
  /** Most sub-agents seen since the view opened; the app places the terminal by the same plan. */
  peak: number;
  /** Not located, none yet, unavailable or an error: one notice pane instead of agents. */
  showNotice: boolean;
  /** Text for the notice pane (not located, none yet, unavailable, error). */
  notice: string;
  /** Excerpts switched off in Settings: say so instead of "no text yet". */
  excerpts: boolean;
  /** Sub-agent picked in the sidebar's tree: its pane or list row is highlighted. */
  picked: string | null;
  onClose: () => void;
}

export function SubAgentPanes({
  title,
  watchKey,
  peak,
  showNotice,
  notice,
  excerpts,
  picked,
  onClose,
}: Props) {
  const t = useT();
  const now = useClock(TICK_MS);
  const { view: watched } = useSubAgentView(watchKey);
  const view = { ...watched, peak };
  const plan = planSubAgentPanes(view, showNotice, picked);
  const byId = new Map(view.agents.map((agent) => [agent.id, agent]));
  // Escape anywhere in the view closes it; the terminal then gets the keyboard back.
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    onClose();
  };
  const closeButton = (
    <button
      type="button"
      className="icon-button subagent-close"
      aria-label={t("session.hideSubAgents")}
      title={t("session.hideSubAgents")}
      onClick={onClose}
    >
      <CloseIcon size={14} />
    </button>
  );

  return plan.panes.map((pane, index) => {
    const zone = index + 1;
    const style = paneStyle(plan.layout, zone);
    // The first sub-agent pane carries the one close button of the view.
    const close = index === 0 ? closeButton : null;
    const key = pane.kind === "agent" ? `agent-${pane.id}` : `${pane.kind}-${String(zone)}`;
    if (pane.kind === "empty") {
      return <div key={key} className="pane-empty subagent-empty" style={style} />;
    }
    if (pane.kind === "notice") {
      return (
        <section
          key={key}
          className="subagent"
          style={style}
          aria-label={t("subAgents.region", { title })}
          onKeyDown={onKeyDown}
        >
          <header className="subagent-header">
            <h2 className="subagent-title">{t("subAgents.region", { title })}</h2>
            {close}
          </header>
          <div className="subagent-body subagent-notice">
            <p role="status">{notice}</p>
          </div>
        </section>
      );
    }
    if (pane.kind === "list") {
      return (
        <AgentList
          key={key}
          style={style}
          agents={view.agents}
          picked={picked}
          now={now}
          close={close}
          onKeyDown={onKeyDown}
        />
      );
    }
    const agent = byId.get(pane.id);
    if (agent === undefined) {
      return <div key={key} className="pane-empty subagent-empty" style={style} />;
    }
    return (
      <AgentPane
        key={key}
        style={style}
        agent={agent}
        history={view.history.get(agent.id) ?? []}
        now={now}
        excerpts={excerpts}
        picked={agent.id === picked}
        close={close}
        onKeyDown={onKeyDown}
      />
    );
  });
}

/** Status as text with a dot whose shape differs too, never color alone. */
function Status({ agent }: { agent: SubAgent }) {
  const t = useT();
  return (
    <span className="subagent-status">
      <span className={`subagent-dot ${agent.status}`} aria-hidden="true" />
      {t(STATUS_LABELS[agent.status])}
    </span>
  );
}

function Elapsed({ agent, now }: { agent: SubAgent; now: number }) {
  const t = useT();
  const ms = elapsedMs(agent, now);
  if (ms === null) return null;
  return (
    <span className="subagent-time" title={t("subAgents.elapsed")}>
      <span className="sr-only">{t("subAgents.elapsed")} </span>
      {formatElapsed(ms)}
    </span>
  );
}

interface AgentPaneProps {
  style: React.CSSProperties;
  agent: SubAgent;
  history: readonly string[];
  now: number;
  excerpts: boolean;
  picked: boolean;
  close: React.ReactNode;
  onKeyDown: (event: KeyboardEvent) => void;
}

function AgentPane({
  style,
  agent,
  history,
  now,
  excerpts,
  picked,
  close,
  onKeyDown,
}: AgentPaneProps) {
  const t = useT();
  const name = agentTitle(agent, t);
  const type = agent.description !== null ? agent.agentType : null;
  return (
    <section
      className={picked ? `subagent ${agent.status} picked` : `subagent ${agent.status}`}
      style={style}
      aria-label={t("subAgents.paneLabel", { name })}
      aria-current={picked ? "true" : undefined}
      onKeyDown={onKeyDown}
    >
      <header className="subagent-header">
        <h2 className="subagent-title" title={name}>
          {name}
        </h2>
        {type !== null && <span className="subagent-type">{type}</span>}
        <Status agent={agent} />
        <Elapsed agent={agent} now={now} />
        {close}
      </header>
      {/* Focusable, so a long excerpt can be scrolled by keyboard. */}
      <div className="subagent-body" tabIndex={0} aria-label={t("subAgents.bodyLabel", { name })}>
        <p className="subagent-tool">
          <span className="subagent-label">{t("subAgents.tool")}</span>
          <span className="subagent-tool-name">{agent.tool ?? t("subAgents.noTool")}</span>
        </p>
        {history.length > 0 && (
          <p className="subagent-history">
            <span className="subagent-label">{t("subAgents.history")}</span>
            <span className="subagent-history-names">{history.join(" · ")}</span>
          </p>
        )}
        <p className={agent.excerpt === null ? "subagent-excerpt empty" : "subagent-excerpt"}>
          {agent.excerpt ?? (excerpts ? t("subAgents.noExcerpt") : t("subAgents.excerptsOff"))}
        </p>
      </div>
    </section>
  );
}

interface AgentListProps {
  style: React.CSSProperties;
  agents: readonly SubAgent[];
  picked: string | null;
  now: number;
  close: React.ReactNode;
  onKeyDown: (event: KeyboardEvent) => void;
}

/** Every sub-agent in one compact list, newest first. */
function AgentList({ style, agents, picked, now, close, onKeyDown }: AgentListProps) {
  const t = useT();
  const heading = t("subAgents.all", { count: agents.length });
  const body = useRef<HTMLDivElement>(null);
  // The row picked in the sidebar comes into sight once; later snapshots do not scroll.
  useEffect(() => {
    if (picked === null) return;
    const rows = body.current?.querySelectorAll<HTMLElement>("[data-agent]") ?? [];
    for (const row of rows) {
      if (row.dataset.agent === picked) row.scrollIntoView({ block: "nearest" });
    }
  }, [picked]);
  return (
    <section className="subagent" style={style} aria-label={heading} onKeyDown={onKeyDown}>
      <header className="subagent-header">
        <h2 className="subagent-title">{heading}</h2>
        <span className="subagent-type">{countsText(agents, t)}</span>
        {close}
      </header>
      <div className="subagent-body" tabIndex={0} aria-label={heading} ref={body}>
        <ul className="subagent-list">
          {[...agents].reverse().map((agent) => {
            const name = agentTitle(agent, t);
            const isPicked = agent.id === picked;
            return (
              <li
                key={agent.id}
                className={isPicked ? "subagent-row picked" : "subagent-row"}
                data-agent={agent.id}
                aria-current={isPicked ? "true" : undefined}
              >
                <span className="subagent-row-name" title={name}>
                  {name}
                </span>
                {agent.tool !== null && <span className="subagent-row-tool">{agent.tool}</span>}
                <Status agent={agent} />
                <Elapsed agent={agent} now={now} />
              </li>
            );
          })}
        </ul>
      </div>
    </section>
  );
}

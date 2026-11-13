// The sub-agent view: one read-only pane beside the session's terminal. It shows the activity of
// the sub-agent picked in the sidebar's tree like a console (task, assistant text, tool calls
// with their key argument, short results), or the list of all sub-agents when none is picked.
// Every text from the transcript is rendered as plain React text, never as HTML, Markdown or a
// link.

import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";

import { useT, type Translate } from "./i18n";
import { ChevronIcon, CloseIcon } from "./icons";
import {
  activityText,
  agentTitle,
  countsText,
  elapsedMs,
  formatElapsed,
  formatTokens,
  STATUS_LABELS,
  summaryText,
  visibleAgents,
  type LogEntry,
  type LogLine,
  type SubAgent,
} from "./subAgents";
import { SUB_AGENT_LAYOUT } from "./paneRatios";
import type { Ratios } from "./snapLayouts";
import { paneStyle } from "./TerminalPane";
import { useClock } from "./useClock";
import { useSubAgentLog, useSubAgentView } from "./useSubAgents";

/** Running times count in seconds. */
const TICK_MS = 1000;
/** Closer to the end than this counts as "at the end": new lines keep it scrolled down. */
const STICK_PX = 24;

interface Props {
  /** Title of the session the sub-agents belong to. */
  title: string;
  /** The watch to read (useSubAgentWatch); its events re-render this pane only. */
  watchKey: string | null;
  /** Not located, none yet, unavailable or an error: a notice instead of agents. */
  showNotice: boolean;
  /** Text for the notice (not located, none yet, unavailable, error). */
  notice: string;
  /** Texts switched off in Settings: the log shows tool names only and says so. */
  excerpts: boolean;
  /** Sub-agent whose activity shows; `null`: the list of all of them. */
  picked: string | null;
  /** Sub-agents removed from the tree; the list leaves them out too. */
  hidden: ReadonlySet<string>;
  onPick: (agentId: string | null) => void;
  onClose: () => void;
  /** Splitter position between the session's terminal and this pane (paneRatios.ts). */
  ratios: Ratios;
}

export function SubAgentPane({
  title,
  watchKey,
  showNotice,
  notice,
  excerpts,
  picked,
  hidden,
  onPick,
  onClose,
  ratios,
}: Props) {
  const t = useT();
  const { view } = useSubAgentView(watchKey);
  const style = paneStyle(SUB_AGENT_LAYOUT, 1, ratios);
  /** A switch inside the pane by keyboard: the new body takes the focus once it is shown. */
  const [focusOnShow, setFocusOnShow] = useState(false);
  const focused = useCallback(() => {
    setFocusOnShow(false);
  }, []);

  // Escape anywhere in the pane closes it; the terminal then gets the keyboard back.
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

  if (showNotice) {
    return (
      <section
        className="subagent"
        style={style}
        aria-label={t("subAgents.region", { title })}
        onKeyDown={onKeyDown}
      >
        <header className="subagent-header">
          <h2 className="subagent-title">{t("subAgents.region", { title })}</h2>
          {closeButton}
        </header>
        <div className="subagent-body subagent-notice">
          <p role="status">{notice}</p>
        </div>
      </section>
    );
  }

  const agent = picked === null ? undefined : view.agents.find((a) => a.id === picked);
  if (picked === null) {
    return (
      <AgentList
        style={style}
        agents={visibleAgents(view.agents, hidden)}
        close={closeButton}
        focusOnShow={focusOnShow}
        onFocused={focused}
        onKeyDown={onKeyDown}
        onPick={(id, byKeyboard) => {
          setFocusOnShow(byKeyboard);
          onPick(id);
        }}
      />
    );
  }
  return (
    <AgentLog
      // Another sub-agent starts at the end of its log again.
      key={picked}
      style={style}
      agentId={picked}
      agent={agent}
      watchKey={watchKey}
      excerpts={excerpts}
      close={closeButton}
      focusOnShow={focusOnShow}
      onFocused={focused}
      onKeyDown={onKeyDown}
      onBack={(byKeyboard) => {
        setFocusOnShow(byKeyboard);
        onPick(null);
      }}
    />
  );
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

interface AgentLogProps {
  style: React.CSSProperties;
  agentId: string;
  /** `undefined`: no longer in the snapshot (its files are gone). */
  agent: SubAgent | undefined;
  watchKey: string | null;
  excerpts: boolean;
  close: ReactNode;
  /** Take the keyboard focus once shown (a switch by keyboard), then call `onFocused`. */
  focusOnShow: boolean;
  onFocused: () => void;
  onKeyDown: (event: KeyboardEvent) => void;
  onBack: (byKeyboard: boolean) => void;
}

/** Focuses `element` once `wanted` turns true, then reports it. */
function useFocusOnShow(
  element: React.RefObject<HTMLElement | null>,
  wanted: boolean,
  done: () => void,
) {
  useEffect(() => {
    if (!wanted) return;
    element.current?.focus();
    done();
  }, [element, wanted, done]);
}

const NO_LINES: readonly LogLine[] = [];

/** The sub-agent's activity as a console: newest at the bottom, following it while at the end. */
function AgentLog({
  style,
  agentId,
  agent,
  watchKey,
  excerpts,
  close,
  focusOnShow,
  onFocused,
  onKeyDown,
  onBack,
}: AgentLogProps) {
  const t = useT();
  const now = useClock(TICK_MS);
  const log = useSubAgentLog(watchKey);
  const lines = log.agentId === agentId ? log.lines : NO_LINES;
  /** The backend sent this sub-agent's log; before that, "no output yet" would not be true. */
  const loaded = log.agentId === agentId && log.loaded;
  const body = useRef<HTMLDivElement>(null);
  useFocusOnShow(body, focusOnShow, onFocused);
  const name = agent === undefined ? t("subAgents.untitled") : agentTitle(agent, t);
  const type = agent !== undefined && agent.description !== null ? agent.agentType : null;
  /** At (or near) the end: new lines scroll it down. Scrolling up stops that. */
  const stick = useRef(true);
  const [atEnd, setAtEnd] = useState(true);
  useLayoutEffect(() => {
    const element = body.current;
    if (element !== null && stick.current) element.scrollTop = element.scrollHeight;
  }, [lines]);
  const onScroll = () => {
    const element = body.current;
    if (element === null) return;
    const end = element.scrollHeight - element.scrollTop - element.clientHeight < STICK_PX;
    stick.current = end;
    setAtEnd(end);
  };
  const jumpToEnd = () => {
    const element = body.current;
    if (element === null) return;
    stick.current = true;
    setAtEnd(true);
    element.scrollTop = element.scrollHeight;
    element.focus();
  };

  return (
    <section
      className={agent === undefined ? "subagent" : `subagent ${agent.status}`}
      style={style}
      aria-label={t("subAgents.paneLabel", { name })}
      onKeyDown={onKeyDown}
    >
      <header className="subagent-header">
        <button
          type="button"
          className="icon-button subagent-back"
          aria-label={t("subAgents.showAll")}
          title={t("subAgents.showAll")}
          onClick={(event) => {
            onBack(event.detail === 0);
          }}
        >
          <ChevronIcon size={14} />
        </button>
        <h2 className="subagent-title" title={name}>
          {name}
        </h2>
        {type !== null && <span className="subagent-type">{type}</span>}
        {agent !== undefined && <Status agent={agent} />}
        {agent !== undefined && <Elapsed agent={agent} now={now} />}
        {close}
      </header>
      {/* Focusable, so the log can be scrolled by keyboard. */}
      <div
        className="subagent-log"
        tabIndex={0}
        aria-label={t("subAgents.bodyLabel", { name })}
        ref={body}
        onScroll={onScroll}
      >
        {agent === undefined && <p className="log-note">{t("subAgents.gone")}</p>}
        {!excerpts && <p className="log-note">{t("subAgents.logOff")}</p>}
        {loaded && lines.length === 0 && agent !== undefined && (
          <p className="log-note">{t("subAgents.logEmpty")}</p>
        )}
        {lines.map((line) => (
          <LogRow key={line.id} entry={line.entry} />
        ))}
      </div>
      {!atEnd && (
        <button type="button" className="subagent-jump" onClick={jumpToEnd}>
          {t("subAgents.jumpToEnd")}
        </button>
      )}
      {loaded && agent !== undefined && (
        <footer className="subagent-log-footer">{footerText(agent, log.toolUses, now, t)}</footer>
      )}
    </section>
  );
}

/** "Tool uses: 34 · Tokens: 96.3k · 2:41", as Claude Code ends a sub-agent's view. */
function footerText(agent: SubAgent, toolUses: number, now: number, t: Translate): string {
  const ms = elapsedMs(agent, now);
  return [
    t("subAgents.toolUses", { count: toolUses }),
    agent.tokens === null ? null : t("subAgents.tokens", { tokens: formatTokens(agent.tokens) }),
    ms === null ? null : formatElapsed(ms),
  ]
    .filter((part) => part !== null)
    .join(" · ");
}

/** One console line; memoized, since a batch only appends and older lines never change. */
const LogRow = memo(function LogRow({ entry }: { entry: LogEntry }) {
  const t = useT();
  switch (entry.kind) {
    case "prompt":
      return (
        <div className="log-line log-prompt">
          <span className="log-mark" aria-hidden="true">
            &gt;
          </span>
          <span className="log-text">
            <span className="log-label">{t("subAgents.task")}</span> {entry.text}
          </span>
        </div>
      );
    case "text":
      return (
        <div className="log-line log-say">
          <span className="log-mark" aria-hidden="true">
            {"●"}
          </span>
          <span className="log-text">{entry.text}</span>
        </div>
      );
    case "tool":
      return (
        <div className="log-line log-tool">
          <span className="log-mark" aria-hidden="true">
            {"●"}
          </span>
          <span className="log-text">
            <span className="log-tool-name">{entry.name}</span>
            {entry.detail !== null && <span className="log-detail">({entry.detail})</span>}
          </span>
        </div>
      );
    case "result": {
      // Like Claude Code: a summary for tools that have one, else the first lines of the output.
      const shown =
        entry.summary !== null
          ? summaryText(entry.summary, t)
          : entry.lines.length > 0
            ? entry.lines.join("\n")
            : entry.error
              ? ""
              : t("subAgents.noOutput");
      return (
        <div className={entry.error ? "log-line log-result failed" : "log-line log-result"}>
          <span className="log-mark" aria-hidden="true">
            {"⎿"}
          </span>
          <span className="log-text">
            {entry.error && <span className="log-label">{t("subAgents.failed")} </span>}
            {shown}
            {entry.moreLines > 0 && (
              <span className="log-more">
                {"\n"}
                {t("subAgents.moreLines", { count: entry.moreLines })}
              </span>
            )}
          </span>
        </div>
      );
    }
    case "gap":
      return <p className="log-gap">{t("subAgents.gap")}</p>;
  }
});

interface AgentListProps {
  style: React.CSSProperties;
  agents: readonly SubAgent[];
  close: ReactNode;
  focusOnShow: boolean;
  onFocused: () => void;
  onKeyDown: (event: KeyboardEvent) => void;
  onPick: (id: string, byKeyboard: boolean) => void;
}

/** Every sub-agent in one compact list, newest first; a row shows its activity. */
function AgentList({
  style,
  agents,
  close,
  focusOnShow,
  onFocused,
  onKeyDown,
  onPick,
}: AgentListProps) {
  const t = useT();
  const now = useClock(TICK_MS);
  const body = useRef<HTMLDivElement>(null);
  // Back from a log by keyboard: on the first row, so Enter opens a log again.
  useEffect(() => {
    if (!focusOnShow) return;
    const element = body.current;
    (element?.querySelector<HTMLElement>(".subagent-row") ?? element)?.focus();
    onFocused();
  }, [focusOnShow, onFocused]);
  const heading = t("subAgents.all", { count: agents.length });
  const hint = t("subAgents.treeHint");
  return (
    <section className="subagent" style={style} aria-label={heading} onKeyDown={onKeyDown}>
      <header className="subagent-header">
        <h2 className="subagent-title">{heading}</h2>
        <span className="subagent-type">{countsText(agents, t)}</span>
        {close}
      </header>
      <div className="subagent-body" tabIndex={-1} aria-label={heading} ref={body}>
        {agents.length === 0 ? (
          <p className="log-note">{t("subAgents.none")}</p>
        ) : (
          <ul className="subagent-list">
            {[...agents].reverse().map((agent) => {
              const name = agentTitle(agent, t);
              return (
                <li key={agent.id}>
                  <button
                    type="button"
                    className="subagent-row"
                    title={`${name}\n${hint}`}
                    onClick={(event) => {
                      onPick(agent.id, event.detail === 0);
                    }}
                  >
                    <span className="subagent-row-name">{name}</span>
                    {agent.status !== "finished" && agent.tool !== null && (
                      <span className="subagent-row-tool">{activityText(agent, t)}</span>
                    )}
                    <Status agent={agent} />
                    <Elapsed agent={agent} now={now} />
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </section>
  );
}

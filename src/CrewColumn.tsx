// The crew beside a session's terminal (docs/wiki/pages/syntheses/crew-column.md): each running
// sub-agent as a still whole figure on a round stage whose light ring shows its state, with a
// speech bubble for what it does and its name on a tag below. In a narrow pane the same crew is a
// strip of faces with one line of text.
// Which sub-agents stand here, their states and texts are decided by crew.ts and crewText.ts.

import { useState } from "react";

import { AgentFigure, useCrewFigure } from "./AgentFigure";
import { BubbleThrottle, crewOf, type CrewEntry, type CrewForm, type CrewState } from "./crew";
import { bubbleText, bubbleTyping } from "./crewText";
import { useT, type MessageKey } from "./i18n";
import { CheckIcon } from "./icons";
import { agentName, visibleAgents, type SubAgent } from "./subAgents";
import { useClock } from "./useClock";
import { useSubAgentView } from "./useSubAgents";

/** Nobody waits. */
const NOBODY: ReadonlySet<string> = new Set();

/** The bubbles and the linger of finished figures move on with this clock. */
const TICK_MS = 500;

const STATE_LABELS: Record<CrewState, MessageKey> = {
  working: "crew.state.working",
  waiting: "crew.state.waiting",
  done: "crew.state.done",
  quiet: "crew.state.quiet",
};

interface Props {
  form: CrewForm;
  sessionId: number;
  sessionTitle: string;
  /** The session's folder: which definitions, and so which figures, its sub-agents have. */
  cwd: string | null;
  /** The watch to read (useSubAgentWatch). */
  watchKey: string | null;
  /** Sub-agents the user removed from the sidebar's tree. */
  hidden: ReadonlySet<string>;
  /** The session waits for a decision of the user (a pending gate decision). */
  sessionWaits: boolean;
  /** Sub-agents its pending gate decisions name (crew.ts `waitingSubAgents`); missing: none. */
  waiting?: ReadonlySet<string>;
  /** Sub-agent shown in the open view. */
  picked: string | null;
  onShow: (sessionId: number, agentId: string | null, byKeyboard: boolean) => void;
  /** Switches the crew off (Settings brings it back). */
  onHide: () => void;
}

/** Nothing to show without sub-agents: then no clock runs either. */
export function CrewColumn(props: Props) {
  const agents = visibleAgents(useSubAgentView(props.watchKey).view.agents, props.hidden);
  if (agents.length === 0) return null;
  return <CrewBody {...props} agents={agents} />;
}

function CrewBody({
  form,
  sessionId,
  sessionTitle,
  cwd,
  agents,
  sessionWaits,
  waiting = NOBODY,
  picked,
  onShow,
  onHide,
}: Props & { agents: readonly SubAgent[] }) {
  const t = useT();
  const now = useClock(TICK_MS);
  const [throttle] = useState(() => new BubbleThrottle());
  const crew = crewOf(agents, now, sessionWaits ? waiting : NOBODY);
  const wanted = new Map(
    crew.entries.map(({ agent, state }) => [
      agent.id,
      { text: bubbleText(agent, state, t), state },
    ]),
  );
  const texts = throttle.update(wanted, now);
  if (crew.entries.length === 0) return null;
  const hideLabel = t("crew.hide");
  return (
    <aside className={`crew crew-${form}`} aria-label={t("crew.region", { title: sessionTitle })}>
      <ul className="crew-list">
        {crew.entries.map((entry) => (
          <CrewItem
            key={entry.agent.id}
            entry={entry}
            form={form}
            cwd={cwd}
            text={texts.get(entry.agent.id) ?? wanted.get(entry.agent.id)?.text ?? ""}
            picked={entry.agent.id === picked}
            onShow={(byKeyboard) => {
              onShow(sessionId, entry.agent.id, byKeyboard);
            }}
          />
        ))}
      </ul>
      {crew.more > 0 && (
        <button
          type="button"
          className="crew-more"
          title={t("subAgents.showAll")}
          onClick={(event) => {
            onShow(sessionId, null, event.detail === 0);
          }}
        >
          {t("crew.more", { count: crew.more })}
        </button>
      )}
      <button
        type="button"
        className="crew-hide"
        title={t("crew.hideHelp")}
        // The terminal keeps the keyboard; the button is for the mouse.
        onMouseDown={(event) => {
          event.preventDefault();
        }}
        onClick={onHide}
      >
        {hideLabel}
      </button>
    </aside>
  );
}

function CrewItem({
  entry,
  form,
  cwd,
  text,
  picked,
  onShow,
}: {
  entry: CrewEntry;
  form: CrewForm;
  cwd: string | null;
  text: string;
  picked: boolean;
  onShow: (byKeyboard: boolean) => void;
}) {
  const t = useT();
  const { agent, state } = entry;
  const figure = useCrewFigure(agent.agentType, cwd);
  const name = agentName(agent, t);
  const stateLabel = t(STATE_LABELS[state]);
  const typing = bubbleTyping(state, text, t);
  return (
    <li className={`crew-item state-${state}${picked ? " picked" : ""}`} data-agent={agent.id}>
      <button
        type="button"
        className="crew-button"
        aria-label={`${name}, ${stateLabel}, ${text}`}
        aria-current={picked ? "true" : undefined}
        onClick={(event) => {
          onShow(event.detail === 0);
        }}
      >
        {form === "column" ? (
          <>
            <span className="crew-bubble" aria-hidden="true">
              <CrewSay text={text} state={state} typing={typing} className="crew-bubble-text" />
            </span>
            <AgentFigure figure={figure} size={88} crop="full" />
            {/* The stage's light ring alone tells the state; the word stays in the label. */}
            <span className="crew-stage" aria-hidden="true" />
            <span className="crew-name" aria-hidden="true">
              {name}
            </span>
          </>
        ) : (
          <>
            <AgentFigure figure={figure} size={30} crop="head" />
            <span className="crew-chip-text" aria-hidden="true">
              <span className="crew-chip-name">{name}</span>{" "}
              <CrewSay text={text} state={state} typing={typing} className="crew-chip-say" />
            </span>
          </>
        )}
      </button>
    </li>
  );
}

/**
 * What a figure says: typing dots while it has not used a tool yet, else the text, keyed by it so
 * that a new text fades in. A finished figure gets a small check before its word.
 */
function CrewSay({
  text,
  state,
  typing,
  className,
}: {
  text: string;
  state: CrewState;
  typing: boolean;
  className: string;
}) {
  if (typing) {
    return (
      <span className={`${className} crew-typing`}>
        <span />
        <span />
        <span />
      </span>
    );
  }
  return (
    <span key={text} className={`${className} crew-say`}>
      {state === "done" && <CheckIcon size={12} />}
      {text}
    </span>
  );
}

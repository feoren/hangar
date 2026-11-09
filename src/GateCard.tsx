import { useEffect, useId, useRef } from "react";

import { decisionText, type GateAnswer, type PendingDecision } from "./gate";
import { useT } from "./i18n";
import { OverlapIcon } from "./icons";

/**
 * A pending decision of the gate: which session wants to change which file, who else is changing
 * it, and the three answers. It sits at the top of the waiting session's pane, so the input line
 * at the bottom stays visible; for a session not on screen it floats at the top of the session
 * area and offers to show that session. It never takes the focus by itself (the person may be
 * typing elsewhere); Ctrl+Shift+A brings it there. Escape returns to the terminal and answers
 * nothing: an unanswered decision is denied by the backend after 10 minutes.
 */
export function GateCard(props: {
  decision: PendingDecision;
  /** The waiting session is not on screen: the card names it and offers to show it. */
  floating: boolean;
  /** Further pending decisions, of any session. */
  more: number;
  error: string | null;
  nameOf: (id: number) => string;
  onAnswer: (answer: GateAnswer) => void;
  onShowSession: (id: number) => void;
  onEscape: () => void;
  /** Set (above 0) when the card should take the focus (Ctrl+Shift+A); `onFocused` resets it. */
  focusToken: number;
  onFocused: () => void;
}) {
  const { decision } = props;
  const t = useT();
  const id = useId();
  const card = useRef<HTMLElement>(null);
  const { focusToken, onFocused } = props;
  // Runs after the panes' own effects, which focus the terminal of a newly active session.
  useEffect(() => {
    if (focusToken === 0) return;
    card.current?.focus();
    onFocused();
  }, [focusToken, onFocused]);
  const text = decisionText(decision, props.nameOf, t);
  const other = decision.otherSessions[0];
  const answer = (value: GateAnswer) => () => {
    props.onAnswer(value);
  };
  return (
    <section
      ref={card}
      className={props.floating ? "gate-card floating" : "gate-card"}
      aria-label={t("gate.label")}
      aria-describedby={`${id}-title ${id}-detail`}
      tabIndex={-1}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        event.stopPropagation();
        props.onEscape();
      }}
    >
      <p className="gate-title selectable" id={`${id}-title`}>
        <OverlapIcon size={14} />
        <span>{text.title}</span>
      </p>
      <p className="gate-detail selectable" id={`${id}-detail`}>
        {text.detail}
      </p>
      <div className="gate-actions">
        <button type="button" className="secondary-button" onClick={answer("allowOnce")}>
          {t("gate.allowOnce")}
        </button>
        <button type="button" className="secondary-button" onClick={answer("allowFile")}>
          {t("gate.allowFile")}
        </button>
        <button type="button" className="secondary-button" onClick={answer("deny")}>
          {t("gate.deny")}
        </button>
      </div>
      {props.error !== null && (
        <p className="gate-error" role="alert">
          {props.error}
        </p>
      )}
      <div className="gate-foot">
        {props.floating && (
          <button
            type="button"
            className="link-button"
            onClick={() => {
              props.onShowSession(decision.sessionId);
            }}
          >
            {t("claims.showSession")}
          </button>
        )}
        {other !== undefined && (
          <button
            type="button"
            className="link-button"
            onClick={() => {
              props.onShowSession(other);
            }}
          >
            {t("gate.showOther")}
          </button>
        )}
        <span className="gate-note">
          {props.more > 0 ? t("gate.more", { count: props.more }) : t("gate.timeout")}
        </span>
      </div>
    </section>
  );
}

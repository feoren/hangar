import { useEffect, useId, useRef, useState } from "react";

import {
  decisionText,
  ruleAnswerText,
  ruleFolder,
  rulePathLabel,
  ruleText,
  type GateAnswer,
  type PendingDecision,
  type RuleAnswer,
} from "./gate";
import { useT } from "./i18n";
import { OverlapIcon } from "./icons";
import { Revealed } from "./Revealed";
import { useArmDelay } from "./useArmDelay";

/**
 * A pending decision of the gate: which session wants to change which file, who else is changing
 * it, and the three answers. It sits at the top of the waiting session's pane, so the input line
 * at the bottom stays visible; for a session not on screen it floats at the top of the session
 * area and offers to show that session. It never takes the focus by itself (the person may be
 * typing elsewhere); Ctrl+Shift+A brings it there. Escape returns to the terminal and answers
 * nothing: an unanswered decision is denied by the backend after 10 minutes.
 *
 * "Remember for this project" opens two more rows, "always" and "never", for the file or its
 * folder; the backend keeps that answer as a rule of the project and answers later asks itself.
 *
 * A shell command the command guard holds shows the matched rules, the program call that matched
 * and the command as sent, with control and invisible characters as marked placeholders (text the
 * agent typed cannot pose as one). "Allow once" and "Block"; "Remember for this project" names the
 * rules again and offers to always allow or always block them in the project, except for Hangar's
 * own settings. Without an answer the backend blocks it after 10 minutes.
 *
 * The answers take no click for a moment after the card appears or its decision changes
 * (useArmDelay.ts): a click meant for what was under the mouse before must not answer it.
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
  const [remembering, setRemembering] = useState(false);
  const text = decisionText(decision, props.nameOf, t);
  const other = decision.otherSessions[0];
  const { armed, guard } = useArmDelay(decision.id);
  /** Props of an answer button: its click, ignored while the card is not armed yet. */
  const answer = (value: GateAnswer) => ({
    onClick: guard(() => {
      props.onAnswer(value);
    }),
    "aria-disabled": armed ? undefined : true,
  });
  const command = decision.command;
  const remember = command === null || command.rememberable;
  const folder = command === null ? ruleFolder(decision.path) : null;
  const ruleRow = (label: string, file: GateAnswer, inFolder: GateAnswer) => (
    <div className="gate-rule-row">
      <span className="gate-rule-label">{label}</span>
      <button type="button" className="secondary-button" {...answer(file)}>
        {t("gate.ruleFile")}
      </button>
      {folder !== null && (
        <button type="button" className="secondary-button" {...answer(inFolder)}>
          {t("gate.ruleFolder", { folder: rulePathLabel(folder, "folder", t) })}
        </button>
      )}
    </div>
  );
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
      {command !== null && command.call !== "" && command.call !== command.text && (
        <p className="gate-command-call selectable">
          <Revealed text={command.call} />
        </p>
      )}
      {command !== null && (
        // Shown as sent, line by line; invisible characters become marked placeholders.
        <pre className="gate-command selectable" aria-label={t("gate.commandLabel")}>
          {command.text.split("\n").map((line, index) => (
            // Lines never move; the index is stable for this text.
            <span key={index}>
              {index > 0 && "\n"}
              <Revealed text={line} />
            </span>
          ))}
        </pre>
      )}
      {command !== null && command.hidden > 0 && (
        <p className="gate-command-more">{t("gate.commandHidden", { count: command.hidden })}</p>
      )}
      <div className="gate-actions">
        <button type="button" className="secondary-button" {...answer("allowOnce")}>
          {t("gate.allowOnce")}
        </button>
        {command === null && (
          <button type="button" className="secondary-button" {...answer("allowFile")}>
            {t("gate.allowFile")}
          </button>
        )}
        <button type="button" className="secondary-button" {...answer("deny")}>
          {t(command === null ? "gate.deny" : "gate.block")}
        </button>
      </div>
      {remembering && command === null && (
        <div className="gate-rules" id={`${id}-rules`} role="group" aria-label={t("gate.remember")}>
          {ruleRow(t("gate.always"), "alwaysFile", "alwaysFolder")}
          {ruleRow(t("gate.never"), "neverFile", "neverFolder")}
        </div>
      )}
      {remembering && command !== null && command.rememberable && (
        <div className="gate-rules" id={`${id}-rules`} role="group" aria-label={t("gate.remember")}>
          <p className="gate-rule-applies">
            {t("gate.commandAppliesTo", {
              rules: command.rules.map((rule) => ruleText(rule, t)).join(" · "),
            })}
          </p>
          <div className="gate-rule-row">
            <button type="button" className="secondary-button" {...answer("alwaysCommand")}>
              {t("gate.alwaysCommand")}
            </button>
            <button type="button" className="secondary-button" {...answer("neverCommand")}>
              {t("gate.neverCommand")}
            </button>
          </div>
        </div>
      )}
      {props.error !== null && (
        <p className="gate-error" role="alert">
          {props.error}
        </p>
      )}
      <div className="gate-foot">
        {remember && (
          <button
            type="button"
            className="link-button"
            aria-expanded={remembering}
            aria-controls={remembering ? `${id}-rules` : undefined}
            onClick={() => {
              setRemembering((open) => !open);
            }}
          >
            {t("gate.remember")}
          </button>
        )}
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
          {props.more > 0
            ? t("gate.more", { count: props.more })
            : t(command === null ? "gate.timeout" : "gate.commandTimeout")}
        </span>
      </div>
    </section>
  );
}

/**
 * The short note on a session's pane after a remembered answer replied for it (useRuleNotes.ts):
 * one quiet line at the top, without buttons, gone after a few seconds. Nothing without `entry`.
 */
export function GateRuleNote(props: { entry: RuleAnswer | undefined }) {
  const t = useT();
  if (props.entry === undefined) return null;
  return (
    <p className="gate-rule-note" role="status">
      {ruleAnswerText(props.entry, t)}
    </p>
  );
}

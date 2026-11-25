import { Fragment, useEffect, useId, useRef, useState, type SyntheticEvent } from "react";

import { AutoGrowTextarea } from "./AutoGrowTextarea";
import { useT } from "./i18n";
import { Revealed } from "./Revealed";
import { TeamCard, TeamCardEmpty } from "./TeamCard";
import {
  decisionDate,
  decisionText,
  personTag,
  teamErrorText,
  type DecisionView,
  type TeamActivity,
  type TeamProblem,
} from "./team";
import { decisionKey, team } from "./useTeam";

/** Longest decision and reason the team takes (Rust: `team_decisions.rs`). */
export const MAX_DECISION_CHARS = 300;
export const MAX_REASON_CHARS = 1000;

/** The running session a decision can be pasted into, when its agent does not read them itself. */
export interface DecisionTarget {
  id: number;
  title: string;
}

/**
 * "Decisions" of a connected shared project: one "Record decision" entry with two fields, the
 * team's decisions newest first, and one Withdraw action. Agents that take the team's context get
 * them unasked; for any other running session "Send to session" pastes one in, as notes do.
 * Shown only where the team server keeps decisions; an older one gets a quiet line for owners and
 * admins, who can update it. While this device holds no notes key of the team, one calm line says
 * when decisions start instead of a button that cannot work.
 */
export function TeamDecisionsView(props: {
  projectId: string;
  activity: TeamActivity;
  busy: ReadonlySet<string>;
  /** The person is the project's owner or an admin. */
  manages: boolean;
  /** Where "Send to session" pastes; `null` hides it. */
  target: DecisionTarget | null;
  onSend: (text: string, sessionId: number) => void;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [problem, setProblem] = useState<TeamProblem | null>(null);
  const recordButton = useRef<HTMLButtonElement>(null);
  const wasOpen = useRef(false);
  const { decisions, decisionsState } = props.activity;

  useEffect(() => {
    if (wasOpen.current && !open) recordButton.current?.focus();
    wasOpen.current = open;
  }, [open]);

  if (decisionsState === "unsupported") {
    return props.manages ? (
      <p className="team-help team-decisions-server" role="note">
        {t("team.decisions.needsServer")}
      </p>
    ) : null;
  }
  if (decisionsState === "waitingForKeys") {
    return (
      <p className="team-help team-decisions-server" role="note">
        {t("team.decisions.waitingForKeys")}
      </p>
    );
  }
  if (decisionsState !== "ready") return null;

  const { target, onSend } = props;
  return (
    <TeamCard
      className="team-decisions"
      title={t("team.decisions.title")}
      count={decisions.length}
      action={
        open ? undefined : (
          <button
            ref={recordButton}
            type="button"
            className="secondary-button team-task-reserve"
            onClick={() => {
              setProblem(null);
              setOpen(true);
            }}
          >
            {t("team.decisions.record")}
          </button>
        )
      }
    >
      {open && (
        <RecordForm
          projectId={props.projectId}
          busy={props.busy.has(decisionKey(props.projectId, "new"))}
          onDone={() => {
            setOpen(false);
          }}
        />
      )}
      {decisions.length === 0 ? (
        open ? null : (
          <TeamCardEmpty>{t("team.decisions.none")}</TeamCardEmpty>
        )
      ) : (
        <ul className="team-task-list">
          {decisions.map((decision) => (
            <Decision
              key={decision.blob}
              decision={decision}
              busy={props.busy.has(decisionKey(props.projectId, decision.blob))}
              target={target}
              onWithdraw={() => {
                setProblem(null);
                team
                  .withdrawDecision(props.projectId, decision.blob)
                  .then(setProblem)
                  .catch(() => undefined);
              }}
              onSend={(sessionId) => {
                onSend(decisionText(decision), sessionId);
              }}
            />
          ))}
        </ul>
      )}
      {problem !== null && (
        <p className="team-help error-text" role="alert">
          {teamErrorText(problem, t)}
        </p>
      )}
    </TeamCard>
  );
}

function RecordForm(props: { projectId: string; busy: boolean; onDone: () => void }) {
  const t = useT();
  const ids = useId();
  const [decision, setDecision] = useState("");
  const [reason, setReason] = useState("");
  const [problem, setProblem] = useState<TeamProblem | null>(null);
  const decisionRef = useRef<HTMLInputElement>(null);

  const submit = (event: SyntheticEvent) => {
    event.preventDefault();
    if (decision.trim() === "") {
      decisionRef.current?.focus();
      return;
    }
    setProblem(null);
    team
      .recordDecision(props.projectId, decision.trim(), reason.trim())
      .then((next) => {
        // A refusal keeps both fields as typed, so nothing has to be written again.
        if (next === null) props.onDone();
        else setProblem(next);
      })
      .catch(() => undefined);
  };

  const closeOnEscape = (event: React.KeyboardEvent) => {
    if (event.key !== "Escape") return;
    // Escape closes the form only, not the Team window around it.
    event.stopPropagation();
    props.onDone();
  };

  return (
    <form className="team-task-form" onSubmit={submit}>
      <label className="team-task-field" htmlFor={`${ids}-decision`}>
        {t("team.decisions.whatLabel")}
      </label>
      <input
        ref={decisionRef}
        id={`${ids}-decision`}
        className="text-input"
        value={decision}
        maxLength={MAX_DECISION_CHARS}
        aria-describedby={`${ids}-help`}
        // The form opens because the person asked for it: the first field takes the keyboard.
        autoFocus
        onChange={(event) => {
          setDecision(event.currentTarget.value);
        }}
        onKeyDown={closeOnEscape}
      />
      <label className="team-task-field" htmlFor={`${ids}-reason`}>
        {t("team.decisions.whyLabel")}
      </label>
      <AutoGrowTextarea
        id={`${ids}-reason`}
        className="text-input team-decision-reason"
        value={reason}
        rows={2}
        maxLength={MAX_REASON_CHARS}
        onChange={(event) => {
          setReason(event.currentTarget.value);
        }}
        onKeyDown={closeOnEscape}
      />
      <p id={`${ids}-help`} className="team-help">
        {t("team.decisions.help")}
      </p>
      <div className="team-confirm-actions">
        <button type="submit" className="primary-button" disabled={props.busy}>
          {t("team.decisions.confirm")}
        </button>
        <button type="button" className="secondary-button" onClick={props.onDone}>
          {t("common.cancel")}
        </button>
      </div>
      {problem !== null && (
        <p className="team-help error-text" role="alert">
          {teamErrorText(problem, t)}
        </p>
      )}
    </form>
  );
}

function Decision(props: {
  decision: DecisionView;
  busy: boolean;
  target: DecisionTarget | null;
  onWithdraw: () => void;
  onSend: (sessionId: number) => void;
}) {
  const t = useT();
  const { decision, target } = props;
  const [asking, setAsking] = useState(false);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const withdrawButton = useRef<HTMLButtonElement>(null);
  const wasAsking = useRef(false);
  const who = decision.mine
    ? t("team.tasks.you")
    : personTag({ name: decision.name, member: decision.member }, t);
  const meta = [who];
  if (decision.former) meta.push(t("team.decisions.former"));
  meta.push(decisionDate(decision.at));

  // The question takes the focus on its safe answer; once it closes, back to Withdraw.
  useEffect(() => {
    if (asking) cancelButton.current?.focus();
    else if (wasAsking.current) withdrawButton.current?.focus();
    wasAsking.current = asking;
  }, [asking]);

  return (
    <li className={decision.former ? "team-task team-decision former" : "team-task team-decision"}>
      <div className="team-task-head">
        <span className="team-decision-text">
          <Revealed text={decision.decision} />
        </span>
        {target !== null && (
          <button
            type="button"
            className="link-button"
            title={t("team.decisions.sendTitle", { session: target.title })}
            onClick={() => {
              props.onSend(target.id);
            }}
          >
            {t("team.decisions.send")}
          </button>
        )}
        {decision.canWithdraw && !asking && (
          <button
            ref={withdrawButton}
            type="button"
            className="link-button"
            disabled={props.busy}
            onClick={() => {
              setAsking(true);
            }}
          >
            {t("team.decisions.withdraw")}
          </button>
        )}
      </div>
      {decision.reason !== "" && (
        <div className="team-decision-reason-text">
          {/* A reason may hold line breaks (and only those): each line on its own, so no line
              break shows as a mark while every other invisible character still does. */}
          {decision.reason.split("\n").map((line, index) => (
            // Lines never move; the index is stable for this text.
            <Fragment key={index}>
              {index > 0 && <br />}
              <Revealed text={line} />
            </Fragment>
          ))}
        </div>
      )}
      <div className="team-task-meta" title={decision.member}>
        {meta.join(" · ")}
      </div>
      {asking && (
        <div
          className="team-confirm-ask"
          role="group"
          aria-label={t("team.decisions.withdraw")}
          onKeyDown={(event) => {
            if (event.key !== "Escape") return;
            // Escape answers the question only, not the Team window around it.
            event.stopPropagation();
            setAsking(false);
          }}
        >
          <p className="team-help">{t("team.decisions.withdrawConfirm")}</p>
          <div className="team-confirm-actions">
            <button
              type="button"
              className="primary-button danger"
              disabled={props.busy}
              onClick={() => {
                setAsking(false);
                props.onWithdraw();
              }}
            >
              {t("team.decisions.withdraw")}
            </button>
            <button
              ref={cancelButton}
              type="button"
              className="secondary-button"
              onClick={() => {
                setAsking(false);
              }}
            >
              {t("common.cancel")}
            </button>
          </div>
        </div>
      )}
    </li>
  );
}

import { useId, useState } from "react";

import { Dialog } from "./Dialog";
import { shortPath } from "./format";
import { useT, type MessageKey } from "./i18n";
import { CloseIcon } from "./icons";
import {
  MAX_LINK_MEMBERS,
  leadOf,
  linkColor,
  nameProblem,
  type LinkGroup,
  type NameProblem,
} from "./links";
import type { BadgeLook } from "./programPrefs";
import { SessionLook } from "./SessionLook";
import type { SessionView } from "./sessions";

const PROBLEM_TEXT: Record<NameProblem, MessageKey> = {
  invalid: "links.nameInvalid",
  reserved: "links.nameReserved",
  twice: "links.nameTwice",
};

function DialogHead({ id, title, onClose }: { id: string; title: string; onClose: () => void }) {
  const t = useT();
  return (
    <div className="dialog-header">
      <h2 id={id}>{title}</h2>
      <button
        type="button"
        className="icon-button"
        aria-label={t("common.close")}
        title={t("common.close")}
        onClick={onClose}
      >
        <CloseIcon />
      </button>
    </div>
  );
}

/**
 * The optional lead of a group: "No lead" or one of `options`. Only offered from two members on.
 */
function LeadChoice(props: {
  options: readonly { session: number; label: string }[];
  lead: number | null;
  onChange: (lead: number | null) => void;
}) {
  const t = useT();
  return (
    <div className="link-lead">
      <label className="link-lead-field">
        <span>{t("links.lead")}</span>
        <select
          className="text-input"
          value={props.lead === null ? "" : String(props.lead)}
          onChange={(event) => {
            const value = event.currentTarget.value;
            props.onChange(value === "" ? null : Number(value));
          }}
        >
          <option value="">{t("links.noLead")}</option>
          {props.options.map((option) => (
            <option key={option.session} value={String(option.session)}>
              {option.label}
            </option>
          ))}
        </select>
      </label>
      <p className="muted link-lead-help">{t("links.leadHelp")}</p>
    </div>
  );
}

/**
 * "Link with …": a check box per session that can be linked, the session the menu was opened on
 * checked first, optionally a lead, one button. The agents then message each other on their own
 * (ADR 0014).
 */
export function LinkDialog(props: {
  /** Sessions that can be linked (links.ts `linkCandidates`), in sidebar order. */
  candidates: readonly SessionView[];
  /** Checked at the start. */
  initial: readonly number[];
  badgeFor: (profileId: string) => BadgeLook;
  /** The program's name for a profile. */
  programOf: (profileId: string) => string;
  /** Sessions whose agent runs without asking before tools (links.ts `LinkState.unattended`). */
  unattended: ReadonlySet<number>;
  /** The key that adds a row to the sidebar's selection, for the tip ("Ctrl" or "⌘"). */
  selectKey: string;
  /** Links and closes, or answers why not; the dialog then stays open and shows it. */
  onLink: (ids: number[], lead: number | null) => Promise<string | null>;
  onClose: () => void;
}) {
  const t = useT();
  const id = useId();
  const [chosen, setChosen] = useState<ReadonlySet<number>>(() => new Set(props.initial));
  const [lead, setLead] = useState<number | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [linking, setLinking] = useState(false);
  const picked = props.candidates.filter((session) => chosen.has(session.id));
  const ids = picked.map((s) => s.id);
  // A lead that was unchecked again leads nothing.
  const chosenLead = lead !== null && ids.includes(lead) ? lead : null;
  const ready = ids.length >= 2 && ids.length <= MAX_LINK_MEMBERS && !linking;

  const link = () => {
    if (!ready) return;
    setLinking(true);
    setProblem(null);
    void props.onLink(ids, chosenLead).then((answer) => {
      setLinking(false);
      setProblem(answer);
    });
  };

  return (
    <Dialog labelledBy={`${id}-title`} className="link-dialog" onClose={props.onClose}>
      <DialogHead id={`${id}-title`} title={t("links.title")} onClose={props.onClose} />
      <p className="muted link-help">{t("links.help")}</p>
      {props.candidates.length < 2 ? (
        <p className="muted">{t("links.none")}</p>
      ) : (
        <ul className="link-list">
          {props.candidates.map((session) => (
            <li key={session.id}>
              <label className="check link-session" title={session.cwd}>
                <input
                  type="checkbox"
                  checked={chosen.has(session.id)}
                  data-autofocus={session.id === props.initial[0] ? "" : undefined}
                  onChange={(event) => {
                    const on = event.currentTarget.checked;
                    setProblem(null);
                    setChosen((current) => {
                      const next = new Set(current);
                      if (on) next.add(session.id);
                      else next.delete(session.id);
                      return next;
                    });
                  }}
                />
                <SessionLook
                  look={props.badgeFor(session.profileId)}
                  session={session}
                  crop="full"
                />
                <span className="link-session-text">
                  <span className="link-session-title">{session.title}</span>
                  <span className="link-session-facts">
                    {[
                      props.programOf(session.profileId),
                      props.unattended.has(session.id) ? t("links.unattended") : null,
                      shortPath(session.cwd),
                    ]
                      .filter((part): part is string => part !== null)
                      .join(" · ")}
                  </span>
                </span>
              </label>
            </li>
          ))}
        </ul>
      )}
      {ids.length >= 2 && (
        <LeadChoice
          options={picked.map((session) => ({ session: session.id, label: session.title }))}
          lead={chosenLead}
          onChange={setLead}
        />
      )}
      {ids.some((chosenId) => props.unattended.has(chosenId)) && (
        <p className="link-note">{t("links.unattendedNote")}</p>
      )}
      <p className="muted link-tip">{t("links.tip", { key: props.selectKey })}</p>
      {problem !== null && (
        <p className="link-problem" role="alert">
          {problem}
        </p>
      )}
      <div className="dialog-actions">
        <button type="button" className="secondary-button" onClick={props.onClose}>
          {t("common.cancel")}
        </button>
        <button type="button" className="primary-button" disabled={!ready} onClick={link}>
          {t("links.link")}
        </button>
      </div>
    </Dialog>
  );
}

/**
 * "Edit group": each member's name inside the group, which its agent writes to, the lead and the
 * toggle "Ask before pasting". Names are checked as the backend checks them before Save is
 * offered.
 */
export function LinkGroupDialog(props: {
  group: LinkGroup;
  titleOf: (session: number) => string;
  /** Saves and closes, or answers why not; the dialog then stays open and shows it. */
  onSave: (
    names: { session: number; name: string }[],
    askFirst: boolean,
    lead: number | null,
  ) => Promise<string | null>;
  onClose: () => void;
}) {
  const t = useT();
  const id = useId();
  const [names, setNames] = useState<readonly string[]>(() =>
    props.group.members.map((member) => member.name),
  );
  const [askFirst, setAskFirst] = useState(props.group.askFirst);
  const [lead, setLead] = useState<number | null>(() => leadOf(props.group));
  const [problem, setProblem] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const local = nameProblem(names);
  const ready = local === null && !saving;

  const save = () => {
    if (!ready) return;
    setSaving(true);
    setProblem(null);
    const edited = props.group.members.map((member, index) => ({
      session: member.session,
      name: (names[index] ?? member.name).trim(),
    }));
    void props.onSave(edited, askFirst, lead).then((answer) => {
      setSaving(false);
      setProblem(answer);
    });
  };

  const shown = problem ?? (local === null ? null : t(PROBLEM_TEXT[local]));
  return (
    <Dialog labelledBy={`${id}-title`} className="link-dialog" onClose={props.onClose}>
      <DialogHead id={`${id}-title`} title={t("links.editTitle")} onClose={props.onClose} />
      <p className="muted link-help">{t("links.namesHelp")}</p>
      <ul className="link-names">
        {props.group.members.map((member, index) => {
          const title = props.titleOf(member.session);
          return (
            <li key={member.session}>
              <label className="link-name">
                <span className="link-name-session">
                  <span
                    className="link-swatch"
                    style={{ background: linkColor(props.group.color) }}
                    aria-hidden="true"
                  />
                  {title}
                </span>
                <input
                  className="text-input"
                  aria-label={t("links.nameOf", { session: title })}
                  value={names[index] ?? ""}
                  spellCheck={false}
                  autoComplete="off"
                  maxLength={32}
                  data-autofocus={index === 0 ? "" : undefined}
                  aria-invalid={local !== null || undefined}
                  onChange={(event) => {
                    const value = event.currentTarget.value;
                    setProblem(null);
                    setNames((current) => current.map((name, at) => (at === index ? value : name)));
                  }}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") save();
                  }}
                />
              </label>
            </li>
          );
        })}
      </ul>
      <LeadChoice
        options={props.group.members.map((member) => ({
          session: member.session,
          label: props.titleOf(member.session),
        }))}
        lead={lead}
        onChange={(next) => {
          setProblem(null);
          setLead(next);
        }}
      />
      <label className="check link-ask">
        <input
          type="checkbox"
          checked={askFirst}
          onChange={(event) => {
            setAskFirst(event.currentTarget.checked);
          }}
        />
        <span>
          {t("links.askFirst")}
          <span className="muted link-ask-help">{t("links.askFirstHelp")}</span>
        </span>
      </label>
      {shown !== null && (
        <p className="link-problem" role="alert">
          {shown}
        </p>
      )}
      <div className="dialog-actions">
        <button type="button" className="secondary-button" onClick={props.onClose}>
          {t("common.cancel")}
        </button>
        <button type="button" className="primary-button" disabled={!ready} onClick={save}>
          {t("links.save")}
        </button>
      </div>
    </Dialog>
  );
}

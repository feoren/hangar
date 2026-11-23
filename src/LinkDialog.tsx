import { useId, useState } from "react";

import { costNote, groupCost } from "./costs";
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
import { useCostReport } from "./useCosts";

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
      {/* What a lead does is said once one is picked; "No lead" needs no words. */}
      {props.lead !== null && <p className="muted link-lead-help">{t("links.leadHelp")}</p>}
    </div>
  );
}

/**
 * "Link with …": the groups the session can join, one click each, and below a check box per
 * session that can be linked, the session the menu was opened on checked first, optionally a
 * lead, one button for a new group. The agents then message each other on their own (ADR 0014).
 */
export function LinkDialog(props: {
  /** Sessions that can be linked (links.ts `linkCandidates`), in sidebar order. */
  candidates: readonly SessionView[];
  /** Groups with room for one more (links.ts `groupsWithRoom`); none without. */
  groups?: readonly LinkGroup[];
  /** Adds the session the menu was opened on to `group` and closes, or answers why not. */
  onJoin?: (group: number) => Promise<string | null>;
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

  const groups = props.onJoin === undefined ? [] : (props.groups ?? []);
  const fresh = props.candidates.length >= 2;

  const link = () => {
    if (!ready) return;
    setLinking(true);
    setProblem(null);
    void props.onLink(ids, chosenLead).then((answer) => {
      setLinking(false);
      setProblem(answer);
    });
  };

  const join = (group: number) => {
    if (linking || props.onJoin === undefined) return;
    setLinking(true);
    setProblem(null);
    void props.onJoin(group).then((answer) => {
      setLinking(false);
      setProblem(answer);
    });
  };

  return (
    <Dialog labelledBy={`${id}-title`} className="link-dialog" onClose={props.onClose}>
      <DialogHead id={`${id}-title`} title={t("links.title")} onClose={props.onClose} />
      <p className="muted link-help">{t("links.help")}</p>
      {groups.length > 0 && (
        <ul className="link-groups" aria-label={t("links.joinGroup")}>
          {groups.map((group, index) => (
            <li key={group.id}>
              <button
                type="button"
                className="secondary-button link-join"
                disabled={linking}
                data-autofocus={index === 0 && !fresh ? "" : undefined}
                onClick={() => {
                  join(group.id);
                }}
              >
                <span
                  className="link-swatch"
                  style={{ background: linkColor(group.color) }}
                  aria-hidden="true"
                />
                {t("links.addToGroup", {
                  names: group.members.map((member) => member.name).join(", "),
                })}
              </button>
            </li>
          ))}
        </ul>
      )}
      {groups.length > 0 && fresh && <h3 className="link-section">{t("links.newGroup")}</h3>}
      {!fresh ? (
        groups.length === 0 && <p className="muted">{t("links.none")}</p>
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
      {fresh && <p className="muted link-tip">{t("links.tip", { key: props.selectKey })}</p>}
      {problem !== null && (
        <p className="link-problem" role="alert">
          {problem}
        </p>
      )}
      <div className="dialog-actions">
        <button type="button" className="secondary-button" onClick={props.onClose}>
          {t("common.cancel")}
        </button>
        {fresh && (
          <button type="button" className="primary-button" disabled={!ready} onClick={link}>
            {t("links.link")}
          </button>
        )}
      </div>
    </Dialog>
  );
}

/** A session that joins the group in the "Edit group" dialog, with its name there. */
export interface AddedMember {
  session: number;
  name: string;
}

/**
 * "Edit group": each member's name inside the group, which its agent writes to, sessions to add
 * with their names, the lead and the toggle "Ask before pasting". Names are checked as the
 * backend checks them before Save is offered.
 */
export function LinkGroupDialog(props: {
  group: LinkGroup;
  /** For the group's cost today; the browser's language without. */
  language?: string;
  titleOf: (session: number) => string;
  /** Sessions that could join (links.ts `linkCandidates`), in sidebar order; none without. */
  candidates?: readonly SessionView[];
  /** The name a joining session gets by default, around the names the others carry now. */
  defaultName?: (session: number, others: readonly AddedMember[]) => string;
  /** The program's look for a session that could join; the buttons show only the title without. */
  badgeFor?: (profileId: string) => BadgeLook;
  /**
   * Saves and closes, or answers why not; the dialog then stays open and shows it. `added`: the
   * sessions that join, with their names.
   */
  onSave: (
    names: { session: number; name: string }[],
    askFirst: boolean,
    lead: number | null,
    added: AddedMember[],
  ) => Promise<string | null>;
  onClose: () => void;
}) {
  const t = useT();
  const id = useId();
  const [names, setNames] = useState<readonly string[]>(() =>
    props.group.members.map((member) => member.name),
  );
  const [added, setAdded] = useState<readonly AddedMember[]>([]);
  const [askFirst, setAskFirst] = useState(props.group.askFirst);
  const [lead, setLead] = useState<number | null>(() => leadOf(props.group));
  const [problem, setProblem] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  // Only sessions that can still join: one that ended or joined elsewhere meanwhile drops out.
  const candidates = props.candidates ?? [];
  const joining = added.filter((entry) => candidates.some((s) => s.id === entry.session));
  const local = nameProblem([...names, ...joining.map((entry) => entry.name)]);
  const ready = local === null && !saving;
  const offered = candidates.filter((s) => !added.some((entry) => entry.session === s.id));
  const room = MAX_LINK_MEMBERS - props.group.members.length - joining.length;
  const canAdd = props.defaultName !== undefined && offered.length > 0 && room > 0;
  // A lead among sessions that dropped out leads nothing.
  const chosenLead =
    lead === null ||
    props.group.members.some((m) => m.session === lead) ||
    joining.some((entry) => entry.session === lead)
      ? lead
      : null;

  const add = (session: number) => {
    if (props.defaultName === undefined) return;
    const others: AddedMember[] = [
      ...props.group.members.map((member, index) => ({
        session: member.session,
        name: (names[index] ?? member.name).trim(),
      })),
      ...added,
    ];
    const name = props.defaultName(session, others);
    setProblem(null);
    setAdded((current) => [...current, { session, name }]);
  };
  const report = useCostReport();
  const today = groupCost(
    report,
    props.group.members.map((member) => member.session),
  );
  const cost = costNote(today ?? undefined, props.language ?? navigator.language, t);

  const save = () => {
    if (!ready) return;
    setSaving(true);
    setProblem(null);
    const edited = props.group.members.map((member, index) => ({
      session: member.session,
      name: (names[index] ?? member.name).trim(),
    }));
    const joined = joining.map((entry) => ({ session: entry.session, name: entry.name.trim() }));
    void props.onSave(edited, askFirst, chosenLead, joined).then((answer) => {
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
        {joining.map((entry) => {
          const title = props.titleOf(entry.session);
          return (
            <li key={entry.session} className="link-joining">
              <label className="link-name">
                <span className="link-name-session">
                  <span className="link-swatch link-swatch-new" aria-hidden="true" />
                  {title}
                  <span className="muted link-new">{t("links.newMember")}</span>
                </span>
                <input
                  className="text-input"
                  aria-label={t("links.nameOf", { session: title })}
                  value={entry.name}
                  spellCheck={false}
                  autoComplete="off"
                  maxLength={32}
                  aria-invalid={local !== null || undefined}
                  onChange={(event) => {
                    const value = event.currentTarget.value;
                    setProblem(null);
                    setAdded((current) =>
                      current.map((other) =>
                        other.session === entry.session ? { ...other, name: value } : other,
                      ),
                    );
                  }}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") save();
                  }}
                />
              </label>
              <button
                type="button"
                className="icon-button link-unadd"
                aria-label={t("links.dontAdd", { session: title })}
                title={t("links.dontAdd", { session: title })}
                onClick={() => {
                  setProblem(null);
                  setAdded((current) => current.filter((other) => other.session !== entry.session));
                }}
              >
                <CloseIcon />
              </button>
            </li>
          );
        })}
      </ul>
      {/* One button per session: a select that adds on change would add on an arrow key. */}
      {canAdd && (
        <div className="link-add" role="group" aria-labelledby={`${id}-add`}>
          <h3 id={`${id}-add`} className="link-section">
            {t("links.addSession")}
          </h3>
          <ul className="link-add-list">
            {offered.map((session) => (
              <li key={session.id}>
                <button
                  type="button"
                  className="secondary-button link-add-session"
                  title={session.cwd}
                  onClick={() => {
                    add(session.id);
                  }}
                >
                  {props.badgeFor !== undefined && (
                    <SessionLook
                      look={props.badgeFor(session.profileId)}
                      session={session}
                      crop="full"
                    />
                  )}
                  <span className="link-session-title">{session.title}</span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
      <LeadChoice
        options={[
          ...props.group.members.map((member) => member.session),
          ...joining.map((entry) => entry.session),
        ].map((session) => ({ session, label: props.titleOf(session) }))}
        lead={chosenLead}
        onChange={(next) => {
          setProblem(null);
          setLead(next);
        }}
      />
      {cost !== null && <p className="muted link-cost">{t("links.groupCost", { cost })}</p>}
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

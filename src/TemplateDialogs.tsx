import { useId, useState } from "react";

import { Badge } from "./Badge";
import { Dialog } from "./Dialog";
import { folderName } from "./folders";
import { useT } from "./i18n";
import { CloseIcon, TeamIcon } from "./icons";
import type { TemplateMember } from "./links";
import type { BadgeLook } from "./programPrefs";
import {
  renameProblem,
  TEMPLATE_NAME_LENGTH,
  templateNamed,
  type TemplateLink,
  type TemplateSession,
  type WorkspaceTemplate,
} from "./templates";

/** A running session offered for the template, with what would be saved for it. */
export interface TemplateCandidate {
  id: number;
  program: string;
  entry: TemplateSession;
}

interface SaveProps {
  candidates: readonly TemplateCandidate[];
  templates: readonly WorkspaceTemplate[];
  badgeFor: (profileId: string) => BadgeLook;
  /** Candidates that are agent sessions with the send client: only these can be linked. */
  linkable: ReadonlySet<number>;
  /** Name and role of the candidates that form a link group now (links.ts `templateMembers`). */
  members: ReadonlyMap<number, TemplateMember>;
  /** That group asks before pasting. */
  askFirst: boolean;
  onSave: (name: string, sessions: TemplateSession[], link: TemplateLink) => void;
  onClose: () => void;
}

/**
 * "Save sessions as template": a name, then one row per session with a check box and an optional
 * title, and "Link on start" where at least two chosen sessions can be linked (checked when they
 * are linked now; their names and lead go into the template). A name that exists already
 * replaces that template, and the dialog says so.
 */
export function SaveTemplateDialog({
  candidates,
  templates,
  badgeFor,
  linkable,
  members,
  askFirst,
  onSave,
  onClose,
}: SaveProps) {
  const t = useT();
  const id = useId();
  const [name, setName] = useState("");
  const [chosen, setChosen] = useState<ReadonlySet<number>>(
    () => new Set(candidates.map((candidate) => candidate.id)),
  );
  const [titles, setTitles] = useState<ReadonlyMap<number, string>>(
    () => new Map(candidates.map((candidate) => [candidate.id, candidate.entry.title ?? ""])),
  );
  const [link, setLink] = useState(
    () => candidates.filter((candidate) => members.has(candidate.id)).length >= 2,
  );
  const picked = candidates.filter((candidate) => chosen.has(candidate.id));
  // Only offer what works: linking needs two agent sessions that can be linked.
  const canLink =
    picked.filter((candidate) => linkable.has(candidate.id) || members.has(candidate.id)).length >=
    2;
  const linking = link && canLink;
  const sessions = picked.map((candidate): TemplateSession => {
    const title = (titles.get(candidate.id) ?? "").trim();
    const { profileId, folder, mode } = candidate.entry;
    const member = linking ? members.get(candidate.id) : undefined;
    return {
      profileId,
      folder,
      mode,
      ...(title === "" ? {} : { title }),
      ...(member === undefined ? {} : { memberName: member.memberName, role: member.role }),
    };
  });
  const ready = name.trim() !== "" && sessions.length > 0;
  const replaces = name.trim() !== "" && templateNamed(templates, name) !== undefined;

  const save = () => {
    if (ready) onSave(name, sessions, { link: linking, askFirst: linking && askFirst });
  };

  return (
    <Dialog labelledBy={`${id}-title`} className="template-dialog" onClose={onClose}>
      <div className="dialog-header">
        <h2 id={`${id}-title`}>{t("templates.saveTitle")}</h2>
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
      <p className="muted">{t("templates.help")}</p>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          save();
        }}
      >
        <label className="template-name">
          <span>{t("templates.name")}</span>
          <input
            className="text-input"
            type="text"
            value={name}
            maxLength={TEMPLATE_NAME_LENGTH}
            spellCheck={false}
            autoComplete="off"
            aria-describedby={replaces ? `${id}-replaces` : undefined}
            data-autofocus
            onChange={(event) => {
              setName(event.currentTarget.value);
            }}
          />
        </label>
        {replaces && (
          <p id={`${id}-replaces`} className="hint">
            {t("templates.replaces")}
          </p>
        )}
        <fieldset className="settings-group template-sessions">
          <legend>{t("sidebar.sessions")}</legend>
          <ul className="template-list">
            {candidates.map((candidate) => {
              const team = candidate.entry.mode === "team";
              return (
                <li key={candidate.id} className="template-row">
                  <label className="check template-session" title={candidate.entry.folder}>
                    <input
                      type="checkbox"
                      checked={chosen.has(candidate.id)}
                      onChange={(event) => {
                        const on = event.currentTarget.checked;
                        setChosen((current) => {
                          const next = new Set(current);
                          if (on) next.add(candidate.id);
                          else next.delete(candidate.id);
                          return next;
                        });
                      }}
                    />
                    <Badge look={badgeFor(candidate.entry.profileId)} />
                    <span className="template-program">{candidate.program}</span>
                    <span className="template-folder">{folderName(candidate.entry.folder)}</span>
                    <span
                      className="template-mode"
                      title={t(team ? "start.team" : "start.private")}
                    >
                      {team && <TeamIcon size={12} />}
                      {t(team ? "start.team" : "start.private")}
                    </span>
                  </label>
                  <input
                    className="text-input template-title"
                    type="text"
                    value={titles.get(candidate.id) ?? ""}
                    maxLength={80}
                    placeholder={t("templates.titlePlaceholder")}
                    aria-label={t("templates.sessionTitle", { program: candidate.program })}
                    disabled={!chosen.has(candidate.id)}
                    spellCheck={false}
                    autoComplete="off"
                    onChange={(event) => {
                      const value = event.currentTarget.value;
                      setTitles((current) => new Map(current).set(candidate.id, value));
                    }}
                  />
                </li>
              );
            })}
          </ul>
        </fieldset>
        {canLink && (
          <label className="check link-ask template-link">
            <input
              type="checkbox"
              checked={link}
              onChange={(event) => {
                setLink(event.currentTarget.checked);
              }}
            />
            <span>
              {t("links.templateLink")}
              <span className="muted link-ask-help">{t("links.templateLinkHelp")}</span>
            </span>
          </label>
        )}
        <div className="dialog-actions">
          <button type="button" className="secondary-button" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary-button" disabled={!ready}>
            {t("templates.saveButton")}
          </button>
        </div>
      </form>
    </Dialog>
  );
}

interface RenameProps {
  template: WorkspaceTemplate;
  templates: readonly WorkspaceTemplate[];
  onRename: (name: string) => void;
  onClose: () => void;
}

/** Rename a template; a name another template carries is refused with a short reason. */
export function RenameTemplateDialog({ template, templates, onRename, onClose }: RenameProps) {
  const t = useT();
  const id = useId();
  const [name, setName] = useState(template.name);
  const problem = renameProblem(templates, template.id, name);

  return (
    <Dialog labelledBy={`${id}-title`} className="template-dialog small" onClose={onClose}>
      <h2 id={`${id}-title`}>{t("templates.renameTitle")}</h2>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (problem === null) onRename(name);
        }}
      >
        <label className="template-name">
          <span>{t("templates.name")}</span>
          <input
            className="text-input"
            type="text"
            value={name}
            maxLength={TEMPLATE_NAME_LENGTH}
            spellCheck={false}
            autoComplete="off"
            aria-invalid={problem === "taken"}
            aria-describedby={problem === "taken" ? `${id}-taken` : undefined}
            data-autofocus
            onChange={(event) => {
              setName(event.currentTarget.value);
            }}
            onFocus={(event) => {
              event.currentTarget.select();
            }}
          />
        </label>
        {problem === "taken" && (
          <p id={`${id}-taken`} className="hint" role="alert">
            {t("templates.taken")}
          </p>
        )}
        <div className="dialog-actions">
          <button type="button" className="secondary-button" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary-button" disabled={problem !== null}>
            {t("templates.renameButton")}
          </button>
        </div>
      </form>
    </Dialog>
  );
}

import { useEffect, useId, useState } from "react";

import { api, errorText, type NoteFolder } from "./api";
import { Dialog } from "./Dialog";
import { folderName } from "./folders";
import { useT } from "./i18n";
import {
  parseSaveCode,
  parseSharedStatus,
  parseUnlock,
  unlockError,
  type SharedStatus,
} from "./sharedNotes";

/**
 * "setup": turn solo shared notes on (with a code); "unlock": type the notes recovery code;
 * "teamOn": turn team notes on (no code); "shareTeam": share the unlocked solo notes with the team.
 */
export type SharedDialogMode = "setup" | "unlock" | "teamOn" | "shareTeam";

interface Props {
  mode: SharedDialogMode;
  /** The working folder (private form, `{ path }`). */
  folder: NoteFolder;
  /** The team's project as the texts name it (team modes). */
  project: string;
  onDone: (status: SharedStatus) => void;
  onClose: () => void;
}

/**
 * The one dialog of shared notes. Setup shows the notes recovery code once with Copy and Save,
 * and "Turn on" works only after "I saved the code" (owner decision: the code is mandatory).
 * Unlock takes the code and says in one line what was wrong with it. The team modes need no
 * code: one sentence says who can read the notes, one button confirms.
 */
export function SharedNotesDialog(props: Props) {
  if (props.mode === "teamOn" || props.mode === "shareTeam") {
    return <TeamDialog {...props} mode={props.mode} />;
  }
  return <CodeDialog {...props} mode={props.mode} />;
}

/** Turns team notes on or shares solo notes with the team; the relay may take up to 20 s. */
function TeamDialog({
  mode,
  folder,
  project,
  onDone,
  onClose,
}: Props & { mode: "teamOn" | "shareTeam" }) {
  const t = useT();
  const titleId = useId();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const label = t("sharedNotes.otherVersion");
  const untitled = t("notes.untitled");
  const sharing = mode === "shareTeam";

  const confirm = async () => {
    setBusy(true);
    setError(null);
    try {
      const value = sharing
        ? await api.sharedNotesToTeam(folder, label, untitled)
        : await api.sharedNotesTeamEnable(folder, label, untitled);
      const status = parseSharedStatus(value);
      if (status) onDone(status);
      else onClose();
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };

  return (
    <Dialog labelledBy={titleId} className="shared-notes-dialog" onClose={onClose}>
      <h2 id={titleId}>
        {sharing ? t("sharedNotes.shareLine", { project }) : t("sharedNotes.setupTitle")}
      </h2>
      <p>{t(sharing ? "sharedNotes.shareBody" : "sharedNotes.teamOnBody", { project })}</p>
      <p className="muted" role="status">
        {busy ? t("sharedNotes.teamWorking") : ""}
      </p>
      {error && (
        <p className="error-text" role="alert">
          {error}
        </p>
      )}
      <div className="dialog-actions">
        <button type="button" className="secondary-button" onClick={onClose}>
          {t("common.cancel")}
        </button>
        <button
          type="button"
          className="primary-button"
          disabled={busy}
          data-autofocus
          onClick={() => void confirm()}
        >
          {sharing ? t("sharedNotes.shareButton") : t("sharedNotes.turnOn")}
        </button>
      </div>
    </Dialog>
  );
}

function CodeDialog({ mode, folder, onDone, onClose }: Props & { mode: "setup" | "unlock" }) {
  const t = useT();
  const titleId = useId();
  const [code, setCode] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState("");
  const label = t("sharedNotes.otherVersion");
  const untitled = t("notes.untitled");
  const project = folder.path === null ? "" : folderName(folder.path);

  useEffect(() => {
    if (mode !== "setup") return;
    let live = true;
    api
      .sharedNotesPrepare(folder)
      .then((value) => {
        if (live && typeof value === "string") setCode(value);
      })
      .catch((e: unknown) => {
        if (live) setError(errorText(e));
      });
    return () => {
      live = false;
    };
  }, [mode, folder]);

  const finish = (value: unknown) => {
    const status = parseSharedStatus(value);
    if (status) onDone(status);
    else onClose();
  };

  /** Closing without turning notes on drops the prepared code in the backend too. */
  const cancel = () => {
    if (mode === "setup") api.sharedNotesCancel(folder).catch(() => undefined);
    onClose();
  };

  const turnOn = async () => {
    setBusy(true);
    setError(null);
    try {
      finish(await api.sharedNotesEnable(folder, label, untitled));
    } catch (e) {
      // The backend keeps the setup unless a file was already written; ask for the code again,
      // and if it is not the one shown, the person has to save the new one.
      const again = await api.sharedNotesPrepare(folder).catch(() => null);
      if (typeof again === "string" && again !== code) {
        setCode(again);
        setSaved(false);
        setError(t("sharedNotes.codeChanged"));
      } else {
        setError(errorText(e));
      }
      setBusy(false);
    }
  };

  const unlock = async () => {
    if (typed.trim() === "") return;
    setBusy(true);
    setError(null);
    try {
      const outcome = parseUnlock(await api.sharedNotesUnlock(folder, typed, label, untitled));
      if (outcome === null) {
        onClose();
        return;
      }
      if (outcome.outcome === "unlocked") {
        onDone(outcome.status);
        return;
      }
      const key = unlockError(outcome);
      if (key) setError(t(key));
    } catch (e) {
      setError(errorText(e));
    }
    setBusy(false);
  };

  if (mode === "unlock") {
    return (
      <Dialog labelledBy={titleId} className="shared-notes-dialog" onClose={cancel}>
        <h2 id={titleId}>{t("sharedNotes.unlockTitle")}</h2>
        <p>{t("sharedNotes.unlockBody")}</p>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void unlock();
          }}
        >
          <input
            className="shared-notes-code-input"
            aria-label={t("sharedNotes.codeLabel")}
            value={typed}
            spellCheck={false}
            autoComplete="off"
            data-autofocus
            onChange={(event) => {
              setTyped(event.currentTarget.value);
              setError(null);
            }}
          />
          {error && (
            <p className="error-text" role="alert">
              {error}
            </p>
          )}
          <div className="dialog-actions">
            <button type="button" className="secondary-button" onClick={cancel}>
              {t("common.cancel")}
            </button>
            <button type="submit" className="primary-button" disabled={busy || typed.trim() === ""}>
              {t("sharedNotes.unlockButton")}
            </button>
          </div>
        </form>
      </Dialog>
    );
  }

  return (
    <Dialog labelledBy={titleId} className="shared-notes-dialog" onClose={cancel}>
      <h2 id={titleId}>{t("sharedNotes.setupTitle")}</h2>
      <p>{t("sharedNotes.setupBody")}</p>
      <p className="shared-notes-code-label">{t("sharedNotes.codeLabel")}</p>
      <output className="shared-notes-code selectable" aria-live="polite">
        {code ?? t("sharedNotes.preparing")}
      </output>
      <p className="muted">{t("sharedNotes.codeWarning")}</p>
      <div className="shared-notes-code-actions">
        <button
          type="button"
          className="secondary-button"
          disabled={code === null}
          onClick={() => {
            if (code === null) return;
            api
              .sharedNotesCopyCode(folder)
              .then(() => {
                setNotice(t("sharedNotes.codeCopied"));
              })
              .catch((e: unknown) => {
                setError(errorText(e));
              });
          }}
        >
          {t("menu.copy")}
        </button>
        <button
          type="button"
          className="secondary-button"
          disabled={code === null}
          onClick={() => {
            api
              .sharedNotesSaveCode(folder, t("sharedNotes.fileHeading", { project }))
              .then((value) => {
                const outcome = parseSaveCode(value);
                if (outcome?.outcome === "saved") {
                  setNotice(t("sharedNotes.codeSaved", { path: outcome.path }));
                } else if (outcome?.outcome === "insideRepository") {
                  setNotice("");
                  setError(t("sharedNotes.codeInRepository"));
                }
              })
              .catch((e: unknown) => {
                setError(errorText(e));
              });
          }}
        >
          {t("sharedNotes.saveCode")}
        </button>
        <span className="muted" role="status">
          {notice}
        </span>
      </div>
      <label className="shared-notes-confirm">
        <input
          type="checkbox"
          checked={saved}
          disabled={code === null}
          onChange={(event) => {
            setSaved(event.currentTarget.checked);
          }}
        />
        <span>{t("sharedNotes.confirmSaved")}</span>
      </label>
      {error && (
        <p className="error-text" role="alert">
          {error}
        </p>
      )}
      <div className="dialog-actions">
        <button type="button" className="secondary-button" onClick={cancel}>
          {t("common.cancel")}
        </button>
        <button
          type="button"
          className="primary-button"
          disabled={!saved || code === null || busy}
          onClick={() => void turnOn()}
        >
          {t("sharedNotes.turnOn")}
        </button>
      </div>
    </Dialog>
  );
}

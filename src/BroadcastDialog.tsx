import { useState } from "react";

import { Badge } from "./Badge";
import { canBroadcast } from "./broadcast";
import { Dialog } from "./Dialog";
import { shortPath } from "./format";
import { useT } from "./i18n";
import { CloseIcon } from "./icons";
import type { BadgeLook } from "./programPrefs";
import { SessionMarker } from "./SessionMarker";
import type { SessionView } from "./sessions";
import { snippetLabel, type Snippet } from "./snippets";

interface Props {
  /** Running sessions that can receive the text (`broadcastCandidates`). */
  candidates: readonly SessionView[];
  /** Checked at the start (`defaultTargets`). */
  initial: readonly number[];
  badgeFor: (profileId: string) => BadgeLook;
  /** Snippets that can start the text; their variables are filled per session on sending. */
  snippets: readonly Snippet[];
  /**
   * Sends and closes, or answers why nothing was sent (a variable without a value in one of the
   * sessions); the dialog then stays open and shows it.
   */
  onSend: (text: string, ids: number[]) => Promise<string | null>;
  onClose: () => void;
}

/**
 * "Send to several": one text, a check box per running session, one button. Each chosen session
 * gets the text as a bracketed paste followed by Enter; afterwards the split view shows them.
 */
export function BroadcastDialog({
  candidates,
  initial,
  badgeFor,
  snippets,
  onSend,
  onClose,
}: Props) {
  const t = useT();
  const [text, setText] = useState("");
  const [chosen, setChosen] = useState<ReadonlySet<number>>(() => new Set(initial));
  const [problem, setProblem] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  // In sidebar order, only sessions that still run.
  const targets = candidates.filter((session) => chosen.has(session.id)).map((s) => s.id);
  const ready = canBroadcast(text, targets) && !sending;

  const send = () => {
    if (!ready) return;
    setSending(true);
    setProblem(null);
    void onSend(text, targets).then((answer) => {
      setSending(false);
      setProblem(answer);
    });
  };

  return (
    <Dialog labelledBy="broadcast-title" className="broadcast" onClose={onClose}>
      <div className="dialog-header">
        <h2 id="broadcast-title">{t("broadcast.title")}</h2>
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
      <textarea
        className="text-input broadcast-text"
        aria-label={t("broadcast.textLabel")}
        aria-describedby="broadcast-help"
        placeholder={t("broadcast.placeholder")}
        rows={4}
        value={text}
        spellCheck={false}
        data-autofocus
        onChange={(event) => {
          setText(event.currentTarget.value);
          setProblem(null);
        }}
        onKeyDown={(event) => {
          // Ctrl+Enter sends, as in chat tools; Enter alone makes a new line.
          if (event.key === "Enter" && event.ctrlKey) {
            event.preventDefault();
            send();
          }
        }}
      />
      <div className="broadcast-help-row">
        <p id="broadcast-help" className="muted">
          {t("broadcast.help")}
        </p>
        {snippets.length > 0 && (
          <select
            className="select broadcast-snippet"
            aria-label={t("snippets.insert")}
            value=""
            onChange={(event) => {
              const picked = snippets.find((snippet) => snippet.id === event.currentTarget.value);
              if (picked === undefined) return;
              setText(picked.text);
              setProblem(null);
            }}
          >
            <option value="" disabled>
              {t("snippets.insert")}
            </option>
            {snippets.map((snippet) => (
              <option key={snippet.id} value={snippet.id}>
                {snippetLabel(snippet)}
              </option>
            ))}
          </select>
        )}
      </div>
      {problem !== null && (
        <p className="broadcast-problem" role="alert">
          {problem}
        </p>
      )}
      <fieldset className="settings-group broadcast-sessions">
        <legend>{t("sidebar.sessions")}</legend>
        {candidates.length === 0 ? (
          <p className="muted">{t("broadcast.none")}</p>
        ) : (
          <ul className="broadcast-list">
            {candidates.map((session) => (
              <li key={session.id}>
                <label className="check broadcast-session" title={session.cwd}>
                  <input
                    type="checkbox"
                    checked={chosen.has(session.id)}
                    onChange={(event) => {
                      const on = event.currentTarget.checked;
                      setChosen((current) => {
                        const next = new Set(current);
                        if (on) next.add(session.id);
                        else next.delete(session.id);
                        return next;
                      });
                    }}
                  />
                  <Badge look={badgeFor(session.profileId)} />
                  <SessionMarker marker={session.marker} />
                  <span className="broadcast-title">{session.title}</span>
                  <span className="broadcast-path">{shortPath(session.cwd)}</span>
                </label>
              </li>
            ))}
          </ul>
        )}
      </fieldset>
      <div className="dialog-actions">
        <button type="button" className="secondary-button" onClick={onClose}>
          {t("common.cancel")}
        </button>
        <button type="button" className="primary-button" disabled={!ready} onClick={send}>
          {t("broadcast.send")}
        </button>
      </div>
    </Dialog>
  );
}

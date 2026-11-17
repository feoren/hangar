import { useEffect, useId, useRef, useState } from "react";

import { errorText } from "./api";
import { Dialog } from "./Dialog";
import { useT } from "./i18n";
import { CloseIcon } from "./icons";
import { MESSAGE_MAX_CHARS, canSendMessage } from "./partnerMessage";
import type { NamedProject } from "./partners";

/** Where the message to the chosen partner would go, as the dialog shows it. */
export type TargetView =
  | { kind: "one"; id: number; title: string }
  | { kind: "choose"; sessions: readonly { id: number; title: string }[] }
  | { kind: "start"; profileId: string; program: string; folder: string }
  | { kind: "none" }
  | { kind: "error"; text: string };

/** The chosen target: a running session, or a new one with this program. */
export type MessageTarget = { kind: "session"; id: number } | { kind: "start"; profileId: string };

export interface PartnerSend {
  partner: string;
  text: string;
  /** Enter after the paste ("Paste and send"). */
  submit: boolean;
  target: MessageTarget;
}

interface Props {
  /** Title of the sending session. */
  title: string;
  /** The partners of the sending session's project, at least one. */
  partners: readonly NamedProject[];
  /** Where a message to `partner` would go right now. */
  resolve: (partner: string) => Promise<TargetView>;
  onSend: (send: PartnerSend) => void;
  onClose: () => void;
}

function chosenTarget(view: TargetView | null, picked: number | null): MessageTarget | null {
  if (view === null) return null;
  switch (view.kind) {
    case "one":
      return { kind: "session", id: view.id };
    case "choose":
      return picked !== null && view.sessions.some((s) => s.id === picked)
        ? { kind: "session", id: picked }
        : null;
    case "start":
      return { kind: "start", profileId: view.profileId };
    case "none":
    case "error":
      return null;
  }
}

/**
 * "Send to partner": pick a partner project, write the text, then paste it into the partner's
 * agent session, with Enter ("Paste and send") or without ("Paste").
 */
export function PartnerMessageDialog({ title, partners, resolve, onSend, onClose }: Props) {
  const t = useT();
  const id = useId();
  const [partner, setPartner] = useState(partners[0]?.name ?? "");
  const [text, setText] = useState("");
  /** The answer for one partner; another partner's answer shows as loading. */
  const [answer, setAnswer] = useState<{ partner: string; view: TargetView } | null>(null);
  const [picked, setPicked] = useState<number | null>(null);
  // The caller's function may change on every render; only a new partner asks again.
  const resolveRef = useRef(resolve);
  useEffect(() => {
    resolveRef.current = resolve;
  });

  useEffect(() => {
    if (partner === "") return;
    let alive = true;
    void resolveRef.current(partner).then(
      (view) => {
        if (alive) setAnswer({ partner, view });
      },
      (error: unknown) => {
        if (alive) setAnswer({ partner, view: { kind: "error", text: errorText(error) } });
      },
    );
    return () => {
      alive = false;
    };
  }, [partner]);

  const view = answer?.partner === partner ? answer.view : null;
  const target = chosenTarget(view, picked);
  const tooLong = Array.from(text).length > MESSAGE_MAX_CHARS;
  const ready = target !== null && canSendMessage(text);
  const send = (submit: boolean) => {
    if (!ready) return;
    onSend({ partner, text, submit, target });
  };

  return (
    <Dialog labelledBy={`${id}-title`} className="partner-dialog" onClose={onClose}>
      <div className="dialog-header">
        <h2 id={`${id}-title`}>
          {t("partner.title")}
          <span className="marker-dialog-session">{title}</span>
        </h2>
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
      <p className="muted partner-help">{t("partner.help")}</p>
      {partners.length > 1 && (
        <label className="partner-field">
          <span>{t("partner.to")}</span>
          <select
            className="text-input"
            value={partner}
            onChange={(event) => {
              setPartner(event.target.value);
              setPicked(null);
            }}
          >
            {partners.map((project) => (
              <option key={project.name} value={project.name} title={project.folder}>
                {project.name}
              </option>
            ))}
          </select>
        </label>
      )}
      <label className="partner-field">
        <span>
          {partners.length === 1
            ? `${t("partner.text")} · ${partners[0]?.name ?? ""}`
            : t("partner.text")}
        </span>
        <textarea
          className="text-input partner-text"
          data-autofocus
          rows={6}
          value={text}
          aria-invalid={tooLong || undefined}
          onChange={(event) => {
            setText(event.target.value);
          }}
          onKeyDown={(event) => {
            // Ctrl+Enter sends, as in the other text dialogs; plain Enter is a new line.
            if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
              event.preventDefault();
              send(true);
            }
          }}
        />
      </label>
      {tooLong && (
        <p className="error-text" role="alert">
          {t("partner.tooLong", { max: MESSAGE_MAX_CHARS })}
        </p>
      )}
      <TargetLine view={view} picked={picked} onPick={setPicked} />
      <div className="dialog-actions">
        <button type="button" className="secondary-button" onClick={onClose}>
          {t("common.cancel")}
        </button>
        <button
          type="button"
          className="secondary-button"
          disabled={!ready}
          onClick={() => {
            send(false);
          }}
        >
          {t("partner.paste")}
        </button>
        <button
          type="button"
          className="primary-button"
          disabled={!ready}
          onClick={() => {
            send(true);
          }}
        >
          {t("partner.pasteSend")}
        </button>
      </div>
    </Dialog>
  );
}

function TargetLine({
  view,
  picked,
  onPick,
}: {
  view: TargetView | null;
  picked: number | null;
  onPick: (id: number) => void;
}) {
  const t = useT();
  const name = useId();
  if (view === null) return <p className="muted partner-target">{t("partner.loading")}</p>;
  switch (view.kind) {
    case "one":
      return <p className="partner-target">{t("partner.toOne", { session: view.title })}</p>;
    case "start":
      return (
        <p className="partner-target">
          {t("partner.start", { program: view.program, folder: view.folder })}
        </p>
      );
    case "none":
      return <p className="error-text partner-target">{t("partner.noProgram")}</p>;
    case "error":
      return (
        <p className="error-text partner-target" role="alert">
          {view.text}
        </p>
      );
    case "choose":
      return (
        <fieldset className="partner-target partner-choice">
          <legend>{t("partner.choose")}</legend>
          {view.sessions.map((session) => (
            <label key={session.id}>
              <input
                type="radio"
                name={name}
                checked={picked === session.id}
                onChange={() => {
                  onPick(session.id);
                }}
              />
              <span>{session.title}</span>
            </label>
          ))}
        </fieldset>
      );
  }
}

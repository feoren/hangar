import { useEffect, useId, useRef, useState } from "react";

import { useT } from "./i18n";
import { CheckIcon, CopyIcon } from "./icons";
import { reasonText, type ProgramUpdate } from "./programUpdates";

interface Props {
  /** Display name of the program. */
  name: string;
  update: ProgramUpdate;
  /** An update session of this program runs right now; the button shows it instead. */
  updating: boolean;
  /** The program runs and the user clicked "Update": ask before running. */
  confirming: boolean;
  /** "Update" clicked: the app starts it or asks first (program running). */
  onRequest: () => void;
  /** "Update anyway" after the question. */
  onConfirm: () => void;
  onCancel: () => void;
  className?: string;
}

/**
 * A program's new version below its row (sidebar, Settings): one click updates when Hangar can
 * run the update, otherwise the command to copy or only the version with the reason. Notes on
 * administrator rights and on other copies stay short; the copies list opens on demand.
 */
export function ProgramUpdateOffer(props: Props) {
  const { update, name } = props;
  const t = useT();
  const [copied, setCopied] = useState<boolean | null>(null);
  const confirmButton = useRef<HTMLButtonElement>(null);
  const confirmId = useId();

  useEffect(() => {
    if (props.confirming) confirmButton.current?.focus();
  }, [props.confirming]);

  useEffect(() => {
    if (copied !== true) return;
    const timer = window.setTimeout(() => {
      setCopied(null);
    }, 4000);
    return () => {
      window.clearTimeout(timer);
    };
  }, [copied]);

  if (!update.updateAvailable || update.latest === null) return null;
  const latest = update.latest;
  const command = update.command;
  const copies = update.warnings.includes("second-copy") && update.copies.length > 1;

  const copy = () => {
    if (command === null) return;
    navigator.clipboard
      .writeText(command)
      .then(() => {
        setCopied(true);
      })
      .catch(() => {
        setCopied(false);
      });
  };

  return (
    <div className={props.className ? `update-offer ${props.className}` : "update-offer"}>
      <div className="update-offer-line">
        <span className="update-offer-versions">
          {update.plan === "run" ? (
            <>
              <span aria-hidden="true">
                {update.installed ?? "?"} → {latest}
              </span>
              <span className="sr-only">{t("update.programAvailable", { version: latest })}</span>
            </>
          ) : (
            t("update.newVersion", { version: latest })
          )}
        </span>
        {update.plan === "run" && (
          <button
            type="button"
            className={props.updating ? "update-offer-button busy" : "update-offer-button"}
            aria-label={
              props.updating
                ? t("update.showRunning", { name })
                : t("update.runFromTo", { name, installed: update.installed ?? "?", latest })
            }
            title={command ?? undefined}
            aria-expanded={props.confirming ? true : undefined}
            aria-controls={props.confirming ? confirmId : undefined}
            onClick={props.onRequest}
          >
            {props.updating ? t("update.running") : t("update.run")}
          </button>
        )}
        {update.plan === "copy" && command !== null && (
          <button
            type="button"
            className={copied === true ? "update-offer-copy copied" : "update-offer-copy"}
            aria-label={t("update.copyFor", { name })}
            title={command}
            onClick={copy}
          >
            {copied === true ? <CheckIcon size={12} /> : <CopyIcon size={12} />}
            <span>{t("update.copyShort")}</span>
          </button>
        )}
      </div>
      {update.plan !== "run" && <p className="update-offer-note">{reasonText(update.reason, t)}</p>}
      {update.plan === "run" && update.needsAdmin && (
        <p className="update-offer-note">{t("update.needsAdmin")}</p>
      )}
      {copies && (
        <details className="update-offer-copies">
          <summary>{t("update.copiesFound", { count: update.copies.length })}</summary>
          <p className="update-offer-note">{t("update.copiesHelp")}</p>
          <ol className="update-offer-paths selectable">
            {update.copies.map((path) => (
              <li key={path}>{path}</li>
            ))}
          </ol>
        </details>
      )}
      {props.confirming && (
        <div id={confirmId} className="update-offer-confirm" role="group">
          <p>{t("update.runningConfirm", { name })}</p>
          <div className="update-offer-actions">
            <button
              ref={confirmButton}
              type="button"
              className="secondary-button"
              onClick={props.onConfirm}
            >
              {t("update.runAnyway")}
            </button>
            <button type="button" className="link-button" onClick={props.onCancel}>
              {t("common.cancel")}
            </button>
          </div>
        </div>
      )}
      <span
        className={
          copied === false ? "update-offer-note error-text selectable" : "update-offer-note"
        }
        role="status"
      >
        {copied === null
          ? ""
          : copied
            ? t("update.commandCopied")
            : t("settings.copyFailed", { command: command ?? "" })}
      </span>
    </div>
  );
}

/** What the app does with "Update" (shared by sidebar and Settings). */
export interface UpdateActions {
  /** Profile IDs whose update session runs right now. */
  updating: ReadonlySet<string>;
  /** Program whose update waits for "Update anyway", otherwise `null`. */
  confirming: string | null;
  request: (profileId: string) => void;
  confirm: (profileId: string) => void;
  cancel: () => void;
}

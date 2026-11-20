import { useEffect, useId, useState } from "react";

import { Dialog } from "./Dialog";
import { formatShortTime } from "./format";
import { useT, type Translate } from "./i18n";
import { CloseIcon } from "./icons";
import { historyPreview, linkColor, type LinkHistoryEntry } from "./links";

/** Who sent a history entry: the member's name, "(lead)" for the lead, or Hangar. */
export function historySender(entry: LinkHistoryEntry, t: Translate): string {
  if (entry.fromName === null) return t("links.hangar");
  return entry.fromRole === "lead" ? `${entry.fromName} (${t("links.lead")})` : entry.fromName;
}

/** One message: time, route and outcome with the first line; opened, the whole text to copy. */
function HistoryRow({
  entry,
  language,
  copy,
}: {
  entry: LinkHistoryEntry;
  language: string;
  copy: (text: string) => Promise<void>;
}) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  return (
    <li className={`link-history-entry ${entry.outcome}`}>
      <details>
        <summary>
          <span className="link-history-head">
            <time dateTime={new Date(entry.at).toISOString()}>
              {formatShortTime(new Date(entry.at), language)}
            </time>
            <span className="link-history-route">
              {t("links.historyRoute", { from: historySender(entry, t), to: entry.toName })}
            </span>
            <span className="link-history-outcome">
              {t(entry.outcome === "delivered" ? "links.delivered" : "links.discarded")}
            </span>
          </span>
          <span className="link-history-preview">{historyPreview(entry.text)}</span>
        </summary>
        <pre className="link-history-text">{entry.text}</pre>
        <button
          type="button"
          className="secondary-button link-history-copy"
          onClick={() => {
            copy(entry.text)
              .then(() => {
                setCopied(true);
              })
              .catch(() => {
                setCopied(false);
              });
          }}
        >
          {copied ? t("links.copied") : t("links.copy")}
        </button>
      </details>
    </li>
  );
}

/**
 * "Group history": the last messages of a link group, newest first, each closed to one line and
 * opened for the whole text (ADR 0014). It reads the history again whenever `version` changes
 * (the group's messages changed); the history lives in the backend's memory only.
 */
export function LinkHistoryDialog(props: {
  color: number;
  /** Reads the history; answers why not as text. */
  load: () => Promise<LinkHistoryEntry[] | string>;
  /** Changes whenever the history may have grown. */
  version: unknown;
  language: string;
  copy: (text: string) => Promise<void>;
  onClose: () => void;
}) {
  const t = useT();
  const id = useId();
  const [entries, setEntries] = useState<LinkHistoryEntry[] | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const { load } = props;

  useEffect(() => {
    let live = true;
    void load().then((answer) => {
      if (!live) return;
      if (typeof answer === "string") {
        setProblem(answer);
      } else {
        setProblem(null);
        setEntries(answer);
      }
    });
    return () => {
      live = false;
    };
  }, [load, props.version]);

  return (
    <Dialog
      labelledBy={`${id}-title`}
      className="link-dialog link-history-dialog"
      onClose={props.onClose}
    >
      <div className="dialog-header">
        <h2 id={`${id}-title`}>
          <span
            className="link-swatch"
            style={{ background: linkColor(props.color) }}
            aria-hidden="true"
          />
          {t("links.historyTitle")}
        </h2>
        <button
          type="button"
          className="icon-button"
          aria-label={t("common.close")}
          title={t("common.close")}
          data-autofocus=""
          onClick={props.onClose}
        >
          <CloseIcon />
        </button>
      </div>
      <p className="muted link-help">{t("links.historyHelp")}</p>
      {problem !== null && (
        <p className="link-problem" role="alert">
          {problem}
        </p>
      )}
      {entries !== null && (
        <LinkHistoryList entries={entries} language={props.language} copy={props.copy} />
      )}
    </Dialog>
  );
}

/** The history's rows, newest first, or a line that there is none yet. */
export function LinkHistoryList(props: {
  entries: readonly LinkHistoryEntry[];
  language: string;
  copy: (text: string) => Promise<void>;
}) {
  const t = useT();
  if (props.entries.length === 0) return <p className="muted">{t("links.historyEmpty")}</p>;
  return (
    <ol className="link-history">
      {props.entries.map((entry, index) => (
        <HistoryRow
          // Counted from the oldest, so an open row stays open when a newer one arrives.
          key={`${String(entry.at)}-${String(props.entries.length - index)}`}
          entry={entry}
          language={props.language}
          copy={props.copy}
        />
      ))}
    </ol>
  );
}

import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";

import { api, errorText } from "./api";
import { ChangeList } from "./ChangeList";
import { Dialog } from "./Dialog";
import { parseGitDiff, type GitDiff } from "./gitDiff";
import { commitAge, commitDate } from "./gitHistory";
import { useT, type Translate } from "./i18n";
import { ChevronIcon } from "./icons";
import { revealInvisible } from "./invisible";
import { Revealed } from "./Revealed";
import {
  canRestore,
  concernsSession,
  parseRestoreOutcome,
  parseSnapshotList,
  SNAPSHOTS_CHANGED,
  snapshotTitle,
  type Skipped,
  type Snapshot,
  type SnapshotList,
} from "./timeline";
import { useClock } from "./useClock";

interface Props {
  /** The active session; the parent remounts this view for another session. */
  sessionId: number;
  /** Changes when the list should be read again (attention state, "Refresh"). */
  reload: string;
  /** UI language, for relative times and dates. */
  language: string;
  /** The session's agent is working: going back waits until it is done. */
  working: boolean;
  /** Whether the list is being read, for the spinning refresh button. */
  onBusy: (busy: boolean) => void;
  /** The work tree went back to a snapshot: the panel reads its changes again. */
  onRestored: () => void;
}

type ListLoad =
  { kind: "loading" } | { kind: "error"; message: string } | { kind: "done"; list: SnapshotList };

type DetailLoad =
  { kind: "loading" } | { kind: "error"; message: string } | { kind: "done"; result: GitDiff };

/** The last jump back, shown above the list until the next one. */
type Notice =
  | { kind: "restored" }
  | { kind: "alreadyThere" }
  | { kind: "incomplete"; message: string }
  | { kind: "error"; message: string };

const MINUTE_MS = 60_000;

/**
 * The silent snapshots Hangar took of the session's work tree after each agent step, newest
 * first, in the "Timeline" view of the changes panel. A click opens a step's changes; "Go back to
 * this state" asks, then makes the files match that snapshot after saving the current state.
 */
export function TimelineView({ sessionId, reload, language, working, onBusy, onRestored }: Props) {
  const t = useT();
  const now = useClock(MINUTE_MS);
  const [changed, setChanged] = useState(0);
  const [answer, setAnswer] = useState<{ key: string; load: ListLoad } | null>(null);
  const [expanded, setExpanded] = useState<ReadonlySet<number>>(() => new Set());
  const [details, setDetails] = useState<ReadonlyMap<number, DetailLoad>>(() => new Map());
  const [asking, setAsking] = useState<Snapshot | null>(null);
  const [restoring, setRestoring] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const latest = useRef("");
  const key = `${reload}\n${String(changed)}`;

  // The backend says when this session's snapshots change (a new step, a jump back).
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | null = null;
    listen<unknown>(SNAPSHOTS_CHANGED, (event) => {
      if (concernsSession(event.payload, sessionId)) setChanged((value) => value + 1);
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else stop = unlisten;
      })
      // Without the event the list still follows the attention state and "Refresh".
      .catch(() => undefined);
    return () => {
      disposed = true;
      stop?.();
    };
  }, [sessionId]);

  useEffect(() => {
    latest.current = key;
    const settle = (load: ListLoad) => {
      // An older answer that arrives late must not replace a newer one.
      if (latest.current === key) setAnswer({ key, load });
    };
    api
      .snapshotList(sessionId)
      .then((value) => {
        const list = parseSnapshotList(value);
        settle(
          list === null ? { kind: "error", message: t("error.unknown") } : { kind: "done", list },
        );
      })
      .catch((e: unknown) => {
        settle({ kind: "error", message: errorText(e) });
      });
    // `t` only changes with the language; a new language needs no new git call.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, key]);

  const busy = answer?.key !== key || restoring;
  useEffect(() => {
    onBusy(busy);
  }, [busy, onBusy]);
  useEffect(
    () => () => {
      onBusy(false);
    },
    [onBusy],
  );

  const readStep = (number: number) => {
    setDetails((current) => new Map(current).set(number, { kind: "loading" }));
    const settle = (detail: DetailLoad) => {
      setDetails((current) => new Map(current).set(number, detail));
    };
    api
      .snapshotDiff(sessionId, number)
      .then((value) => {
        const result = parseGitDiff(value);
        settle(
          result === null
            ? { kind: "error", message: t("error.unknown") }
            : { kind: "done", result },
        );
      })
      .catch((e: unknown) => {
        settle({ kind: "error", message: errorText(e) });
      });
  };

  const toggle = (number: number) => {
    const open = expanded.has(number);
    const next = new Set(expanded);
    if (open) next.delete(number);
    else next.add(number);
    setExpanded(next);
    // A snapshot never changes, so its diff is read once; a failed read is tried again.
    const known = details.get(number);
    if (!open && (known === undefined || known.kind === "error")) readStep(number);
  };

  const restore = (entry: Snapshot) => {
    setAsking(null);
    setRestoring(true);
    setNotice(null);
    api
      .snapshotRestore(sessionId, entry.number)
      .then((value) => {
        const outcome = parseRestoreOutcome(value);
        setNotice(
          outcome === null
            ? { kind: "error", message: t("error.unknown") }
            : outcome.kind === "incomplete"
              ? { kind: "incomplete", message: outcome.message }
              : { kind: outcome.kind },
        );
      })
      .catch((e: unknown) => {
        setNotice({ kind: "error", message: errorText(e) });
      })
      .finally(() => {
        setRestoring(false);
        setChanged((value) => value + 1);
        onRestored();
      });
  };

  const load: ListLoad = answer?.load ?? { kind: "loading" };
  if (load.kind === "loading") {
    return (
      <p className="changes-message" role="status">
        {t("changes.loading")}
      </p>
    );
  }
  if (load.kind === "error") {
    return (
      <p className="changes-message error-text selectable" role="alert">
        {t("timeline.failed", { error: load.message })}
      </p>
    );
  }
  const { list } = load;
  if (!list.isRepo) return <p className="changes-message">{t("changes.notRepo")}</p>;
  const restorable = canRestore(list, working);

  return (
    <div className="changes-body">
      {notice !== null && <NoticeLine notice={notice} />}
      {list.skipped !== null && (
        <p className="changes-hint selectable" role="status">
          {skippedText(list.skipped, t)}
        </p>
      )}
      {list.snapshots.length === 0 ? (
        <p className="changes-message">{t("timeline.none")}</p>
      ) : (
        <ul className="commit-list">
          {list.snapshots.map((entry) => (
            <StepItem
              key={entry.number}
              entry={entry}
              open={expanded.has(entry.number)}
              detail={details.get(entry.number)}
              age={commitAge(entry.createdAt, now, language)}
              date={commitDate(entry.createdAt, language)}
              restoring={restoring}
              restorable={restorable}
              onToggle={() => {
                toggle(entry.number);
              }}
              onRestore={() => {
                setAsking(entry);
                // The other sessions in the folder may have changed since the list was read.
                setChanged((value) => value + 1);
              }}
            />
          ))}
        </ul>
      )}
      {asking !== null && (
        <RestoreDialog
          others={list.others}
          restorable={restorable}
          onCancel={() => {
            setAsking(null);
          }}
          onConfirm={() => {
            restore(asking);
          }}
        />
      )}
    </div>
  );
}

function skippedText(skipped: Skipped, t: Translate): string {
  switch (skipped.reason) {
    case "tooManyFiles":
      return t("timeline.skippedFiles", { limit: skipped.limit });
    case "tooLarge":
      return t("timeline.skippedLarge", { file: skipped.fileMib, total: skipped.totalMib });
    case "failed":
      return t("timeline.skippedFailed", { error: skipped.message });
  }
}

function NoticeLine({ notice }: { notice: Notice }) {
  const t = useT();
  switch (notice.kind) {
    case "restored":
      return (
        <p className="changes-hint" role="status">
          {t("timeline.restored")}
        </p>
      );
    case "alreadyThere":
      return (
        <p className="changes-hint" role="status">
          {t("timeline.alreadyThere")}
        </p>
      );
    case "incomplete":
      return (
        <p className="changes-hint error-text selectable" role="alert">
          {t("timeline.restoreIncomplete", { error: notice.message })}
        </p>
      );
    case "error":
      return (
        <p className="changes-hint error-text selectable" role="alert">
          {t("timeline.restoreFailed", { error: notice.message })}
        </p>
      );
  }
}

function StepItem({
  entry,
  open,
  detail,
  age,
  date,
  restoring,
  restorable,
  onToggle,
  onRestore,
}: {
  entry: Snapshot;
  open: boolean;
  detail: DetailLoad | undefined;
  age: string;
  date: string;
  restoring: boolean;
  /** No agent works in the folder; otherwise the button says why it waits. */
  restorable: boolean;
  onToggle: () => void;
  onRestore: () => void;
}) {
  const t = useT();
  const title = snapshotTitle(entry);
  const text =
    title.kind === "prompt"
      ? title.text
      : title.kind === "undo"
        ? t("timeline.undo")
        : t("timeline.step");
  const tooltip = [
    title.kind === "prompt" ? revealInvisible(entry.prompt) : text,
    `${t("changes.files", { count: entry.files })} · ${date}`,
  ].join("\n");
  return (
    <li className={open ? "commit open" : "commit"}>
      <button
        type="button"
        className="commit-head"
        aria-expanded={open}
        title={tooltip}
        onClick={onToggle}
      >
        <ChevronIcon size={12} />
        <span className="commit-text">
          <span className={title.kind === "prompt" ? "commit-subject" : "commit-subject muted"}>
            {title.kind === "prompt" ? <Revealed text={text} /> : text}
          </span>
          <span className="commit-meta">
            <span>{t("changes.files", { count: entry.files })}</span>
            <span className="commit-age">{age}</span>
          </span>
        </span>
      </button>
      {open && (
        <div className="commit-detail">
          <button
            type="button"
            className="link-button timeline-restore"
            disabled={restoring || !restorable}
            onClick={onRestore}
          >
            {t("timeline.restore")}
          </button>
          {!restorable && <p className="commit-note">{t("timeline.restoreBusy")}</p>}
          <StepDetail detail={detail} />
        </div>
      )}
    </li>
  );
}

function StepDetail({ detail }: { detail: DetailLoad | undefined }) {
  const t = useT();
  if (detail === undefined || detail.kind === "loading") {
    return (
      <p className="change-note" role="status">
        {t("changes.loading")}
      </p>
    );
  }
  if (detail.kind === "error") {
    return (
      <p className="change-note error-text selectable" role="alert">
        {t("timeline.diffFailed", { error: detail.message })}
      </p>
    );
  }
  const { result } = detail;
  if (result.files.length === 0 && result.diff === "") {
    return <p className="commit-note">{t("timeline.empty")}</p>;
  }
  return <ChangeList result={result} />;
}

/** Says what going back does before anything is written. */
function RestoreDialog({
  others,
  restorable,
  onCancel,
  onConfirm,
}: {
  /** Other sessions in the same work tree: their files go back too. */
  others: number;
  /** An agent started working since the button was pressed: the confirmation waits. */
  restorable: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const t = useT();
  return (
    <Dialog labelledBy="timeline-restore-title" onClose={onCancel}>
      <h2 id="timeline-restore-title">{t("timeline.restoreTitle")}</h2>
      <p className="muted">{t("timeline.restoreBody")}</p>
      {others > 0 && <p>{t("timeline.restoreOthers", { count: others })}</p>}
      {!restorable && <p>{t("timeline.restoreBusy")}</p>}
      <div className="dialog-actions">
        <button type="button" className="secondary-button" data-autofocus onClick={onCancel}>
          {t("common.cancel")}
        </button>
        <button type="button" className="primary-button" disabled={!restorable} onClick={onConfirm}>
          {t("timeline.restoreConfirm")}
        </button>
      </div>
    </Dialog>
  );
}

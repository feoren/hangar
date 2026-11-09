import { useEffect, useMemo, useRef, useState } from "react";

import { api, errorText } from "./api";
import type { Overlap, SessionClaims } from "./claims";
import { ClaimsSection } from "./ClaimsView";
import { folderName } from "./folders";
import {
  CHANGE_LABELS,
  changeEntries,
  DIFF_PAGE_LINES,
  FILE_PAGE,
  initiallyOpen,
  lineKind,
  parseGitDiff,
  splitPath,
  type ChangeEntry,
  type GitDiff,
} from "./gitDiff";
import { useT } from "./i18n";
import { ChevronIcon, RestartIcon } from "./icons";
import { hasInvisible, revealInvisible } from "./invisible";
import { Revealed } from "./Revealed";

interface Props {
  /** Working folder of the active session; `null` without a session. */
  cwd: string | null;
  /**
   * Changes when the diff should be read again: the active session's attention state (an agent
   * that finished a step usually changed files).
   */
  trigger: string;
  /** Goes up when the panel should take the focus (opened via shortcut or menu). */
  focusToken: number;
  /** The active session's claims (claims.ts); `null` while off or not known yet. */
  claims: SessionClaims | null;
  /** Its collisions with other sessions, worst first. */
  overlaps: readonly Overlap[];
  nameOf: (id: number) => string;
  onShowSession: (id: number) => void;
}

type Load =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "done"; result: GitDiff };

/**
 * Changes of the active session's folder against the last commit, the second tab of the right panel
 * (Ctrl+Shift+D). Read again on open, on a new session, on "Refresh" and whenever the session's
 * attention state changes. Large diffs show a page of lines per file, more on request.
 */
export function ChangesPanel(props: Props) {
  const { cwd, trigger } = props;
  const t = useT();
  const [manual, setManual] = useState(0);
  /** Last answer, with the request it belongs to. */
  const [answer, setAnswer] = useState<{ key: string; cwd: string; load: Load } | null>(null);
  const panel = useRef<HTMLDivElement>(null);
  const latest = useRef("");
  const key = `${cwd ?? ""}\n${trigger}\n${String(manual)}`;

  useEffect(() => {
    latest.current = key;
    if (cwd === null) return;
    const settle = (load: Load) => {
      // An older answer that arrives late must not replace a newer one.
      if (latest.current === key) setAnswer({ key, cwd, load });
    };
    api
      .gitDiff(cwd)
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
    // `t` only changes with the language; a new language needs no new git call.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  // While reading again, the previous answer of the same folder stays visible (no flicker); a new
  // folder starts empty instead of showing another folder's changes.
  const load: Load =
    cwd === null
      ? { kind: "idle" }
      : answer !== null && answer.cwd === cwd
        ? answer.load
        : { kind: "loading" };
  const refreshing = cwd !== null && answer?.key !== key;

  useEffect(() => {
    if (props.focusToken === 0) return;
    // The panel itself, so no button shows a focus ring; Tab goes on to Refresh and the files.
    panel.current?.focus();
  }, [props.focusToken]);

  return (
    <div ref={panel} tabIndex={-1} className="changes">
      <div className="changes-header">
        <p className="changes-title">
          {cwd !== null ? (
            <span className="changes-folder" title={revealInvisible(cwd)}>
              <Revealed text={folderName(cwd)} />
            </span>
          ) : (
            <span className="changes-folder">{t("changes.title")}</span>
          )}
        </p>
        <button
          type="button"
          className={refreshing ? "icon-button changes-refresh spinning" : "icon-button"}
          aria-label={t("changes.refresh")}
          title={t("changes.refresh")}
          disabled={cwd === null}
          onClick={() => {
            setManual((value) => value + 1);
          }}
        >
          <RestartIcon />
        </button>
      </div>
      <ClaimsSection
        key={props.claims?.sessionId ?? "none"}
        claims={props.claims}
        overlaps={props.overlaps}
        nameOf={props.nameOf}
        onShowSession={props.onShowSession}
      />
      <ChangesBody cwd={cwd} load={load} />
    </div>
  );
}

function ChangesBody({ cwd, load }: { cwd: string | null; load: Load }) {
  const t = useT();
  if (cwd === null) return <p className="changes-message">{t("changes.noSession")}</p>;
  switch (load.kind) {
    case "idle":
    case "loading":
      return (
        <p className="changes-message" role="status">
          {t("changes.loading")}
        </p>
      );
    case "error":
      return (
        <p className="changes-message error-text selectable" role="alert">
          {t("changes.failed", { error: load.message })}
        </p>
      );
    case "done":
      if (!load.result.isRepo) {
        return <p className="changes-message">{t("changes.notRepo")}</p>;
      }
      if (load.result.files.length === 0 && load.result.diff === "") {
        return <p className="changes-message">{t("changes.none")}</p>;
      }
      return <ChangeList result={load.result} />;
  }
}

function ChangeList({ result }: { result: GitDiff }) {
  const t = useT();
  const entries = useMemo(() => changeEntries(result), [result]);
  const [open, setOpen] = useState(() => initiallyOpen(entries));
  // A new answer opens its small files again; files the user opened or closed keep their state.
  const [openFor, setOpenFor] = useState(entries);
  if (openFor !== entries) {
    setOpenFor(entries);
    const known = new Set(openFor.map((entry) => entry.path));
    const fresh = initiallyOpen(entries.filter((entry) => !known.has(entry.path)));
    setOpen((current) => new Set([...current, ...fresh]));
  }
  // Thousands of files (a generated folder) would make the panel slow; the totals count them all.
  const [listed, setListed] = useState(FILE_PAGE);
  const restFiles = entries.length - listed;
  const additions = entries.reduce((sum, entry) => sum + entry.additions, 0);
  const deletions = entries.reduce((sum, entry) => sum + entry.deletions, 0);
  return (
    <div className="changes-body">
      <p className="changes-summary">
        <span>{t("changes.files", { count: entries.length })}</span>
        <span className="changes-plus">+{additions}</span>
        <span className="changes-minus">−{deletions}</span>
      </p>
      {result.truncated && <p className="changes-hint">{t("changes.truncated")}</p>}
      <ul className="changes-files">
        {entries.slice(0, listed).map((entry) => (
          <FileItem
            key={entry.path}
            entry={entry}
            open={open.has(entry.path)}
            truncated={result.truncated}
            onToggle={() => {
              setOpen((current) => {
                const next = new Set(current);
                if (next.has(entry.path)) next.delete(entry.path);
                else next.add(entry.path);
                return next;
              });
            }}
          />
        ))}
      </ul>
      {restFiles > 0 && (
        <button
          type="button"
          className="link-button change-more files"
          onClick={() => {
            setListed((value) => value + FILE_PAGE);
          }}
        >
          {t("changes.moreFiles", { count: restFiles })}
        </button>
      )}
    </div>
  );
}

function FileItem({
  entry,
  open,
  truncated,
  onToggle,
}: {
  entry: ChangeEntry;
  open: boolean;
  truncated: boolean;
  onToggle: () => void;
}) {
  const t = useT();
  const { folder, name } = splitPath(entry.path);
  const label = CHANGE_LABELS[entry.kind];
  const kindName = t(label.name);
  return (
    <li className={open ? "change-file open" : "change-file"}>
      <button
        type="button"
        className="change-head"
        aria-expanded={open}
        title={[revealInvisible(entry.path), kindName, entry.hidden ? t("changes.hiddenChars") : ""]
          .filter(Boolean)
          .join("\n")}
        onClick={onToggle}
      >
        <ChevronIcon size={12} />
        <span className={`change-kind ${entry.kind}`} aria-hidden="true">
          {label.letter}
        </span>
        <span className="sr-only">{kindName}</span>
        <span className="change-name">
          <Revealed text={name} />
        </span>
        {folder !== "" && (
          <span className="change-folder">
            <Revealed text={folder} />
          </span>
        )}
        {entry.hidden && (
          <span className="change-warning" title={t("changes.hiddenChars")}>
            {t("changes.hiddenShort")}
          </span>
        )}
        <span className="change-counts">
          {entry.additions > 0 && <span className="changes-plus">+{entry.additions}</span>}
          {entry.deletions > 0 && <span className="changes-minus">−{entry.deletions}</span>}
        </span>
      </button>
      {open && <FileDiffView entry={entry} truncated={truncated} />}
    </li>
  );
}

function FileDiffView({ entry, truncated }: { entry: ChangeEntry; truncated: boolean }) {
  const t = useT();
  const [shown, setShown] = useState(DIFF_PAGE_LINES);
  const diff = entry.diff;
  if (diff === null) {
    const note =
      entry.kind === "untracked"
        ? t("changes.untrackedNote")
        : truncated
          ? t("changes.cutNote")
          : t("changes.noContent");
    return <p className="change-note">{note}</p>;
  }
  if (diff.binary) return <p className="change-note">{t("changes.binary")}</p>;
  if (diff.lines.length === 0) return <p className="change-note">{t("changes.noContent")}</p>;
  const rest = diff.lines.length - shown;
  return (
    <>
      <div className="diff">
        {diff.lines.slice(0, shown).map((line, index) => (
          // Lines have no identity of their own; the list only grows at the end.
          <div
            key={index}
            className={
              hasInvisible(line)
                ? `diff-line ${lineKind(line)} hidden-chars`
                : `diff-line ${lineKind(line)}`
            }
            title={hasInvisible(line) ? t("changes.hiddenChars") : undefined}
          >
            {line === "" ? " " : <Revealed text={line} />}
          </div>
        ))}
      </div>
      {rest > 0 && (
        <button
          type="button"
          className="link-button change-more"
          onClick={() => {
            setShown((value) => value + DIFF_PAGE_LINES);
          }}
        >
          {t("changes.more", { count: rest })}
        </button>
      )}
    </>
  );
}

import { useEffect, useId, useMemo, useRef, useState } from "react";

import { diffKey, newLineNumbers, stageAction, stageState } from "./gitActions";

import {
  CHANGE_LABELS,
  changeEntries,
  DIFF_PAGE_LINES,
  FILE_PAGE,
  isFileOpen,
  NO_CHOICES,
  toggleFile,
  type OpenChoices,
  lineKind,
  splitPath,
  type ChangeEntry,
  type GitDiff,
} from "./gitDiff";
import { useT } from "./i18n";
import { ChevronIcon, CommentIcon, DiscardIcon } from "./icons";
import { hasInvisible, revealInvisible } from "./invisible";
import { Revealed } from "./Revealed";

// The file list with its diffs, shared by the work tree's changes and a commit's changes in the
// changes panel: the same rows, the same diff view, the same "only the last file opens" rule.
// Only the work tree's changes pass `actions` (stage box and discard per file) and `onComment`
// (a comment on a diff line for the agent).

/** Stage box and discard button of each file; the panel runs the backend calls. */
export interface FileActions {
  /** An action runs: boxes and buttons wait for it. */
  busy: boolean;
  onStage: (entry: ChangeEntry, stage: boolean) => void;
  /** The panel asks before anything is thrown away. */
  onDiscard: (entry: ChangeEntry) => void;
}

/** A comment on line `line` (new side) of `path`, as typed. */
export type LineCommentHandler = (path: string, line: number, comment: string) => void;

interface Props {
  result: GitDiff;
  actions?: FileActions | undefined;
  onComment?: LineCommentHandler | undefined;
}

/** Totals, the files and their diffs; the caller provides the scrolling frame. */
export function ChangeList({ result, actions, onComment }: Props) {
  const t = useT();
  const entries = useMemo(() => changeEntries(result), [result]);
  // Every file starts closed; files the user opened stay open across refreshes.
  const [choices, setChoices] = useState<OpenChoices>(NO_CHOICES);
  // Thousands of files (a generated folder) would make the panel slow; the totals count them all.
  const [listed, setListed] = useState(FILE_PAGE);
  const restFiles = entries.length - listed;
  const additions = entries.reduce((sum, entry) => sum + entry.additions, 0);
  const deletions = entries.reduce((sum, entry) => sum + entry.deletions, 0);
  return (
    <>
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
            open={isFileOpen(entry.path, choices)}
            truncated={result.truncated}
            actions={actions}
            onComment={onComment}
            onToggle={() => {
              setChoices((current) => toggleFile(current, entry.path));
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
    </>
  );
}

function FileItem({
  entry,
  open,
  truncated,
  actions,
  onComment,
  onToggle,
}: {
  entry: ChangeEntry;
  open: boolean;
  truncated: boolean;
  actions: FileActions | undefined;
  onComment: LineCommentHandler | undefined;
  onToggle: () => void;
}) {
  const t = useT();
  const { folder, name } = splitPath(entry.path);
  const label = CHANGE_LABELS[entry.kind];
  const kindName = t(label.name);
  return (
    <li className={open ? "change-file open" : "change-file"}>
      <div className="change-row">
        {actions && <StageBox entry={entry} actions={actions} />}
        <button
          type="button"
          className="change-head"
          aria-expanded={open}
          title={[
            revealInvisible(entry.path),
            kindName,
            entry.hidden ? t("changes.hiddenChars") : "",
          ]
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
        {actions && (
          <button
            type="button"
            className="icon-button change-discard"
            aria-label={t("changes.discard")}
            title={t("changes.discard")}
            disabled={actions.busy}
            onClick={() => {
              actions.onDiscard(entry);
            }}
          >
            <DiscardIcon size={14} />
          </button>
        )}
      </div>
      {open && <FileDiffView entry={entry} truncated={truncated} onComment={onComment} />}
    </li>
  );
}

/** Checked: all changes staged; mixed: some of them; empty: none (untracked files too). */
function StageBox({ entry, actions }: { entry: ChangeEntry; actions: FileActions }) {
  const t = useT();
  const state = stageState(entry.status);
  const box = useRef<HTMLInputElement>(null);
  useEffect(() => {
    // `indeterminate` exists only as a property, not as an attribute React could set.
    if (box.current) box.current.indeterminate = state === "partial";
  }, [state]);
  const label = state === "staged" ? t("changes.unstage") : t("changes.stage");
  return (
    <input
      ref={box}
      type="checkbox"
      className="change-stage"
      checked={state === "staged"}
      // Not `disabled`: that would drop the focus while the action runs, and keyboard users
      // would lose their place in the list.
      aria-disabled={actions.busy}
      aria-label={label}
      title={label}
      onChange={() => {
        if (actions.busy) return;
        actions.onStage(entry, stageAction(state) === "stage");
      }}
    />
  );
}

function FileDiffView({
  entry,
  truncated,
  onComment,
}: {
  entry: ChangeEntry;
  truncated: boolean;
  onComment: LineCommentHandler | undefined;
}) {
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
      <DiffLines lines={diff.lines} shown={shown} path={entry.path} onComment={onComment} />
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

/** The lines of one file's diff; with `onComment`, added and unchanged lines take a comment. */
function DiffLines({
  lines,
  shown,
  path,
  onComment,
}: {
  lines: readonly string[];
  shown: number;
  path: string;
  onComment: LineCommentHandler | undefined;
}) {
  const t = useT();
  const numbers = useMemo(() => (onComment ? newLineNumbers(lines) : null), [lines, onComment]);
  /** Index of the line whose comment field is open. */
  const [commenting, setCommenting] = useState<number | null>(null);
  /** Keyboard cursor: the line Enter or `c` comments on while the diff has the focus. */
  const [cursor, setCursor] = useState<number | null>(null);
  const frame = useRef<HTMLDivElement>(null);
  const ids = useId();
  const lineId = (index: number) => `${ids}-line-${String(index)}`;
  const close = () => {
    setCommenting(null);
    frame.current?.focus();
  };
  if (!numbers || !onComment) {
    return (
      <div className="diff">
        {lines.slice(0, shown).map((line, index) => (
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
    );
  }
  return (
    <div
      ref={frame}
      className="diff commentable"
      // One Tab stop for the whole diff; arrows pick a line (diffKey in gitActions.ts).
      tabIndex={0}
      role="group"
      aria-label={t("changes.diffKeys")}
      aria-activedescendant={cursor === null ? undefined : lineId(cursor)}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return;
        if (event.ctrlKey || event.altKey || event.metaKey) return;
        const result = diffKey(event.key, cursor, numbers, shown);
        if (result === null) return;
        event.preventDefault();
        if (result.kind === "move") {
          setCursor(result.index);
          document.getElementById(lineId(result.index))?.scrollIntoView({ block: "nearest" });
        } else {
          setCommenting(result.index);
        }
      }}
    >
      {lines.slice(0, shown).map((line, index) => {
        const number = numbers[index] ?? null;
        const className = hasInvisible(line)
          ? `diff-line ${lineKind(line)} hidden-chars`
          : `diff-line ${lineKind(line)}`;
        const title = hasInvisible(line) ? t("changes.hiddenChars") : undefined;
        const text = line === "" ? " " : <Revealed text={line} />;
        if (number === null) {
          // Lines have no identity of their own; the list only grows at the end.
          return (
            <div key={index} className={className} title={title}>
              {text}
            </div>
          );
        }
        const open = () => {
          setCursor(index);
          setCommenting(index);
        };
        return (
          <div key={index} className="diff-row">
            <div
              id={lineId(index)}
              className={
                index === cursor ? `${className} commentable cursor` : `${className} commentable`
              }
              title={title}
              onClick={() => {
                // Selecting text to copy it is no request to comment.
                if (window.getSelection()?.isCollapsed === false) return;
                open();
              }}
            >
              <button
                type="button"
                className="diff-comment-button"
                // Hundreds of lines would each be a Tab stop; a click on the line works too.
                tabIndex={-1}
                aria-label={t("changes.comment")}
                title={t("changes.comment")}
                onClick={(event) => {
                  event.stopPropagation();
                  open();
                }}
              >
                <CommentIcon size={12} />
              </button>
              {text}
            </div>
            {commenting === index && (
              <CommentField
                line={number}
                onSend={(comment) => {
                  onComment(path, number, comment);
                  setCommenting(null);
                }}
                onCancel={close}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

/** One line for a comment: Enter sends it, Escape closes the field. */
function CommentField({
  line,
  onSend,
  onCancel,
}: {
  line: number;
  onSend: (text: string) => void;
  onCancel: () => void;
}) {
  const t = useT();
  const [text, setText] = useState("");
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    input.current?.focus();
  }, []);
  const placeholder = t("changes.commentPlaceholder", { line: String(line) });
  return (
    <div className="diff-comment-row">
      <input
        ref={input}
        type="text"
        className="diff-comment-input"
        value={text}
        placeholder={placeholder}
        aria-label={placeholder}
        onChange={(event) => {
          setText(event.target.value);
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter" && !event.nativeEvent.isComposing) {
            event.preventDefault();
            if (text.trim() !== "") onSend(text);
          } else if (event.key === "Escape") {
            // The panel would close or hand the focus to the terminal otherwise.
            event.preventDefault();
            event.stopPropagation();
            onCancel();
          }
        }}
      />
    </div>
  );
}

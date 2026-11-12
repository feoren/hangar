import { useMemo, useState } from "react";

import {
  CHANGE_LABELS,
  changeEntries,
  DIFF_PAGE_LINES,
  FILE_PAGE,
  autoOpenPath,
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
import { ChevronIcon } from "./icons";
import { hasInvisible, revealInvisible } from "./invisible";
import { Revealed } from "./Revealed";

// The file list with its diffs, shared by the work tree's changes and a commit's changes in the
// changes panel: the same rows, the same diff view, the same "only the last file opens" rule.

/** Totals, the files and their diffs; the caller provides the scrolling frame. */
export function ChangeList({ result }: { result: GitDiff }) {
  const t = useT();
  const entries = useMemo(() => changeEntries(result), [result]);
  // Only the last file opens by itself; files the user opened or closed keep their state.
  const auto = useMemo(() => autoOpenPath(entries), [entries]);
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
            open={isFileOpen(entry.path, auto, choices)}
            truncated={result.truncated}
            onToggle={() => {
              setChoices((current) =>
                toggleFile(current, entry.path, isFileOpen(entry.path, auto, current)),
              );
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

import { useEffect, useRef, useState } from "react";

import { api, errorText } from "./api";
import { Dialog } from "./Dialog";
import { relativeTime } from "./format";
import { useT } from "./i18n";
import { CloseIcon } from "./icons";
import { currentPlatform, revealLabelKey } from "./platform";
import {
  cleanupResult,
  entryAction,
  folderName,
  groupByRepository,
  lastChange,
  parseCleanupEntries,
  parseWorktreeSize,
  safeEntries,
  sizeText,
  stateHint,
  stateLabel,
  type CleanupEntry,
  type CleanupResult,
  type WorktreeSize,
} from "./worktreeCleanup";

interface Props {
  /** Folders whose repositories are searched too (`cleanupFolders`). */
  folders: readonly string[];
  language: string;
  onClose: () => void;
}

/**
 * "Clean up worktrees": the worktrees Hangar created, per repository, with their state, size and
 * last change. Only safe ones can be removed (one by one or all at once); the backend checks each
 * again first. The others can be shown in the file manager.
 */
export function WorktreeCleanupDialog({ folders, language, onClose }: Props) {
  const t = useT();
  const [entries, setEntries] = useState<CleanupEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sizes, setSizes] = useState<ReadonlyMap<string, WorktreeSize>>(() => new Map());
  const [busy, setBusy] = useState<ReadonlySet<string>>(() => new Set());
  const [notices, setNotices] = useState<ReadonlyMap<string, string>>(() => new Map());
  const [summary, setSummary] = useState<string | null>(null);
  const folderList = folders.join("\n");

  useEffect(() => {
    let live = true;
    api
      .listHangarWorktrees(folderList === "" ? [] : folderList.split("\n"))
      .then((value) => {
        if (!live) return;
        const parsed = parseCleanupEntries(value);
        if (parsed === null) setError(t("cleanup.loadFailed", { error: t("error.unknown") }));
        else setEntries(parsed);
      })
      .catch((e: unknown) => {
        if (live) setError(t("cleanup.loadFailed", { error: errorText(e) }));
      });
    return () => {
      live = false;
    };
  }, [folderList, t]);

  // Sizes one after another in the background, so the disk is not walked many times at once.
  const measurable = (entries ?? [])
    .filter((entry) => entry.state.kind !== "missing")
    .map((entry) => entry.path)
    .join("\n");
  const asked = useRef(new Set<string>());
  useEffect(() => {
    const paths = measurable === "" ? [] : measurable.split("\n");
    const run = { live: true };
    void (async () => {
      for (const path of paths) {
        if (!run.live) return;
        // Each folder once; a row that changes state keeps the size it had.
        if (asked.current.has(path)) continue;
        asked.current.add(path);
        try {
          const size = parseWorktreeSize(await api.hangarWorktreeSize(path));
          if (size !== null) setSizes((current) => new Map(current).set(path, size));
        } catch {
          // No size is shown; the row stays usable.
        }
      }
    })();
    return () => {
      run.live = false;
    };
  }, [measurable]);

  // A removed row or a disabled button takes the focus with it; keep it in the dialog, so Tab
  // and Escape still reach it.
  const title = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    const active = document.activeElement;
    if (active === null || active === document.body) {
      title.current?.closest<HTMLElement>('[role="dialog"]')?.focus();
    }
  }, [entries, busy]);

  const setNotice = (path: string, notice: string | null) => {
    setNotices((current) => {
      const next = new Map(current);
      if (notice === null) next.delete(path);
      else next.set(path, notice);
      return next;
    });
  };

  const setBusyPath = (path: string, on: boolean) => {
    setBusy((current) => {
      const next = new Set(current);
      if (on) next.add(path);
      else next.delete(path);
      return next;
    });
  };

  /** Removes or prunes one entry; the backend decides with the state it checks itself. */
  const cleanUp = async (entry: CleanupEntry): Promise<CleanupResult | null> => {
    const action = entryAction(entry);
    if ((action !== "remove" && action !== "prune") || entry.repoRoot === null) return null;
    try {
      const value =
        action === "remove"
          ? await api.removeHangarWorktree(entry.path, entry.repoRoot)
          : await api.pruneHangarWorktree(entry.path, entry.repoRoot);
      return cleanupResult(entry, value, t);
    } catch (e: unknown) {
      return { done: false, entry, notice: t("cleanup.removeFailed", { error: errorText(e) }) };
    }
  };

  const apply = (path: string, result: CleanupResult) => {
    if (result.done) {
      setEntries((current) => current?.filter((entry) => entry.path !== path) ?? null);
      setNotice(path, null);
      return;
    }
    setEntries(
      (current) => current?.map((entry) => (entry.path === path ? result.entry : entry)) ?? null,
    );
    setNotice(path, result.notice === "" ? null : result.notice);
  };

  const runOne = async (entry: CleanupEntry) => {
    setSummary(null);
    setBusyPath(entry.path, true);
    const result = await cleanUp(entry);
    setBusyPath(entry.path, false);
    if (result === null) return;
    apply(entry.path, result);
    if (result.done) setSummary(result.notice);
  };

  const runAll = async () => {
    const targets = safeEntries(entries ?? []);
    setSummary(null);
    setBusy(new Set(targets.map((entry) => entry.path)));
    let removed = 0;
    for (const entry of targets) {
      const result = await cleanUp(entry);
      setBusyPath(entry.path, false);
      if (result === null) continue;
      apply(entry.path, result);
      if (result.done) removed += 1;
    }
    setSummary(t("cleanup.removedCount", { count: removed }));
  };

  const reveal = (entry: CleanupEntry) => {
    setNotice(entry.path, null);
    api.openPath(entry.path).catch((e: unknown) => {
      setNotice(entry.path, errorText(e));
    });
  };

  const [now] = useState(() => Date.now());
  const safe = safeEntries(entries ?? []);
  const working = busy.size > 0;

  return (
    <Dialog labelledBy="cleanup-title" className="worktree-cleanup" onClose={onClose}>
      <div className="dialog-header">
        <h2 id="cleanup-title" ref={title}>
          {t("cleanup.title")}
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
      <p className="muted">{t("cleanup.intro")}</p>
      {error !== null && (
        <p className="error-text" role="alert">
          {error}
        </p>
      )}
      {error === null && entries === null && (
        <p className="muted" role="status">
          {t("cleanup.loading")}
        </p>
      )}
      {entries !== null && entries.length === 0 && <p>{t("cleanup.empty")}</p>}
      {summary !== null && (
        <p className="cleanup-summary" role="status">
          {summary}
        </p>
      )}
      {entries !== null && entries.length > 0 && (
        <div className="cleanup-list">
          {groupByRepository(entries).map((group) => (
            <section key={group.key} className="cleanup-group">
              <h3 className="cleanup-repo" title={group.repoRoot ?? undefined}>
                {group.name ?? t("cleanup.unknownRepo")}
              </h3>
              <ul>
                {group.entries.map((entry) => {
                  const action = entryAction(entry);
                  const size = sizes.get(entry.path);
                  const changed = lastChange(entry, size);
                  const meta = [
                    sizeText(size, t, language),
                    changed === null
                      ? ""
                      : t("cleanup.modified", { time: relativeTime(changed, now, language) }),
                  ].filter((part) => part !== "");
                  const notice = notices.get(entry.path);
                  const pending = busy.has(entry.path);
                  return (
                    <li key={entry.path} className="cleanup-row">
                      <div className="cleanup-main">
                        <div className="cleanup-line">
                          <span className="cleanup-name selectable" title={entry.path}>
                            {folderName(entry)}
                          </span>
                          <span className="cleanup-state" data-state={entry.state.kind}>
                            {stateLabel(entry.state, t)}
                          </span>
                        </div>
                        <div className="cleanup-hint">{stateHint(entry.state, t)}</div>
                        {meta.length > 0 && <div className="cleanup-meta">{meta.join(" · ")}</div>}
                        {notice !== undefined && (
                          <div className="cleanup-notice" role="status">
                            {notice}
                          </div>
                        )}
                      </div>
                      {action === "reveal" && (
                        <button
                          type="button"
                          className="secondary-button"
                          onClick={() => {
                            reveal(entry);
                          }}
                        >
                          {t(revealLabelKey(currentPlatform()))}
                        </button>
                      )}
                      {(action === "remove" || action === "prune") && (
                        <button
                          type="button"
                          className="secondary-button"
                          disabled={working}
                          aria-busy={pending}
                          onClick={() => void runOne(entry)}
                        >
                          {action === "remove" ? t("cleanup.remove") : t("cleanup.prune")}
                        </button>
                      )}
                    </li>
                  );
                })}
              </ul>
            </section>
          ))}
        </div>
      )}
      <div className="dialog-actions">
        <button type="button" className="secondary-button" onClick={onClose}>
          {t("common.close")}
        </button>
        {safe.length > 1 && (
          <button
            type="button"
            className="primary-button"
            disabled={working}
            onClick={() => void runAll()}
          >
            {t("cleanup.removeAll", { count: safe.length })}
          </button>
        )}
      </div>
    </Dialog>
  );
}

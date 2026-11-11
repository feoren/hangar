import { useEffect, useMemo, useReducer, useRef, useState } from "react";

import { api, errorText, parseFolderListing } from "./api";
import { Avatar } from "./Avatar";
import { isFileLevel, type ChangeKind, type ClaimsSnapshot } from "./claims";
import {
  avatarStack,
  buildMarks,
  CHANGE_LETTERS,
  folderKeyOf,
  foldersToRefresh,
  INITIAL_TREE,
  isBusyError,
  peersOf,
  treeKey,
  treeReducer,
  treeStep,
  visibleRows,
  type FileMark,
  type FolderMark,
  type HintRow,
  type ItemRow,
  type TeamEditor,
  type TreeAction,
  type TreeState,
} from "./fileTree";
import { folderName } from "./folders";
import { useT, type MessageKey, type Translate } from "./i18n";
import { ChevronIcon, FileIcon, FolderIcon, LinkIcon, OverlapIcon, RestartIcon } from "./icons";
import { revealInvisible } from "./invisible";
import { Revealed } from "./Revealed";
import { personTag, type TeamActivity } from "./team";

/** A listing refused because the same folder is still being read is asked for once more after this. */
const BUSY_RETRY_MS = 1200;
/** A change of the claims reads the open folders again after this quiet time ... */
const AUTO_REFRESH_DELAY_MS = 1500;
/** ... and never sooner than this after the previous reading. */
const AUTO_REFRESH_GAP_MS = 5000;
/** Indentation per level, in pixels; the guides sit on it. */
const INDENT = 12;
/** Avatars at the end of a file row before "+N". */
const MAX_AVATARS = 2;
/** Trees kept for sessions the panel showed before (expanded folders, focus). */
const MAX_SAVED_TREES = 24;

/** The tree of each session the panel showed, while the window lives. */
const savedTrees = new Map<number, TreeState>();

function saveTree(sessionId: number, state: TreeState): void {
  savedTrees.delete(sessionId);
  savedTrees.set(sessionId, state);
  for (const oldest of savedTrees.keys()) {
    if (savedTrees.size <= MAX_SAVED_TREES) break;
    savedTrees.delete(oldest);
  }
}

const CHANGE_NAMES: Record<ChangeKind, MessageKey> = {
  added: "changes.added",
  modified: "changes.modified",
  deleted: "changes.deleted",
  untracked: "changes.untracked",
  conflicted: "changes.conflict",
};

interface Props {
  /** The active session; `null` without one. */
  session: { id: number; cwd: string } | null;
  /** This device's claims: the worktree's changes, and when they change, a reason to read again. */
  claims: ClaimsSnapshot;
  /** Shared projects: the teammates whose files get an avatar. */
  activities: ReadonlyMap<string, TeamActivity>;
  nameOf: (id: number) => string;
  /** Goes up when the tree should take the focus (opened from the session row). */
  focusToken: number;
}

/**
 * The third tab of the right panel: the active session's folder as a tree, as in VS Code's
 * explorer. Folders are listed when opened; files teammates edit carry their avatars, files this
 * worktree changed a letter (M, A, U), and folders show a dot when something inside is marked.
 */
export function FilesPanel(props: Props) {
  const t = useT();
  if (props.session === null) {
    return (
      <div className="changes">
        <p className="changes-message">{t("files.noSession")}</p>
      </div>
    );
  }
  return <SessionTree key={props.session.id} {...props} session={props.session} />;
}

function SessionTree(props: Props & { session: { id: number; cwd: string } }) {
  const { session, claims, activities, nameOf } = props;
  const t = useT();
  const [state, dispatch] = useReducer(treeReducer, savedTrees.get(session.id) ?? INITIAL_TREE);
  const stateRef = useRef(state);
  useEffect(() => {
    stateRef.current = state;
    saveTree(session.id, state);
  }, [state, session.id]);
  const [refreshing, setRefreshing] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
  const items = useRef(new Map<string, HTMLLIElement>());
  /** The next render moves the DOM focus to the focused row (keyboard steps). */
  const wantFocus = useRef(false);

  const loader = useLoader(session.id, dispatch);

  // Open: read the root and the folders that were open, keeping what the saved tree shows.
  useEffect(() => {
    loader.start();
    loader.refresh(stateRef.current);
    return loader.dispose;
  }, [loader]);

  // The claims change when an agent changed files: read the open folders again, calmly.
  const firstClaims = useRef(true);
  useEffect(() => {
    if (firstClaims.current) {
      firstClaims.current = false;
      return;
    }
    loader.schedule(() => stateRef.current);
  }, [claims, loader]);

  useEffect(() => loader.onBusy(setRefreshing), [loader]);

  const rows = useMemo(() => visibleRows(state), [state]);
  const windows = folderKeyOf(state.root ?? session.cwd).windows;
  const marks = useMemo(() => {
    const overlaps = claims.overlaps
      .filter((o) => o.sessionId === session.id && o.path !== null && isFileLevel(o.level))
      .map((o) => ({ path: o.path ?? "", session: o.otherId }));
    return buildMarks({
      root: state.root,
      repoRoot: state.repoRoot,
      peers: peersOf(activities, session.id),
      changes: claims.sessions.map((entry) => ({ root: entry.root, claims: entry.claims })),
      overlaps,
    });
  }, [state.root, state.repoRoot, claims, activities, session.id]);

  const itemRows = rows.filter((row): row is ItemRow => row.kind === "item");
  const tabStop = itemRows.some((row) => row.path === state.focused)
    ? state.focused
    : (itemRows[0]?.path ?? null);

  useEffect(() => {
    if (!wantFocus.current || state.focused === null) return;
    wantFocus.current = false;
    const element = items.current.get(state.focused);
    element?.focus();
    element?.scrollIntoView({ block: "nearest" });
  }, [state.focused]);

  useEffect(() => {
    if (props.focusToken === 0) return;
    const element = tabStop === null ? null : items.current.get(tabStop);
    (element ?? panel.current)?.focus();
    // Only a new request moves the focus, not every new row.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.focusToken]);

  const expand = (path: string) => {
    dispatch({ type: "expand", path });
    // Opened for the first time, or reading it failed before: read it (again).
    const load = stateRef.current.folders.get(path);
    if (load === undefined || load.kind === "failed") loader.read(path);
  };
  const toggle = (row: ItemRow) => {
    if (row.expanded) dispatch({ type: "collapse", path: row.path });
    else expand(row.path);
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLUListElement>) => {
    if (event.ctrlKey || event.altKey || event.metaKey) return;
    const step = treeStep(rows, tabStop, event.key);
    if (step === null) return;
    event.preventDefault();
    switch (step.kind) {
      case "focus":
      case "select":
        wantFocus.current = true;
        dispatch({ type: "focus", path: step.path });
        break;
      case "expand":
        expand(step.path);
        break;
      case "collapse":
        dispatch({ type: "collapse", path: step.path });
        break;
      case "none":
        break;
    }
  };

  const root = state.folders.get("");
  const title = folderName(state.root ?? session.cwd);
  return (
    <div ref={panel} tabIndex={-1} className="changes files">
      <div className="changes-header">
        <p className="changes-title">
          <span className="changes-folder" title={revealInvisible(state.root ?? session.cwd)}>
            <Revealed text={title} />
          </span>
        </p>
        <button
          type="button"
          className={refreshing ? "icon-button changes-refresh spinning" : "icon-button"}
          aria-label={t("changes.refresh")}
          title={t("changes.refresh")}
          onClick={() => {
            loader.refresh(stateRef.current);
          }}
        >
          <RestartIcon />
        </button>
      </div>
      {root?.kind === "failed" ? (
        <p className="changes-message error-text selectable" role="alert">
          {t("files.failed", { error: root.message })}
        </p>
      ) : root === undefined || root.kind === "loading" ? (
        <p className="changes-message" role="status">
          {t("changes.loading")}
        </p>
      ) : (
        <ul
          className="file-tree"
          role="tree"
          aria-label={t("files.label", { folder: revealInvisible(title) })}
          onKeyDown={onKeyDown}
        >
          {rows.map((row) =>
            row.kind === "hint" ? (
              <TreeHint key={`${row.folder}\u0000${row.hint}`} row={row} />
            ) : (
              <TreeItem
                key={row.path}
                row={row}
                focused={row.path === tabStop}
                selected={row.path === state.focused}
                fileMark={marks.files.get(treeKey(row.path, windows)) ?? null}
                folderMark={marks.folders.get(treeKey(row.path, windows)) ?? null}
                nameOf={nameOf}
                setRef={(element) => {
                  if (element) items.current.set(row.path, element);
                  else items.current.delete(row.path);
                }}
                onClick={() => {
                  dispatch({ type: "focus", path: row.path });
                  if (row.entry.kind === "dir") toggle(row);
                }}
              />
            ),
          )}
        </ul>
      )}
    </div>
  );
}

/**
 * Reads folders for one session's tree: a busy folder once more after a short wait, a refresh of
 * the root and every open folder one after another, and automatic refreshes no closer together
 * than AUTO_REFRESH_GAP_MS. Answers arriving after the tree closed are dropped.
 */
function useLoader(sessionId: number, dispatch: React.Dispatch<TreeAction>) {
  const [loader] = useState(() => {
    /** Goes up on every start and stop: work of an earlier one ends quietly. */
    let generation = 0;
    let running = false;
    let again: TreeState | null = null;
    let lastRefresh = 0;
    let timer: number | null = null;
    let busyListener: ((busy: boolean) => void) | null = null;

    const wait = (ms: number) =>
      new Promise<void>((resolve) => {
        window.setTimeout(resolve, ms);
      });

    const once = async (path: string, retry: boolean): Promise<void> => {
      const mine = generation;
      try {
        const listing = parseFolderListing(await api.listSessionFolder(sessionId, path));
        if (mine !== generation) return;
        if (listing === null) dispatch({ type: "failed", path, message: errorText(null) });
        else dispatch({ type: "loaded", path, listing });
      } catch (error: unknown) {
        if (mine !== generation) return;
        const message = errorText(error);
        if (retry && isBusyError(message)) {
          await wait(BUSY_RETRY_MS);
          if (mine === generation) await once(path, false);
          return;
        }
        dispatch({ type: "failed", path, message });
      }
    };

    const read = (path: string) => {
      dispatch({ type: "request", path });
      void once(path, true);
    };

    const refresh = (state: TreeState) => {
      if (running) {
        again = state;
        return;
      }
      running = true;
      busyListener?.(true);
      lastRefresh = Date.now();
      const mine = generation;
      void (async () => {
        let current: TreeState | null = state;
        while (current !== null && mine === generation) {
          again = null;
          for (const path of foldersToRefresh(current)) {
            if (mine !== generation) break;
            dispatch({ type: "request", path });
            await once(path, true);
          }
          current = again;
        }
        if (mine !== generation) return;
        running = false;
        busyListener?.(false);
      })();
    };

    const schedule = (latest: () => TreeState) => {
      if (timer !== null) return;
      const delay = Math.max(AUTO_REFRESH_DELAY_MS, lastRefresh + AUTO_REFRESH_GAP_MS - Date.now());
      timer = window.setTimeout(() => {
        timer = null;
        refresh(latest());
      }, delay);
    };

    return {
      read,
      refresh,
      schedule,
      onBusy: (listener: (busy: boolean) => void) => {
        busyListener = listener;
        return () => {
          busyListener = null;
        };
      },
      start: () => {
        generation += 1;
        running = false;
        again = null;
      },
      dispose: () => {
        generation += 1;
        running = false;
        if (timer !== null) window.clearTimeout(timer);
        timer = null;
      },
    };
  });
  return loader;
}

function TreeHint({ row }: { row: HintRow }) {
  const t = useT();
  const text =
    row.hint === "loading"
      ? t("changes.loading")
      : row.hint === "failed"
        ? t("files.failed", { error: row.message })
        : row.hint === "empty"
          ? t("files.empty")
          : t("files.truncated");
  return (
    <li
      role="none"
      className={row.hint === "failed" ? "tree-hint error-text" : "tree-hint"}
      style={{ paddingLeft: rowIndent(row.depth) }}
    >
      {text}
    </li>
  );
}

/** Left padding of a row: the indentation plus the chevron's column for hints. */
function rowIndent(depth: number): number {
  return 8 + depth * INDENT + 16;
}

function TreeItem(props: {
  row: ItemRow;
  focused: boolean;
  selected: boolean;
  fileMark: FileMark | null;
  folderMark: FolderMark | null;
  nameOf: (id: number) => string;
  setRef: (element: HTMLLIElement | null) => void;
  onClick: () => void;
}) {
  const { row, focused, selected, fileMark, folderMark, nameOf, setRef, onClick } = props;
  const t = useT();
  const kind = row.entry.kind;
  const notes = rowNotes(row, fileMark, folderMark, nameOf, t);
  const className = ["tree-item", row.entry.ignored ? "ignored" : "", selected ? "selected" : ""]
    .filter(Boolean)
    .join(" ");
  return (
    <li
      ref={setRef}
      role="treeitem"
      className={className}
      aria-level={row.depth + 1}
      aria-setsize={row.siblings}
      aria-posinset={row.position}
      aria-expanded={kind === "dir" ? row.expanded : undefined}
      aria-selected={selected}
      tabIndex={focused ? 0 : -1}
      title={[revealInvisible(row.path), ...notes].join("\n")}
      onClick={onClick}
    >
      <span
        className="tree-indent"
        aria-hidden="true"
        style={{ width: row.depth * INDENT, backgroundSize: `${String(INDENT)}px 100%` }}
      />
      <span className={row.expanded ? "tree-chevron open" : "tree-chevron"} aria-hidden="true">
        {kind === "dir" && <ChevronIcon size={12} />}
      </span>
      <span className="tree-icon" aria-hidden="true">
        {kind === "dir" ? (
          <FolderIcon size={14} />
        ) : kind === "symlink" ? (
          <LinkIcon size={14} />
        ) : (
          <FileIcon size={14} />
        )}
      </span>
      <span className="tree-name">
        <Revealed text={row.name} />
      </span>
      {notes.length > 0 && <span className="sr-only">{notes.join(". ")}</span>}
      <span className="tree-marks" aria-hidden="true">
        {folderMark !== null && kind === "dir" && (
          <span className={folderMark.team ? "tree-dot team" : "tree-dot change"} />
        )}
        {fileMark !== null && fileMark.alsoIn.length > 0 && (
          <span className="tree-also">
            <OverlapIcon size={11} />
          </span>
        )}
        {fileMark !== null && fileMark.editors.length > 0 && <Avatars editors={fileMark.editors} />}
        {fileMark !== null && fileMark.change !== null && (
          <span className={`tree-change ${fileMark.change}`}>
            {CHANGE_LETTERS[fileMark.change]}
          </span>
        )}
      </span>
    </li>
  );
}

function Avatars({ editors }: { editors: readonly TeamEditor[] }) {
  const { shown, more } = avatarStack(editors, MAX_AVATARS);
  return (
    <span className="tree-people">
      {shown.map((editor) => (
        <Avatar
          key={`${editor.member}\u0000${String(editor.own)}`}
          name={editor.name}
          size="mini"
        />
      ))}
      {more > 0 && <span className="tree-more">+{more}</span>}
    </span>
  );
}

/** What the row's marks mean, for its tooltip and screen readers. */
function rowNotes(
  row: ItemRow,
  fileMark: FileMark | null,
  folderMark: FolderMark | null,
  nameOf: (id: number) => string,
  t: Translate,
): string[] {
  const notes: string[] = [];
  if (row.entry.kind === "symlink") notes.push(t("files.link"));
  if (row.entry.ignored) notes.push(t("files.ignored"));
  if (fileMark !== null) {
    if (fileMark.change !== null) notes.push(t(CHANGE_NAMES[fileMark.change]));
    for (const editor of fileMark.editors) {
      const person = personTag(editor, t);
      const agent = revealInvisible(editor.agent.trim());
      notes.push(
        agent === ""
          ? t("files.editedByPlain", { person })
          : t("files.editedBy", { person, agent }),
      );
    }
    for (const session of fileMark.alsoIn) {
      notes.push(t("files.alsoIn", { session: revealInvisible(nameOf(session)) }));
    }
  }
  if (folderMark !== null && row.entry.kind === "dir") {
    if (folderMark.team) notes.push(t("files.folderTeam"));
    if (folderMark.change) notes.push(t("files.folderChanged"));
  }
  return notes;
}

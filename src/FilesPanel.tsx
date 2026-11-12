import { useEffect, useMemo, useReducer, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { api, errorText, parseFolderListing, parsePathIndex, type PathIndex } from "./api";
import { Avatar } from "./Avatar";
import { isFileLevel, type ChangeKind, type ClaimsSnapshot } from "./claims";
import {
  avatarStack,
  buildMarks,
  CHANGE_LETTERS,
  filteredStep,
  filterTree,
  folderKeyOf,
  foldersToRefresh,
  INITIAL_TREE,
  isBusyError,
  loadedPaths,
  onChangePath,
  revealFolders,
  rowMention,
  startsFilter,
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
  type TreeRow,
  type TreeState,
} from "./fileTree";
import { peersOf } from "./repoPeers";
import { folderName } from "./folders";
import { useT, type MessageKey, type Translate } from "./i18n";
import {
  ChevronIcon,
  CloseIcon,
  FileIcon,
  FolderIcon,
  LinkIcon,
  OverlapIcon,
  RestartIcon,
  RevealFolderIcon,
  SearchIcon,
} from "./icons";
import { revealInvisible } from "./invisible";
import { Revealed } from "./Revealed";
import { beyondThreshold } from "./sessionDrag";
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
/** How long a failed opening stays in the panel. */
const OPEN_ERROR_MS = 6000;
/** Trees kept for sessions the panel showed before (expanded folders, focus). */
const MAX_SAVED_TREES = 24;
/** A revealed row is focused once it shows, if that happens within this time. */
const REVEAL_FOCUS_MS = 3000;

/**
 * A tree row dragged with the mouse: `move` while it travels, then `drop` where it was let go or
 * `cancel` (Escape, the pointer lost). Coordinates are CSS pixels of the window.
 */
export interface MentionDrag {
  phase: "move" | "drop" | "cancel";
  x: number;
  y: number;
  /** The row's mention, as the right click types it (`rowMention`). */
  text: string;
}

/** The path index for the filter, read once per filter. */
type IndexLoad =
  { kind: "idle" } | { kind: "loading" } | { kind: "ready"; index: PathIndex } | { kind: "failed" };

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
  /** Right click on a row: this text (`@path `) goes into the active session's terminal. */
  onMention: (text: string) => void;
  /**
   * Rows can be dragged onto a terminal pane (main window only; the detached panel has no
   * panes). Without it, rows do not drag.
   */
  onDragMention?: (drag: MentionDrag) => void;
}

/**
 * The third tab of the right panel: the active session's folder as a tree, as in VS Code's
 * explorer. Folders are listed when opened; files teammates edit carry their avatars, files this
 * worktree changed a letter (M, A, U), and folders show a dot when something inside is marked.
 * The names along the path to every changed file show in the "working" color. A double click or
 * Enter opens a file in a read-only viewer window.
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
  const [openError, setOpenError] = useState<string | null>(null);
  useEffect(() => {
    if (openError === null) return;
    const timer = window.setTimeout(() => {
      setOpenError(null);
    }, OPEN_ERROR_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [openError]);
  const openFile = (path: string) => {
    setOpenError(null);
    api.openFileViewer(session.id, path).catch((error: unknown) => {
      setOpenError(t("files.openFailed", { error: errorText(error) }));
    });
  };

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

  // Filter: what the tree has read answers at once, the full index (read once per filter) when
  // it is in.
  const [query, setQuery] = useState("");
  const [indexLoad, setIndexLoad] = useState<IndexLoad>({ kind: "idle" });
  const filtering = query.trim() !== "";
  const filterInput = useRef<HTMLInputElement>(null);
  /** Goes up whenever the index is dropped: an answer to an older request is ignored. */
  const indexRequest = useRef(0);
  /** Starting a filter, or a refresh while one runs, reads the index. */
  const readIndex = () => {
    const mine = (indexRequest.current += 1);
    setIndexLoad({ kind: "loading" });
    api
      .listSessionPaths(session.id)
      .then((value) => {
        if (mine !== indexRequest.current) return;
        const index = parsePathIndex(value);
        setIndexLoad(index === null ? { kind: "failed" } : { kind: "ready", index });
      })
      .catch(() => {
        if (mine === indexRequest.current) setIndexLoad({ kind: "failed" });
      });
  };
  /** The filter field shows while it holds text or has the focus; otherwise only its button. */
  const [filterOpen, setFilterOpen] = useState(false);
  /** The next render focuses the field (it was just opened and is not in the DOM yet). */
  const focusFilterNext = useRef(false);
  useEffect(() => {
    if (!focusFilterNext.current || filterInput.current === null) return;
    focusFilterNext.current = false;
    filterInput.current.focus();
  });
  const openFilter = () => {
    if (filterInput.current !== null) {
      filterInput.current.focus();
      return;
    }
    focusFilterNext.current = true;
    setFilterOpen(true);
  };
  const changeQuery = (next: string) => {
    const was = query.trim() !== "";
    const now = next.trim() !== "";
    setQuery(next);
    // Cleared from outside the field (reveal, Escape on the tree): the field closes.
    if (next === "" && document.activeElement !== filterInput.current) setFilterOpen(false);
    if (!now) {
      // Dropped with the filter: the next one reads the folder as it is then.
      indexRequest.current += 1;
      setIndexLoad({ kind: "idle" });
    } else if (!was) {
      readIndex();
    }
  };
  // Keyed on the folders, not the whole state: moving the focus must not filter again.
  const folders = state.folders;
  const source = useMemo(() => {
    if (!filtering) return null;
    return indexLoad.kind === "ready" ? indexLoad.index.entries : loadedPaths({ folders });
  }, [filtering, indexLoad, folders]);
  const filtered = useMemo(
    () => (source === null ? null : filterTree(source, query)),
    [source, query],
  );
  const rows: TreeRow[] = useMemo(() => filtered?.rows ?? visibleRows(state), [filtered, state]);

  /** A row revealed from the filter takes the focus once it shows (its folders may still load). */
  const revealFocus = useRef<string | null>(null);
  useEffect(() => {
    const pending = revealFocus.current;
    if (pending === null) return;
    const element = items.current.get(pending);
    if (!element) return;
    revealFocus.current = null;
    element.focus();
    element.scrollIntoView({ block: "nearest" });
  }, [rows]);

  /** Clears the filter and shows `path` in the whole tree, its folders open. */
  const reveal = (path: string, folder: boolean) => {
    for (const each of revealFolders(path, folder)) expand(each);
    changeQuery("");
    dispatch({ type: "focus", path });
    revealFocus.current = path;
    // A folder that never shows (it failed to load) must not take the focus much later.
    window.setTimeout(() => {
      if (revealFocus.current === path) revealFocus.current = null;
    }, REVEAL_FOCUS_MS);
  };

  const { canDrag, startDrag, justDropped, ghost, ghostRef } = useRowDrag(props.onDragMention);
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
    const modifiers = event.ctrlKey || event.altKey || event.metaKey;
    // Typing on the tree filters it: the character goes into the filter field.
    if (startsFilter(event.key, modifiers)) {
      event.preventDefault();
      changeQuery(query + event.key);
      openFilter();
      return;
    }
    if (event.key === "Escape" && filtering) {
      event.preventDefault();
      const row = itemRows.find((item) => item.path === tabStop);
      if (row) reveal(row.path, false);
      else changeQuery("");
      return;
    }
    if (modifiers) return;
    const step = filtering
      ? filteredStep(rows, tabStop, event.key)
      : treeStep(rows, tabStop, event.key);
    if (step === null) return;
    event.preventDefault();
    switch (step.kind) {
      case "focus":
      case "select":
        wantFocus.current = true;
        dispatch({ type: "focus", path: step.path });
        break;
      case "open":
        dispatch({ type: "focus", path: step.path });
        openFile(step.path);
        break;
      case "expand":
        expand(step.path);
        break;
      case "collapse":
        dispatch({ type: "collapse", path: step.path });
        break;
      case "reveal":
        reveal(step.path, true);
        break;
      case "none":
        break;
    }
  };

  const onFilterKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      if (query !== "") {
        changeQuery("");
        return;
      }
      // Empty: the field closes and the tree takes the focus back.
      setFilterOpen(false);
      const row = tabStop === null ? null : items.current.get(tabStop);
      (row ?? panel.current)?.focus();
      return;
    }
    if (event.key === "ArrowDown" || event.key === "Enter") {
      // Into the results: the first match (or the tree's first row).
      const first = itemRows.find((row) => row.context !== true) ?? itemRows[0];
      if (first === undefined) return;
      event.preventDefault();
      wantFocus.current = true;
      dispatch({ type: "focus", path: first.path });
      // Already focused in the state: the effect does not run, so focus the row here.
      if (state.focused === first.path) items.current.get(first.path)?.focus();
    }
  };

  const root = state.folders.get("");
  const title = folderName(state.root ?? session.cwd);
  const filterNote =
    filtered === null
      ? null
      : filtered.matched === 0
        ? indexLoad.kind === "loading"
          ? t("files.filterSearching")
          : t("files.filterNone")
        : filtered.matched > filtered.rows.filter((row) => row.context !== true).length
          ? t("files.filterMore")
          : indexLoad.kind === "ready" && indexLoad.index.truncated
            ? t("files.filterPartial")
            : null;
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
            if (filtering) readIndex();
          }}
        >
          <RestartIcon />
        </button>
        <button
          type="button"
          className="icon-button"
          aria-label={t("files.filter")}
          title={t("files.filter")}
          aria-expanded={filterOpen || query !== ""}
          onClick={openFilter}
        >
          <SearchIcon />
        </button>
        <button
          type="button"
          className="icon-button"
          aria-label={t("files.reveal")}
          title={t("files.reveal")}
          onClick={() => {
            api.revealSessionFolder(session.id).catch((error: unknown) => {
              setOpenError(t("files.revealFailed", { error: errorText(error) }));
            });
          }}
        >
          <RevealFolderIcon />
        </button>
      </div>
      {(filterOpen || query !== "") && (
        <div className="files-filter">
          <span className="files-filter-icon" aria-hidden="true">
            <SearchIcon size={13} />
          </span>
          <input
            ref={filterInput}
            type="text"
            className="files-filter-input"
            value={query}
            placeholder={t("files.filter")}
            aria-label={t("files.filter")}
            spellCheck={false}
            autoComplete="off"
            onChange={(event) => {
              changeQuery(event.target.value);
            }}
            onKeyDown={onFilterKeyDown}
            onBlur={(event) => {
              // Leaving an empty field closes it; the clear button inside keeps it open.
              const into = event.relatedTarget;
              if (into instanceof Node && event.currentTarget.parentElement?.contains(into)) {
                return;
              }
              if (event.currentTarget.value === "") setFilterOpen(false);
            }}
          />
          {query !== "" && (
            <button
              type="button"
              className="files-filter-clear"
              aria-label={t("files.filterClear")}
              title={t("files.filterClear")}
              onClick={() => {
                changeQuery("");
                filterInput.current?.focus();
              }}
            >
              <CloseIcon size={12} />
            </button>
          )}
        </div>
      )}
      {openError !== null && (
        <p className="changes-message error-text selectable" role="alert">
          {openError}
        </p>
      )}
      {filterNote !== null && (
        <p className="changes-message files-filter-note" role="status">
          {filterNote}
        </p>
      )}
      {!filtering && root?.kind === "failed" ? (
        <p className="changes-message error-text selectable" role="alert">
          {t("files.failed", { error: root.message })}
        </p>
      ) : !filtering && (root === undefined || root.kind === "loading") ? (
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
                canDrag={canDrag}
                setRef={(element) => {
                  if (element) items.current.set(row.path, element);
                  else items.current.delete(row.path);
                }}
                onClick={() => {
                  if (justDropped()) return;
                  dispatch({ type: "focus", path: row.path });
                  if (row.entry.kind !== "dir") return;
                  // In the filtered tree a folder does not fold: it shows in the whole tree.
                  if (filtering) reveal(row.path, true);
                  else toggle(row);
                }}
                onOpen={() => {
                  openFile(row.path);
                }}
                onMention={() => {
                  dispatch({ type: "focus", path: row.path });
                  props.onMention(rowMention(row));
                }}
                onPointerDown={(event) => {
                  startDrag(event, row);
                }}
              />
            ),
          )}
        </ul>
      )}
      {ghost !== null &&
        // On the body: a transformed panel would otherwise carry the fixed ghost along.
        createPortal(
          <div
            ref={ghostRef}
            className="tree-drag-ghost"
            aria-hidden="true"
            style={{
              transform: `translate(${String(ghost.x)}px, ${String(ghost.y)}px)`,
            }}
          >
            {revealInvisible(ghost.text)}
          </div>,
          document.body,
        )}
    </div>
  );
}

/** Offset of the dragged mention from the pointer, so it never hides the drop point. */
const GHOST_OFFSET_X = 14;
const GHOST_OFFSET_Y = 16;

/**
 * Dragging tree rows onto a terminal pane, with pointer events: Tauri's native file drop owns
 * HTML5 drag and drop in the window (see useSessionDrag.ts). The drag starts beyond the usual
 * threshold, so a click or a double click stays one; while it runs the row holds the pointer
 * capture, so no terminal sees mouse motion with a pressed button. Without a drop target (the
 * detached panel window) nothing drags.
 */
function useRowDrag(onDrag: ((drag: MentionDrag) => void) | undefined) {
  const [ghost, setGhost] = useState<{ text: string; x: number; y: number } | null>(null);
  const ghostRef = useRef<HTMLDivElement>(null);
  /** Set from the drop until the click that follows it has passed. */
  const dropped = useRef(false);
  /** Ends a running drag; called when the panel goes away. */
  const stop = useRef<(() => void) | null>(null);
  useEffect(
    () => () => {
      stop.current?.();
    },
    [],
  );
  const startDrag = (event: React.PointerEvent<HTMLElement>, row: ItemRow) => {
    if (onDrag === undefined) return;
    if (event.button !== 0 || !event.isPrimary || stop.current !== null) return;
    const element = event.currentTarget;
    const pointer = event.pointerId;
    const startX = event.clientX;
    const startY = event.clientY;
    const text = rowMention(row);
    let active = false;

    const finish = (phase: "drop" | "cancel", x: number, y: number) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("keydown", key, true);
      stop.current = null;
      if (element.hasPointerCapture(pointer)) element.releasePointerCapture(pointer);
      if (!active) return;
      setGhost(null);
      dropped.current = true;
      window.setTimeout(() => {
        dropped.current = false;
      }, 0);
      onDrag({ phase, x, y, text });
    };
    const move = (moved: PointerEvent) => {
      if (moved.pointerId !== pointer) return;
      if (!active) {
        if (!beyondThreshold(moved.clientX - startX, moved.clientY - startY)) return;
        active = true;
        try {
          element.setPointerCapture(pointer);
        } catch {
          // The pointer is already gone; the drag ends with its pointerup or pointercancel.
        }
        setGhost({
          text: text.trim(),
          x: moved.clientX + GHOST_OFFSET_X,
          y: moved.clientY + GHOST_OFFSET_Y,
        });
      }
      const node = ghostRef.current;
      if (node) {
        node.style.transform = `translate(${String(moved.clientX + GHOST_OFFSET_X)}px, ${String(moved.clientY + GHOST_OFFSET_Y)}px)`;
      }
      onDrag({ phase: "move", x: moved.clientX, y: moved.clientY, text });
    };
    const up = (released: PointerEvent) => {
      if (released.pointerId === pointer) finish("drop", released.clientX, released.clientY);
    };
    const cancel = (lost: PointerEvent) => {
      if (lost.pointerId === pointer) finish("cancel", lost.clientX, lost.clientY);
    };
    const key = (pressed: KeyboardEvent) => {
      if (pressed.key !== "Escape" || !active) return;
      pressed.preventDefault();
      pressed.stopPropagation();
      finish("cancel", startX, startY);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("keydown", key, true);
    stop.current = () => {
      finish("cancel", startX, startY);
    };
  };

  return {
    canDrag: onDrag !== undefined,
    startDrag,
    ghost,
    ghostRef,
    /** The click after a drop belongs to the drag, not to the row. */
    justDropped: () => dropped.current,
  };
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
  /** Rows can be dragged onto a terminal. */
  canDrag: boolean;
  setRef: (element: HTMLLIElement | null) => void;
  onClick: () => void;
  /** Double click on a file: show it in the viewer. */
  onOpen: () => void;
  /** Right click: the row's path as an @ mention into the terminal. */
  onMention: () => void;
  onPointerDown: (event: React.PointerEvent<HTMLLIElement>) => void;
}) {
  const { row, focused, selected, fileMark, folderMark, nameOf, setRef, onClick, onOpen } = props;
  const t = useT();
  const kind = row.entry.kind;
  const notes = rowNotes(row, fileMark, folderMark, nameOf, t);
  const className = [
    "tree-item",
    row.entry.ignored ? "ignored" : "",
    row.context === true ? "context" : "",
    selected ? "selected" : "",
    onChangePath(kind, fileMark, folderMark) ? "on-path" : "",
  ]
    .filter(Boolean)
    .join(" ");
  const tooltip = [revealInvisible(row.path), ...notes];
  if (kind === "file") tooltip.push(t("files.openHint"));
  tooltip.push(t("files.mentionHint"));
  if (props.canDrag) tooltip.push(t("files.dragHint"));
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
      title={tooltip.join("\n")}
      onClick={onClick}
      onDoubleClick={kind === "file" ? onOpen : undefined}
      onContextMenu={(event) => {
        event.preventDefault();
        props.onMention();
      }}
      onPointerDown={props.onPointerDown}
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

import { useEffect, useMemo, useReducer, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { useEventLog } from "./activityStore";
import { api, errorText, parseFolderListing, parsePathIndex, type PathIndex } from "./api";
import { Avatar } from "./Avatar";
import { isFileLevel, type ChangeKind, type ClaimsSnapshot } from "./claims";
import { rememberFile } from "./fileContext";
import {
  HEAT_WINDOWS,
  heatLevel,
  heatOf,
  NO_HEAT,
  otherWorktrees,
  PLAY_STEPS,
  projectKeyOf,
  sinceOf,
  stepTime,
  windowStart,
  type ActivityEvent,
  type Heat,
  type HeatLevel,
  type HeatWindow,
  type LocalSession,
} from "./fileActivity";
import {
  avatarStack,
  buildMarks,
  CHANGE_LETTERS,
  filteredStep,
  filterTree,
  folderKeyOf,
  folderPeople,
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
  type SessionEditor,
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
  HeatIcon,
  LinkIcon,
  OverlapIcon,
  PauseIcon,
  PlayIcon,
  RestartIcon,
  RevealFolderIcon,
  SearchIcon,
} from "./icons";
import { revealInvisible } from "./invisible";
import { Revealed } from "./Revealed";
import { beyondThreshold } from "./sessionDrag";
import { clockTime, personName, personTag, type TeamActivity } from "./team";

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
/** Names in a folder's tooltip, of each kind (teammates, the user's sessions). */
const MAX_FOLDER_NAMES = 4;
/** How long a failed opening stays in the panel. */
const OPEN_ERROR_MS = 6000;
/** Trees kept for sessions the panel showed before (expanded folders, focus). */
const MAX_SAVED_TREES = 24;
/** A revealed row is focused once it shows, if that happens within this time. */
const REVEAL_FOCUS_MS = 3000;
/** One replay step of the activity map, and the calmer pace with reduced motion. */
const PLAY_STEP_MS = 450;
const PLAY_STEP_CALM_MS = 1200;
/** How often a live activity map moves its window on with the clock. */
const HEAT_CLOCK_MS = 60_000;

const NO_EVENTS: readonly ActivityEvent[] = [];

const HEAT_LABELS: Record<HeatWindow, MessageKey> = {
  hour: "files.heatHour",
  today: "files.heatToday",
  week: "files.heatWeek",
};

/** Whether the activity map is on, and its window: kept while the window lives, for every tree. */
let heatPrefs: { on: boolean; window: HeatWindow } = { on: false, window: "today" };

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
  /** This device's sessions: names and programs for the avatars, repositories for the projects. */
  sessions: readonly LocalSession[];
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
 * explorer. Folders are listed when opened; files teammates or the user's other sessions (other
 * worktrees) change carry their avatars, files this worktree changed a letter (M, A, U), and
 * folders show who works inside (a count while collapsed) or a ring for changes. The names along
 * the path to every changed file show in the "working" color. The activity map tints rows by how
 * often agents changed them in a time window and replays that window. A double click or Enter
 * opens a file in a read-only viewer window.
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
    rememberFile(session.id, path);
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
  const log = useEventLog();
  const sessions = props.sessions;
  const project = useMemo(() => {
    const repo = sessions.find((entry) => entry.id === session.id)?.repo ?? null;
    const root = claims.sessions.find((entry) => entry.sessionId === session.id)?.root ?? null;
    return projectKeyOf(repo, root ?? state.repoRoot);
  }, [sessions, claims, session.id, state.repoRoot]);
  const seen = log.seen;
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
      others: otherWorktrees(claims, sessions, session.id),
      sinceOf: (holder, path) => (project === null ? null : sinceOf(seen, project, holder, path)),
    });
  }, [state.root, state.repoRoot, claims, activities, session.id, sessions, seen, project]);
  const sessionOf = useMemo(() => {
    const byId = new Map(sessions.map((entry) => [entry.id, entry]));
    return (id: number) => byId.get(id);
  }, [sessions]);

  // Activity map: the heat of the project's change events in the chosen window, computed once per
  // change of the log, the window or the replay step, never per row.
  const [heatOn, setHeatOn] = useState(heatPrefs.on);
  const [heatWindow, setHeatWindow] = useState<HeatWindow>(heatPrefs.window);
  /** When the window was chosen or the map opened: its fixed start. */
  const [heatNow, setHeatNow] = useState(() => Date.now());
  const [step, setStep] = useState(PLAY_STEPS);
  const [playing, setPlaying] = useState(false);
  const events = project === null ? NO_EVENTS : (log.projects.get(project) ?? NO_EVENTS);
  const heatFrom = windowStart(heatWindow, heatNow);
  // Live at the end: events after opening still count.
  const heatTo = Math.max(heatNow, events[events.length - 1]?.at ?? 0);
  const heatUntil = stepTime(heatFrom, heatTo, step);
  const heat = useMemo(
    () => (heatOn ? heatOf(events, heatFrom, heatUntil, state.root, state.repoRoot) : NO_HEAT),
    [heatOn, events, heatFrom, heatUntil, state.root, state.repoRoot],
  );
  const showHeat = (on: boolean) => {
    heatPrefs = { ...heatPrefs, on };
    setHeatOn(on);
    setHeatNow(Date.now());
    setStep(PLAY_STEPS);
    setPlaying(false);
  };
  const chooseWindow = (window: HeatWindow) => {
    heatPrefs = { ...heatPrefs, window };
    setHeatWindow(window);
    setHeatNow(Date.now());
    setStep(PLAY_STEPS);
    setPlaying(false);
  };
  /** The replay runs until it reaches the end, then the map stays live. */
  const running = heatOn && playing && step < PLAY_STEPS;
  const togglePlay = () => {
    if (running) {
      setPlaying(false);
      return;
    }
    setHeatNow(Date.now());
    if (step >= PLAY_STEPS) setStep(0);
    setPlaying(true);
  };
  const seek = (next: number) => {
    setPlaying(false);
    if (next >= PLAY_STEPS) setHeatNow(Date.now());
    setStep(next);
  };
  // Live at the slider's end, the window slides with the clock: "last hour" stays one hour long
  // and "today" starts again at midnight, even while no new event arrives.
  const live = heatOn && step >= PLAY_STEPS && !running;
  useEffect(() => {
    if (!live) return;
    const timer = window.setInterval(() => {
      setHeatNow(Date.now());
    }, HEAT_CLOCK_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, [live]);
  useEffect(() => {
    if (!running) return;
    const calm = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const timer = window.setInterval(
      () => {
        setStep((current) => Math.min(PLAY_STEPS, current + 1));
      },
      calm ? PLAY_STEP_CALM_MS : PLAY_STEP_MS,
    );
    return () => {
      window.clearInterval(timer);
    };
  }, [running]);

  const itemRows = rows.filter((row): row is ItemRow => row.kind === "item");
  const tabStop = itemRows.some((row) => row.path === state.focused)
    ? state.focused
    : (itemRows[0]?.path ?? null);

  // The selected file is what a snippet's {{file}} means for this session (fileContext.ts).
  const focusedFile =
    itemRows.find((row) => row.path === state.focused && row.entry.kind === "file")?.path ?? null;
  useEffect(() => {
    if (focusedFile !== null) rememberFile(session.id, focusedFile);
  }, [focusedFile, session.id]);

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
        {state.repoRoot !== null && (
          <button
            type="button"
            className={heatOn ? "icon-button toggle on" : "icon-button toggle"}
            aria-label={t("files.heat")}
            title={t("files.heatHint")}
            aria-pressed={heatOn}
            onClick={() => {
              showHeat(!heatOn);
            }}
          >
            <HeatIcon />
          </button>
        )}
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
      {heatOn && state.repoRoot !== null && (
        <HeatBar
          window={heatWindow}
          step={step}
          running={running}
          until={heatUntil}
          empty={heat.total === 0}
          onWindow={chooseWindow}
          onStep={seek}
          onPlay={togglePlay}
        />
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
          className={heatOn ? "file-tree heat" : "file-tree"}
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
                heat={heatOn ? rowHeat(heat, row, windows) : null}
                nameOf={nameOf}
                sessionOf={sessionOf}
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
  /** The row's share of the activity map; `null` while the map is off. */
  heat: RowHeat | null;
  nameOf: (id: number) => string;
  sessionOf: (id: number) => LocalSession | undefined;
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
  const { row, focused, selected, fileMark, folderMark, heat, setRef, onClick, onOpen } = props;
  const t = useT();
  const kind = row.entry.kind;
  const notes = rowNotes(row, fileMark, folderMark, props, t);
  if (heat !== null && heat.count > 0) notes.push(t("files.heatCount", { count: heat.count }));
  /** A collapsed folder counts who works inside; an open one shows them on its rows. */
  const people =
    kind === "dir" && !row.expanded && folderMark !== null ? folderPeople(folderMark) : 0;
  const className = [
    "tree-item",
    row.entry.ignored ? "ignored" : "",
    row.context === true ? "context" : "",
    selected ? "selected" : "",
    onChangePath(kind, fileMark, folderMark) ? "on-path" : "",
    heat !== null && heat.level > 0 ? `heat-${String(heat.level)}` : "",
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
        {people > 0 ? (
          <span className="tree-count">{people}</span>
        ) : (
          folderMark !== null &&
          kind === "dir" && (
            <span className={folderMark.team ? "tree-dot team" : "tree-dot change"} />
          )
        )}
        {fileMark !== null && fileMark.change !== null && fileMark.alsoIn.length > 0 && (
          <span className="tree-also">
            <OverlapIcon size={11} />
          </span>
        )}
        {fileMark !== null && (fileMark.editors.length > 0 || fileMark.alsoIn.length > 0) && (
          <Avatars
            editors={fileMark.editors}
            sessions={fileMark.alsoIn}
            sessionOf={props.sessionOf}
          />
        )}
        {fileMark !== null && fileMark.change !== null && (
          <span className={`tree-change ${fileMark.change}`}>
            {CHANGE_LETTERS[fileMark.change]}
          </span>
        )}
      </span>
    </li>
  );
}

/**
 * Teammates first, then the user's other sessions (rounded squares), at most MAX_AVATARS in all,
 * the rest as "+N".
 */
function Avatars({
  editors,
  sessions,
  sessionOf,
}: {
  editors: readonly TeamEditor[];
  sessions: readonly SessionEditor[];
  sessionOf: (id: number) => LocalSession | undefined;
}) {
  const { shown, more } = avatarStack(editors, MAX_AVATARS);
  const shownSessions = sessions.slice(0, MAX_AVATARS - shown.length);
  const rest = more + sessions.length - shownSessions.length;
  return (
    <span className="tree-people">
      {shown.map((editor) => (
        <Avatar
          key={`${editor.member}\u0000${String(editor.own)}`}
          name={editor.name}
          size="mini"
        />
      ))}
      {shownSessions.map((editor) => (
        <Avatar
          key={editor.session}
          name={sessionOf(editor.session)?.title ?? ""}
          size="mini"
          session
        />
      ))}
      {rest > 0 && <span className="tree-more">+{rest}</span>}
    </span>
  );
}

/** "…, since 14:05" when the time is known. */
function withSince(text: string, since: number | null, t: Translate): string {
  return since === null ? text : t("files.since", { text, time: clockTime(since) });
}

/** "Claude 2 (Claude Code)": one of the user's sessions with its program, if that says more. */
function sessionLabel(id: number, props: Pick<RowNoteProps, "nameOf" | "sessionOf">): string {
  const name = revealInvisible(props.nameOf(id));
  const program = revealInvisible(props.sessionOf(id)?.program.trim() ?? "");
  return program === "" || name.includes(program) ? name : `${name} (${program})`;
}

interface RowNoteProps {
  nameOf: (id: number) => string;
  sessionOf: (id: number) => LocalSession | undefined;
}

/** What the row's marks mean, for its tooltip and screen readers. */
function rowNotes(
  row: ItemRow,
  fileMark: FileMark | null,
  folderMark: FolderMark | null,
  props: RowNoteProps,
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
      const text =
        agent === ""
          ? t("files.editedByPlain", { person })
          : t("files.editedBy", { person, agent });
      notes.push(withSince(text, editor.since, t));
    }
    for (const editor of fileMark.alsoIn) {
      const text = t("files.alsoIn", { session: sessionLabel(editor.session, props) });
      notes.push(withSince(text, editor.since, t));
    }
  }
  if (folderMark !== null && row.entry.kind === "dir") {
    if (folderMark.team) {
      const names = [
        ...avatarStack(folderMark.editors, MAX_FOLDER_NAMES).shown.map((editor) =>
          personName(editor, t),
        ),
        ...folderMark.sessions.slice(0, MAX_FOLDER_NAMES).map((id) => sessionLabel(id, props)),
      ];
      notes.push(t("files.folderOthers", { names: names.join(", ") }));
    }
    if (folderMark.change) notes.push(t("files.folderChanged"));
  }
  return notes;
}

/** The row's share of the activity map. */
interface RowHeat {
  level: HeatLevel;
  count: number;
}

function rowHeat(heat: Heat, row: ItemRow, windows: boolean): RowHeat {
  const key = treeKey(row.path, windows);
  if (row.entry.kind === "dir") {
    const count = heat.folders.get(key) ?? 0;
    return { level: heatLevel(count, heat.maxFolder), count };
  }
  const count = heat.files.get(key) ?? 0;
  return { level: heatLevel(count, heat.maxFile), count };
}

/**
 * Below the header while the activity map is on: the window (last hour, today, 7 days), play and
 * pause, and the replay position as a slider. At the slider's end the map is live.
 */
function HeatBar(props: {
  window: HeatWindow;
  step: number;
  running: boolean;
  until: number;
  /** Nothing happened in the window up to the position. */
  empty: boolean;
  onWindow: (window: HeatWindow) => void;
  onStep: (step: number) => void;
  onPlay: () => void;
}) {
  const t = useT();
  const live = props.step >= PLAY_STEPS && !props.running;
  const time = live ? t("files.heatNow") : heatTime(props.until, props.window);
  return (
    <div className="heat-bar">
      <div className="heat-windows segmented" role="radiogroup" aria-label={t("files.heatWindow")}>
        {HEAT_WINDOWS.map((option) => (
          <label key={option} className="segment">
            <input
              type="radio"
              name="heat-window"
              value={option}
              checked={props.window === option}
              onChange={() => {
                props.onWindow(option);
              }}
            />
            <span>{t(HEAT_LABELS[option])}</span>
          </label>
        ))}
      </div>
      <div className="heat-replay">
        <button
          type="button"
          className="icon-button"
          aria-label={props.running ? t("files.heatPause") : t("files.heatPlay")}
          title={props.running ? t("files.heatPause") : t("files.heatPlay")}
          onClick={props.onPlay}
        >
          {props.running ? <PauseIcon size={13} /> : <PlayIcon size={13} />}
        </button>
        <input
          type="range"
          className="heat-slider"
          min={0}
          max={PLAY_STEPS}
          step={1}
          value={props.step}
          aria-label={t("files.heatTime")}
          aria-valuetext={time}
          onChange={(event) => {
            props.onStep(Number(event.currentTarget.value));
          }}
        />
        <output className="heat-time">{time}</output>
      </div>
      {props.empty && (
        <p className="heat-empty" role="status">
          {t("files.heatNone")}
        </p>
      )}
    </div>
  );
}

/** The replay position as a time of day, with the weekday for the 7 day window. */
function heatTime(ms: number, window: HeatWindow): string {
  return new Intl.DateTimeFormat(undefined, {
    ...(window === "week" ? { weekday: "short" } : {}),
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(ms));
}

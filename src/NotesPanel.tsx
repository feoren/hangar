import {
  useCallback,
  useEffect,
  useId,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";

import { api, errorText, type NoteFolder } from "./api";
import { writeClipboard } from "./clipboard";
import { folderName } from "./folders";
import { formatShortTime, relativeTime } from "./format";
import { DICTIONARIES, LANGUAGES, useT } from "./i18n";
import {
  ChevronIcon,
  CloseIcon,
  EyeIcon,
  LockIcon,
  MoreIcon,
  PlusIcon,
  SearchIcon,
  SendIcon,
  TaskIcon,
} from "./icons";
import type { MarkdownEditor } from "./markdownEditor";
import { NoteEditor } from "./NoteEditor";
import {
  byLastChange,
  cleanTitle,
  folderId,
  isUntitled,
  MAX_NOTE_BYTES,
  noteKey,
  parseNotes,
  parseSearchHits,
  parseTrash,
  sameFolder,
  sameNote,
  titleFromText,
  untitledTitle,
  type NoteFolderInfo,
  type NoteRef,
  type SearchHit,
  type TrashItem,
} from "./notes";
import { sendableText } from "./panelBridge";
import {
  DEFAULT_SCOPE,
  effectiveScope,
  groupsScope,
  mixesScopes,
  newNoteFolder,
  projectFolder,
  sameScope,
  scopeFolders,
  scopeForFolder,
  type NotesScope,
} from "./notesScope";
import { SharedNotesDialog } from "./SharedNotesDialog";
import {
  isShared,
  parseSharedStatus,
  sharedFolder,
  sharedHints,
  sharedLine,
  showsGroups,
  type SharedStatus,
} from "./sharedNotes";
import {
  appendCapture,
  checklistProgress,
  noteExcerpt,
  wordCount,
  type ChecklistProgress,
} from "./noteText";
import { PopupMenu, type PopupItem } from "./PopupMenu";
import { formatShortcut, SHORTCUT_LABELS, type FocusZone } from "./shortcuts";
import { useClock } from "./useClock";
import { utf8Length } from "./workspace";

/** A request from outside the panel; a new `token` means a new request. */
export type NotesRequest =
  | { token: number; kind: "quick" }
  | { token: number; kind: "focus" }
  | { token: number; kind: "capture"; text: string; source: string }
  | { token: number; kind: "openFolder"; folder: NoteFolder };

/** What the window needs from the panel for F6. */
export interface NotesHandle {
  /** Zone inside the panel that has the focus, `null` if none. */
  zone: () => FocusZone | null;
  /** Zones the panel offers right now (the editor only while a note is open). */
  zones: () => FocusZone[];
  focusZone: (zone: FocusZone) => void;
}

export interface SendTarget {
  id: number;
  title: string;
}

interface Props {
  /** UI language, for times and word counts. */
  language: string;
  /** Folder of the active session, `null` without a session. */
  sessionFolder: NoteFolder | null;
  folders: readonly NoteFolderInfo[];
  /** Running sessions that can receive text. */
  sessions: readonly SendTarget[];
  activeSessionId: number | null;
  request: NotesRequest | null;
  /** Goes up when notes changed elsewhere (the other window): reload. */
  refreshToken: number;
  handle: React.RefObject<NotesHandle | null>;
  onSend: (text: string, sessionId: number | null) => void;
  /** Notes of `folder` were created, changed, renamed or deleted. */
  onChanged: (folder: NoteFolder) => void;
  /** Escape: back to the terminal (or close, where the panel lies over it). */
  onEscape: () => void;
}

const SAVE_DELAY_MS = 500;
/** Word count, progress and preview follow the text after a short pause. */
const STATS_DELAY_MS = 250;
const SEARCH_DELAY_MS = 200;
const TOAST_MS = 6000;
/** Rows of the list whose excerpt is read; the rest show the title only. */
const DETAIL_LIMIT = 60;

/** "Note" in every language: a title created in another language also counts as new. */
const UNTITLED_BASES = LANGUAGES.map((language) => DICTIONARIES[language]["notes.untitled"]);

// Remembered while Hangar runs, so closing and reopening the panel finds things as they were.
let lastScope: NotesScope = DEFAULT_SCOPE;
let lastOpen: { folder: NoteFolder; id: string } | null = null;
/**
 * Newest request handled. Module-wide: the panel mounts again each time it opens, while the
 * window keeps its last request; tokens only grow, so an old one is never handled twice.
 */
let handledToken = 0;
/** Notes shown as preview. */
const previewNotes = new Set<string>();
/** Notes Hangar created in this run: their title follows the first line until you rename them. */
const autoTitled = new Set<string>();
/** Excerpt and progress per note, for the list; valid while `updatedAt` stays the same. */
const details = new Map<
  string,
  { updatedAt: number | null; excerpt: string; progress: ChecklistProgress }
>();

/** Pending saves of all notes panels, e.g. before quitting. */
const flushers = new Set<() => Promise<void>>();

export function flushNotes(): Promise<void> {
  return Promise.all([...flushers].map((flush) => flush())).then(() => undefined);
}

/** The note in the editor. `id === null`: a new quick note that exists only once you type. */
interface Current {
  folder: NoteFolder;
  id: string | null;
  title: string;
}

interface Stats {
  words: number;
  progress: ChecklistProgress;
  /** Title the note gets on leaving (auto title), `null` if it keeps its own. */
  autoTitle: string | null;
  /** Nothing to send yet. */
  empty: boolean;
}

type View = "list" | "note" | "trash";

interface Toast {
  message: string;
  action?: { label: string; run: () => void };
}

interface TrashRow extends TrashItem {
  folder: NoteFolder;
}

function isMac(): boolean {
  return /Mac/i.test(navigator.userAgent);
}

/**
 * Notes in the right panel and in the detached window: a list of notes (scope chips, search) and
 * the Markdown editor of one note. Saving is automatic, shortly after the last input and on
 * leaving a note.
 */
export function NotesPanel(props: Props) {
  const { language, sessionFolder, folders, sessions, request, refreshToken } = props;
  const t = useT();
  const now = useClock(30_000);
  const sharedHeadId = useId();
  const localHeadId = useId();
  const root = useRef<HTMLDivElement>(null);
  const [scope, setScopeState] = useState<NotesScope>(lastScope);
  const [view, setView] = useState<View>(lastOpen ? "note" : "list");
  const [list, setList] = useState<NoteRef[]>([]);
  const [listedKey, setListedKey] = useState<string | null>(null);
  // Only re-renders the rows once their excerpts arrive (they live in `details`).
  const [, setDetailTick] = useState(0);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<{ query: string; hits: SearchHit[] } | null>(null);
  const [trash, setTrash] = useState<TrashRow[] | null>(null);
  const [current, setCurrent] = useState<Current | null>(null);
  const [text, setText] = useState("");
  const [loadKey, setLoadKey] = useState("");
  const [stats, setStats] = useState<Stats>({
    words: 0,
    progress: { done: 0, total: 0 },
    autoTitle: null,
    empty: true,
  });
  const [previewHtml, setPreviewHtml] = useState<string | null>(null);
  const [selected, setSelected] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [menu, setMenu] = useState<{ kind: "more" | "send" | "list"; x: number; y: number } | null>(
    null,
  );
  const [toast, setToast] = useState<Toast | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("");
  /** Shared group of the shown project; `null` while unknown or without a project. */
  const [shared, setShared] = useState<{ project: string; status: SharedStatus } | null>(null);
  const [sharedDialog, setSharedDialog] = useState<{
    mode: "setup" | "unlock";
    folder: NoteFolder;
  } | null>(null);

  const currentRef = useRef<Current | null>(null);
  const textRef = useRef("");
  /** Load key whose content is in the editor; input before that belongs to the old note. */
  const editorKey = useRef<string | null>(null);
  const loadKeyRef = useRef("");
  const loadCounter = useRef(0);
  const pending = useRef<{ folder: NoteFolder; id: string; text: string } | null>(null);
  const saveTimer = useRef<number | null>(null);
  const statsTimer = useRef<number | null>(null);
  const toastTimer = useRef<number | null>(null);
  const creating = useRef<Promise<void> | null>(null);
  const generation = useRef(0);
  const editor = useRef<MarkdownEditor | null>(null);
  const focusEditorOnLoad = useRef(false);
  const searchInput = useRef<HTMLInputElement>(null);
  const backButton = useRef<HTMLButtonElement>(null);
  const moreButton = useRef<HTMLButtonElement>(null);
  const sendMenuButton = useRef<HTMLButtonElement>(null);
  const listMoreButton = useRef<HTMLButtonElement>(null);
  const rows = useRef(new Map<string, HTMLButtonElement>());
  const onChangedRef = useRef(props.onChanged);
  const contextRef = useRef({ sessionFolder, folders });
  const loadSharedRef = useRef<() => void>(() => undefined);
  const sharedBusy = useRef(false);
  const sharedAgain = useRef(false);
  /** Counts unlocked status answers; the list reloads on each (files may have changed). */
  const [sharedTick, setSharedTick] = useState(0);

  useEffect(() => {
    onChangedRef.current = (folder: NoteFolder) => {
      props.onChanged(folder);
      if (isShared(folder)) loadSharedRef.current();
    };
    contextRef.current = { sessionFolder, folders };
  });

  const shownScope = effectiveScope(scope, sessionFolder);
  const project = projectFolder(scope, sessionFolder);
  const projectId = project === null ? null : folderId(project);
  const sharedStatus = shared !== null && shared.project === projectId ? shared.status : null;
  const sharedPath =
    project !== null && sharedStatus?.state === "unlocked" ? project.path : undefined;
  const listFolders = useMemo(
    () =>
      scopeFolders(
        scope,
        sessionFolder,
        folders,
        sharedPath === undefined ? null : sharedFolder({ path: sharedPath }),
      ),
    [scope, sessionFolder, folders, sharedPath],
  );
  const listKey = listFolders.map(folderId).join("|");
  const mixed = mixesScopes(scope, sessionFolder);
  const preview = current?.id != null && previewNotes.has(noteKey({ ...current, id: current.id }));
  const general = t("notes.general");

  const setScope = (next: NotesScope) => {
    lastScope = next;
    setScopeState(next);
  };

  const showToast = useCallback((next: Toast) => {
    if (toastTimer.current !== null) window.clearTimeout(toastTimer.current);
    setToast(next);
    setAnnouncement(next.message);
    toastTimer.current = window.setTimeout(() => {
      toastTimer.current = null;
      setToast(null);
    }, TOAST_MS);
  }, []);

  /** Asks the backend for the Shared group's state; it resolves conflicted notes on the way. */
  const loadShared = useCallback(() => {
    if (project === null || projectId === null) {
      setShared(null);
      return;
    }
    // One status call at a time: each runs git and may resolve conflicts, and focus events come
    // in bursts. A call asked for meanwhile runs once afterwards, for the then shown project.
    if (sharedBusy.current) {
      sharedAgain.current = true;
      return;
    }
    sharedBusy.current = true;
    api
      .sharedNotesStatus(project, t("sharedNotes.otherVersion"), t("notes.untitled"))
      .then((value) => {
        const status = parseSharedStatus(value);
        if (status === null) return;
        setShared({ project: projectId, status });
        if (status.state === "unlocked") {
          // A pull or a resolved conflict may have changed the files: list them again.
          setSharedTick((tick) => tick + 1);
          const first = status.resolved[0];
          if (first) showToast({ message: t("sharedNotes.resolved", { title: first.title }) });
        }
      })
      .catch(() => {
        // Without an answer the panel shows the private notes as before.
        setShared({ project: projectId, status: { state: "unavailable" } });
      })
      .finally(() => {
        sharedBusy.current = false;
        if (sharedAgain.current) {
          sharedAgain.current = false;
          loadSharedRef.current();
        }
      });
    // `project` is rebuilt every render; its id says when it changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId, t, showToast]);

  useEffect(() => {
    loadSharedRef.current = loadShared;
  }, [loadShared]);

  useEffect(() => {
    void Promise.resolve().then(loadShared);
    window.addEventListener("focus", loadShared);
    return () => {
      window.removeEventListener("focus", loadShared);
    };
  }, [loadShared, refreshToken]);

  const flush = useCallback(async () => {
    if (saveTimer.current !== null) {
      window.clearTimeout(saveTimer.current);
      saveTimer.current = null;
    }
    const job = pending.current;
    if (!job) return;
    pending.current = null;
    if (utf8Length(job.text) > MAX_NOTE_BYTES) {
      setError(t("notes.tooLarge"));
      return;
    }
    try {
      await api.writeNote(job.folder, job.id, job.text);
      details.delete(noteKey(job));
    } catch (e) {
      setError(t("notes.saveFailed", { error: errorText(e) }));
    }
  }, [t]);

  /** Creates the quick note in the backend as soon as it has content. */
  const ensureCreated = useCallback(async () => {
    if (creating.current) {
      await creating.current;
      return;
    }
    const note = currentRef.current;
    if (!note || note.id !== null || textRef.current.trim() === "") return;
    const work = (async () => {
      try {
        const existing = parseNotes(await api.listNotes(note.folder)).map((n) => n.title);
        const title = untitledTitle(t("notes.untitled"), existing);
        const id = await api.createNote(note.folder, title);
        if (typeof id !== "string") return;
        autoTitled.add(noteKey({ folder: note.folder, id }));
        const created: Current = { folder: note.folder, id, title };
        // The panel may already show another note; then the new one is only saved.
        if (currentRef.current === note) {
          currentRef.current = created;
          setCurrent(created);
          lastOpen = { folder: note.folder, id };
        }
        pending.current = { folder: note.folder, id, text: textRef.current };
        await flush();
        onChangedRef.current(note.folder);
      } catch (e) {
        setError(errorText(e));
      }
    })();
    creating.current = work;
    await work;
    creating.current = null;
  }, [flush, t]);

  /**
   * Leaving the open note: save, give a new note its title from the first line, and drop a new
   * note that ended up empty.
   */
  const leave = useCallback(async () => {
    await (creating.current ?? Promise.resolve());
    await flush();
    const note = currentRef.current;
    if (!note || note.id === null) return;
    const key = noteKey({ folder: note.folder, id: note.id });
    const content = textRef.current;
    try {
      if (content.trim() === "" && autoTitled.has(key)) {
        await api.deleteNote(note.folder, note.id);
        autoTitled.delete(key);
        if (lastOpen && sameNote(lastOpen, { folder: note.folder, id: note.id })) lastOpen = null;
        onChangedRef.current(note.folder);
        return;
      }
      if (autoTitled.has(key) || isUntitled(note.title, UNTITLED_BASES)) {
        const title = titleFromText(content);
        if (title !== null && title !== note.title) {
          await api.renameNote(note.folder, note.id, title);
          const renamed = { ...note, title };
          if (currentRef.current === note) {
            currentRef.current = renamed;
            setCurrent(renamed);
          }
          onChangedRef.current(note.folder);
        }
      }
    } catch {
      // The note keeps its title; the next leave tries again.
    }
  }, [flush]);

  useEffect(() => {
    const flushAll = () => leave();
    flushers.add(flushAll);
    return () => {
      flushers.delete(flushAll);
      void flushAll();
      for (const timer of [saveTimer, statsTimer, toastTimer]) {
        if (timer.current !== null) window.clearTimeout(timer.current);
      }
    };
  }, [leave]);

  const computeStats = useCallback(
    (content: string, note: Current | null) => {
      const auto =
        note !== null &&
        (note.id === null ||
          autoTitled.has(noteKey({ folder: note.folder, id: note.id })) ||
          isUntitled(note.title, UNTITLED_BASES));
      setStats({
        words: wordCount(content, language),
        progress: checklistProgress(content),
        autoTitle: auto ? titleFromText(content) : null,
        empty: content.trim() === "",
      });
    },
    [language],
  );

  /** Puts `content` into the editor for `note`. */
  const show = useCallback(
    (note: Current | null, content: string, focus: boolean) => {
      currentRef.current = note;
      textRef.current = content;
      setCurrent(note);
      setText(content);
      setRenaming(false);
      setSelected(false);
      setPreviewHtml(null);
      computeStats(content, note);
      loadCounter.current += 1;
      const key = String(loadCounter.current);
      loadKeyRef.current = key;
      setLoadKey(key);
      focusEditorOnLoad.current = focus;
      lastOpen = note?.id != null ? { folder: note.folder, id: note.id } : null;
    },
    [computeStats],
  );

  const openNote = useCallback(
    async (ref: { folder: NoteFolder; id: string }, focus: boolean) => {
      generation.current += 1;
      const mine = generation.current;
      const open = currentRef.current;
      if (!(open?.id != null && sameNote({ folder: open.folder, id: open.id }, ref))) {
        await leave();
      } else {
        await flush();
      }
      if (mine !== generation.current) return;
      try {
        const [content, notes] = await Promise.all([
          api.readNote(ref.folder, ref.id),
          api.listNotes(ref.folder),
        ]);
        if (mine !== generation.current) return;
        const info = parseNotes(notes).find((note) => note.id === ref.id);
        show(
          { folder: ref.folder, id: ref.id, title: info?.title ?? "" },
          typeof content === "string" ? content : "",
          focus,
        );
        setView("note");
      } catch (e) {
        if (mine === generation.current) setError(errorText(e));
      }
    },
    [flush, leave, show],
  );

  /** A new quick note in `folder`; it is created once it has content. */
  const startDraft = useCallback(
    async (folder: NoteFolder, focus: boolean, content = "") => {
      generation.current += 1;
      const mine = generation.current;
      await leave();
      if (mine !== generation.current) return;
      show({ folder, id: null, title: "" }, content, focus);
      setView("note");
      if (content !== "") await ensureCreated();
    },
    [ensureCreated, leave, show],
  );

  const reloadList = useCallback(async () => {
    const shown = listFolders;
    const key = listKey;
    try {
      const lists = await Promise.all(
        shown.map(async (folder) =>
          parseNotes(await api.listNotes(folder)).map((note): NoteRef => ({
            folder,
            id: note.id,
            title: note.title,
            updatedAt: note.updatedAt,
          })),
        ),
      );
      setList(byLastChange(lists.flat()));
      setListedKey(key);
    } catch (e) {
      setError(errorText(e));
    }
  }, [listFolders, listKey]);

  // The list follows the scope, the scopes known to the backend and changes from elsewhere.
  useEffect(() => {
    void Promise.resolve().then(reloadList);
  }, [reloadList, refreshToken, sharedTick]);

  // Excerpt and progress for the rows, read in the background a few at a time.
  useEffect(() => {
    const run = { cancelled: false };
    // A function: the check after each await must read the flag again.
    const stopped = () => run.cancelled;
    const missing = list.slice(0, DETAIL_LIMIT).filter((note) => {
      const known = details.get(noteKey(note));
      return !known || known.updatedAt !== note.updatedAt;
    });
    if (missing.length === 0) return;
    const work = async () => {
      for (let index = 0; index < missing.length && !stopped(); index += 4) {
        await Promise.all(
          missing.slice(index, index + 4).map(async (note) => {
            try {
              const value = await api.readNote(note.folder, note.id);
              const content = typeof value === "string" ? value : "";
              details.set(noteKey(note), {
                updatedAt: note.updatedAt,
                excerpt: noteExcerpt(content, note.title),
                progress: checklistProgress(content),
              });
            } catch {
              // The row shows its title only.
            }
          }),
        );
        if (!stopped()) setDetailTick((tick) => tick + 1);
      }
    };
    void work();
    return () => {
      run.cancelled = true;
    };
  }, [list]);

  // Back from elsewhere: the open note reloads from disk if it changed there (no pending input).
  useEffect(() => {
    const reread = () => {
      const note = currentRef.current;
      if (note?.id == null || pending.current !== null || creating.current) return;
      const id = note.id;
      api
        .readNote(note.folder, id)
        .then((value) => {
          if (typeof value !== "string" || value === textRef.current) return;
          if (currentRef.current !== note || pending.current !== null) return;
          show(note, value, false);
        })
        .catch(() => undefined);
    };
    window.addEventListener("focus", reread);
    if (refreshToken > 0) reread();
    return () => {
      window.removeEventListener("focus", reread);
    };
  }, [refreshToken, show]);

  // The note open last time comes back when the panel opens again, unless the panel opened for
  // something else (quick note, terminal output, another project). Once per mount: React's
  // strict mode runs effects twice in development, a second restore would win over the request.
  const restoreChecked = useRef(false);
  useEffect(() => {
    if (restoreChecked.current) return;
    restoreChecked.current = true;
    const open = lastOpen;
    const pending = request !== null && request.token > handledToken;
    const other = pending && request.kind !== "focus";
    if (open && !other) void Promise.resolve().then(() => openNote(open, pending));
    // Only on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Search across all notes, shortly after the last keystroke.
  useEffect(() => {
    const trimmed = query.trim();
    if (trimmed === "") return;
    let current = true;
    const timer = window.setTimeout(() => {
      api
        .searchNotes(trimmed)
        .then((value) => {
          if (current) setHits({ query: trimmed, hits: parseSearchHits(value) });
        })
        .catch((e: unknown) => {
          if (current) setError(errorText(e));
        });
    }, SEARCH_DELAY_MS);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [query, refreshToken]);

  const focusList = useCallback(() => {
    if (view === "note") backButton.current?.focus();
    else searchInput.current?.focus();
  }, [view]);

  const focusEditor = useCallback(() => {
    if (preview) {
      root.current?.querySelector<HTMLElement>(".note-preview")?.focus();
      return;
    }
    if (editor.current) editor.current.focus();
    else focusEditorOnLoad.current = true;
  }, [preview]);

  useImperativeHandle(props.handle, () => ({
    zone: () => {
      const active = document.activeElement;
      if (!active || !root.current?.contains(active)) return null;
      return active.closest(".note-editor, .note-preview") ? "editor" : "list";
    },
    zones: () => (view === "note" ? ["list", "editor"] : ["list"]),
    focusZone: (zone) => {
      if (zone === "editor") focusEditor();
      else focusList();
    },
  }));

  // Requests: quick note, focus, add terminal output, show a project's notes.
  useEffect(() => {
    if (!request || request.token <= handledToken) return;
    handledToken = request.token;
    const { sessionFolder: session } = contextRef.current;
    // After the commit: the steps below change state of their own.
    void Promise.resolve().then(() => {
      switch (request.kind) {
        case "quick":
          setQuery("");
          void startDraft(newNoteFolder(scope, session), true);
          return;
        case "focus":
          if (view === "note") focusEditor();
          else focusList();
          return;
        case "openFolder": {
          const next = scopeForFolder(request.folder, session);
          setScope(next);
          setQuery("");
          void leave().then(() => {
            show(null, "", false);
            setView("list");
            requestAnimationFrame(() => searchInput.current?.focus());
          });
          return;
        }
        case "capture": {
          const stamp = formatShortTime(new Date(), language);
          const open = currentRef.current;
          if (view === "note" && open) {
            const next = appendCapture(textRef.current, request.text, stamp, request.source);
            if (editor.current && editorKey.current === loadKeyRef.current) {
              editor.current.setText(next);
            } else {
              textRef.current = next;
            }
            if (open.id === null) void ensureCreated();
            setAnnouncement(
              t("notes.captured", { title: open.title === "" ? t("notes.quickNote") : open.title }),
            );
            return;
          }
          const content = appendCapture("", request.text, stamp, request.source);
          void startDraft(newNoteFolder(scope, session), false, content).then(() => {
            setAnnouncement(t("notes.capturedNew"));
          });
          return;
        }
      }
    });
  }, [
    request,
    scope,
    view,
    language,
    t,
    startDraft,
    leave,
    show,
    ensureCreated,
    focusEditor,
    focusList,
  ]);

  // Preview: rendered from the text, again after each pause while typing.
  useEffect(() => {
    if (!preview) return;
    let live = true;
    import("./markdownPreview")
      .then(({ renderMarkdown }) => {
        if (live) setPreviewHtml(renderMarkdown(textRef.current));
      })
      .catch(() => {
        if (live) setError(t("notes.editorFailed"));
      });
    return () => {
      live = false;
    };
  }, [preview, loadKey, stats, t]);

  // The preview was drawn again: the checkbox just clicked keeps the focus.
  const refocusTask = useRef<number | null>(null);
  useEffect(() => {
    const line = refocusTask.current;
    if (line === null || previewHtml === null) return;
    refocusTask.current = null;
    root.current
      ?.querySelector<HTMLInputElement>(`.note-preview input[data-line="${String(line)}"]`)
      ?.focus();
  }, [previewHtml]);

  const onEditorChange = (next: string) => {
    // Input before the editor shows the new note belongs to the old one, which is already saved.
    if (editorKey.current !== loadKeyRef.current) return;
    const note = currentRef.current;
    if (!note) return;
    textRef.current = next;
    if (statsTimer.current !== null) window.clearTimeout(statsTimer.current);
    statsTimer.current = window.setTimeout(() => {
      statsTimer.current = null;
      computeStats(textRef.current, currentRef.current);
    }, STATS_DELAY_MS);
    if (note.id === null) {
      void ensureCreated();
      return;
    }
    pending.current = { folder: note.folder, id: note.id, text: next };
    if (saveTimer.current !== null) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => void flush(), SAVE_DELAY_MS);
  };

  const onEditorLoaded = (key: string) => {
    editorKey.current = key;
    if (focusEditorOnLoad.current && key === loadKeyRef.current) {
      focusEditorOnLoad.current = false;
      editor.current?.focus();
    }
  };

  // The editor may load after the note: focus it once it is there.
  useEffect(() => {
    if (view !== "note" || !focusEditorOnLoad.current) return;
    if (editor.current && editorKey.current === loadKey) {
      focusEditorOnLoad.current = false;
      if (preview) root.current?.querySelector<HTMLElement>(".note-preview")?.focus();
      else editor.current.focus();
    }
  }, [view, loadKey, preview]);

  const backToList = async (focusRow: boolean) => {
    const note = currentRef.current;
    generation.current += 1;
    await leave();
    show(null, "", false);
    setView("list");
    await reloadList();
    requestAnimationFrame(() => {
      const row =
        note?.id != null ? rows.current.get(noteKey({ ...note, id: note.id })) : undefined;
      if (focusRow && row) row.focus();
      else if (focusRow) searchInput.current?.focus();
    });
  };

  const newNote = () => {
    setQuery("");
    void startDraft(newNoteFolder(scope, sessionFolder), true);
  };

  const commitRename = async (value: string) => {
    setRenaming(false);
    const note = currentRef.current;
    if (note?.id == null) return;
    const id = note.id;
    if (value.trim() === note.title) return;
    const title = cleanTitle(value);
    if (title === null) {
      setError(t("notes.invalidTitle"));
      return;
    }
    try {
      await api.renameNote(note.folder, id, title);
      autoTitled.delete(noteKey({ folder: note.folder, id }));
      const renamed = { ...note, title };
      currentRef.current = renamed;
      setCurrent(renamed);
      computeStats(textRef.current, renamed);
      props.onChanged(note.folder);
      if (isShared(note.folder)) loadShared();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const restore = async (folder: NoteFolder, id: string, title: string) => {
    try {
      const restored = await api.restoreNote(folder, id);
      if (typeof restored !== "string") return;
      props.onChanged(folder);
      if (isShared(folder)) loadShared();
      showToast({ message: t("notes.restored", { title }) });
      return restored;
    } catch (e) {
      setError(errorText(e));
      return undefined;
    }
  };

  const deleteCurrent = async () => {
    const note = currentRef.current;
    if (!note) return;
    if (note.id === null) {
      show(null, "", false);
      setView("list");
      return;
    }
    const id = note.id;
    const title = note.title;
    try {
      await flush();
      await api.deleteNote(note.folder, id);
      autoTitled.delete(noteKey({ folder: note.folder, id }));
      details.delete(noteKey({ folder: note.folder, id }));
      lastOpen = null;
      show(null, "", false);
      setView("list");
      props.onChanged(note.folder);
      if (isShared(note.folder)) loadShared();
      await reloadList();
      showToast({
        message: t("notes.deleted", { title }),
        action: {
          label: t("notes.undo"),
          run: () => {
            setToast(null);
            void restore(note.folder, id, title).then((restored) => {
              if (restored !== undefined)
                void openNote({ folder: note.folder, id: restored }, true);
            });
          },
        },
      });
      requestAnimationFrame(() => searchInput.current?.focus());
    } catch (e) {
      setError(errorText(e));
    }
  };

  const openTrash = async () => {
    setView("trash");
    setTrash(null);
    try {
      const lists = await Promise.all(
        listFolders.map(async (folder) =>
          parseTrash(await api.listTrash(folder)).map((item): TrashRow => ({ ...item, folder })),
        ),
      );
      setTrash(lists.flat().sort((a, b) => (b.deletedAt ?? 0) - (a.deletedAt ?? 0)));
    } catch (e) {
      setError(errorText(e));
      setTrash([]);
    }
    requestAnimationFrame(() => backButton.current?.focus());
  };

  const send = (sessionId: number | null) => {
    const selection = editor.current?.selection() ?? "";
    const raw = selection !== "" && !preview ? selection : textRef.current;
    const clean = sendableText(raw).replace(/\s+$/, "");
    if (clean === "") return;
    const target = sessions.find((s) => s.id === (sessionId ?? props.activeSessionId));
    props.onSend(clean, target?.id ?? sessionId);
    setAnnouncement(t("notes.sent", { session: target?.title ?? "" }));
  };

  const defaultTarget =
    sessions.find((session) => session.id === props.activeSessionId) ?? sessions[0];
  const sendLabel = !defaultTarget
    ? t("notes.sendNone")
    : selected && !preview
      ? t("notes.sendSelectionTo", { session: defaultTarget.title })
      : t("notes.sendTo", { session: defaultTarget.title });

  const togglePreview = () => {
    const note = currentRef.current;
    if (note?.id == null) return;
    const key = noteKey({ folder: note.folder, id: note.id });
    if (previewNotes.has(key)) previewNotes.delete(key);
    else previewNotes.add(key);
    // A render with the new state; the editor stays mounted underneath.
    setStats((value) => ({ ...value }));
  };

  /**
   * Click on a checkbox in the preview: the editor changes "[ ]" in the text. A click, not a
   * change event: React reports changes only for inputs it rendered itself, and Space on a
   * focused checkbox clicks it too.
   */
  const onPreviewClick = (event: React.MouseEvent<HTMLDivElement>) => {
    const target = event.target;
    if (!(target instanceof HTMLInputElement) || !target.classList.contains("md-task")) return;
    const line = Number(target.dataset["line"]);
    if (!Number.isInteger(line)) return;
    editor.current?.toggleTask(line);
    refocusTask.current = line;
    computeStats(textRef.current, currentRef.current);
  };

  const scopeChips: { scope: NotesScope; label: string; title: string }[] = [
    ...(sessionFolder !== null && sessionFolder.path !== null
      ? [
          {
            scope: { kind: "project" } as const,
            label: t("notes.scopeProject"),
            title: t("notes.scopeProjectTip", {
              folder: folderName(sessionFolder.path),
              path: sessionFolder.path,
            }),
          },
        ]
      : []),
    { scope: { kind: "all" }, label: t("notes.scopeAll"), title: t("notes.scopeAllTip") },
    { scope: { kind: "general" }, label: general, title: t("notes.scopeGeneralTip") },
  ];
  if (shownScope.kind === "folder" && shownScope.folder.path !== null) {
    scopeChips.push({
      scope: shownScope,
      label: folderName(shownScope.folder.path),
      title: shownScope.folder.path,
    });
  }

  const projectLabel = (folder: NoteFolder) =>
    folder.path === null ? general : folderName(folder.path);
  const searching = query.trim() !== "";
  const shownHits = searching && hits?.query === query.trim() ? hits.hits : null;
  const listed = listedKey === listKey;
  const scopeName =
    shownScope.kind === "project" && sessionFolder?.path != null
      ? folderName(sessionFolder.path)
      : shownScope.kind === "folder" && shownScope.folder.path !== null
        ? folderName(shownScope.folder.path)
        : null;

  /** Moves the open note between this computer and the shared notes of its project. */
  const moveCurrent = async (to: NoteFolder) => {
    const note = currentRef.current;
    if (note?.id == null) return;
    const from = note.folder;
    const id = note.id;
    try {
      await flush();
      const moved = await api.moveNote(from, id, to);
      if (typeof moved !== "string") return;
      autoTitled.delete(noteKey({ folder: from, id }));
      details.delete(noteKey({ folder: from, id }));
      props.onChanged(from);
      props.onChanged(to);
      loadShared();
      showToast({ message: t("sharedNotes.moved", { title: note.title }) });
      await openNote({ folder: to, id: moved }, false);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const moveItems: PopupItem[] =
    current?.id == null || current.folder.path === null
      ? []
      : isShared(current.folder)
        ? [
            {
              type: "action",
              key: "move-local",
              label: t("sharedNotes.moveToLocal"),
              run: () => void moveCurrent({ path: current.folder.path }),
            },
          ]
        : sharedPath !== undefined && project !== null && sameFolder(current.folder, project)
          ? [
              {
                type: "action",
                key: "move-shared",
                label: t("sharedNotes.moveToShared"),
                run: () => void moveCurrent(sharedFolder(project)),
              },
            ]
          : [];

  const moreItems: PopupItem[] =
    current?.id == null
      ? []
      : [
          {
            type: "action",
            key: "rename",
            label: t("notes.rename"),
            shortcut: "F2",
            run: () => {
              setRenaming(true);
            },
          },
          {
            type: "action",
            key: "copy",
            label: t("notes.copyMarkdown"),
            run: () => {
              void flush();
              writeClipboard(textRef.current)
                .then(() => {
                  showToast({ message: t("notes.copied") });
                })
                .catch((e: unknown) => {
                  setError(errorText(e));
                });
            },
          },
          {
            type: "action",
            key: "export",
            label: t("notes.export"),
            run: () => {
              const note = currentRef.current;
              if (note?.id == null) return;
              const id = note.id;
              void flush()
                .then(() => api.exportNote(note.folder, id))
                .then((path) => {
                  if (typeof path === "string")
                    showToast({ message: t("notes.exported", { path }) });
                })
                .catch((e: unknown) => {
                  setError(errorText(e));
                });
            },
          },
          ...moveItems,
          ...(isShared(current.folder)
            ? []
            : [
                {
                  type: "action" as const,
                  key: "reveal",
                  label: isMac() ? t("notes.revealFinder") : t("notes.revealExplorer"),
                  run: () => {
                    const note = currentRef.current;
                    if (note?.id == null) return;
                    const id = note.id;
                    void flush()
                      .then(() => api.revealNote(note.folder, id))
                      .catch((e: unknown) => {
                        setError(errorText(e));
                      });
                  },
                },
              ]),
          { type: "separator", key: "sep" },
          {
            type: "action",
            key: "delete",
            label: t("notes.delete"),
            run: () => void deleteCurrent(),
          },
        ];

  const listMoreItems: PopupItem[] = [
    { type: "action", key: "trash", label: t("notes.trash"), run: () => void openTrash() },
    {
      type: "action",
      key: "folder",
      label: t("notes.openFolder"),
      title: newNoteFolder(scope, sessionFolder).path ?? general,
      run: () => {
        api.revealNotesFolder(newNoteFolder(scope, sessionFolder)).catch((e: unknown) => {
          setError(errorText(e));
        });
      },
    },
  ];

  const sendItems: PopupItem[] = sessions.map((session) => ({
    type: "action",
    key: `send-${String(session.id)}`,
    label: session.title,
    current: session.id === defaultTarget?.id,
    run: () => {
      send(session.id);
    },
  }));

  const openMenu = (kind: "more" | "send" | "list", button: HTMLElement | null) => {
    if (!button) return;
    const rect = button.getBoundingClientRect();
    setMenu((open) => (open?.kind === kind ? null : { kind, x: rect.left, y: rect.bottom + 4 }));
  };

  /** Moves the focus `step` rows from row `index` (-1: the search field above the list). */
  const moveRowFocus = (index: number, step: 1 | -1) => {
    const ordered = [...rows.current.values()]
      .filter((row) => row.isConnected)
      .sort((a, b) => (a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1));
    const next = ordered[index + step];
    if (next) next.focus();
    else if (index + step < 0) searchInput.current?.focus();
  };

  const onRowKey = (event: React.KeyboardEvent, index: number) => {
    const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    moveRowFocus(index, step);
  };

  const rowsShown: (NoteRef & { excerpt: string; progress: ChecklistProgress | null })[] =
    shownHits !== null
      ? shownHits.map((hit) => ({
          ...hit,
          progress: details.get(noteKey(hit))?.progress ?? null,
        }))
      : list.map((note) => {
          const detail = details.get(noteKey(note));
          return { ...note, excerpt: detail?.excerpt ?? "", progress: detail?.progress ?? null };
        });

  const grouped = !searching && groupsScope(scope, sessionFolder) && showsGroups(sharedStatus);
  const sharedRows = rowsShown.filter((note) => isShared(note.folder));
  const localRows = rowsShown.filter((note) => !isShared(note.folder));
  const groupLine = sharedStatus === null ? null : sharedLine(sharedStatus);
  const groupHints = sharedStatus === null ? [] : sharedHints(sharedStatus);

  const progressText = (progress: ChecklistProgress) =>
    t("notes.progress", { done: progress.done, total: progress.total });
  const shownTitle =
    current === null
      ? ""
      : current.id === null
        ? (stats.autoTitle ?? t("notes.quickNote"))
        : (stats.autoTitle ?? current.title);

  const renderRow = (
    note: NoteRef & { excerpt: string; progress: ChecklistProgress | null },
    index: number,
  ) => {
    const key = noteKey(note);
    const time = note.updatedAt === null ? "" : relativeTime(note.updatedAt, now, language);
    const showProject = mixed || searching;
    const progress = note.progress !== null && note.progress.total > 0 ? note.progress : null;
    return (
      <li key={key}>
        <button
          type="button"
          className="note-row"
          ref={(element) => {
            if (element) rows.current.set(key, element);
            else rows.current.delete(key);
          }}
          title={note.folder.path ?? general}
          onClick={() => void openNote(note, true)}
          onKeyDown={(event) => {
            onRowKey(event, index);
          }}
        >
          <span className="note-row-head">
            {isShared(note.folder) && (
              <span className="note-row-lock" title={t("sharedNotes.lockLabel")}>
                <LockIcon size={12} />
                <span className="sr-only">{t("sharedNotes.lockLabel")}</span>
              </span>
            )}
            <span className="note-row-title">{note.title}</span>
            {progress !== null && (
              <span
                className={
                  progress.done === progress.total ? "note-progress complete" : "note-progress"
                }
                title={progressText(progress)}
              >
                <TaskIcon size={12} />
                <span aria-hidden="true">
                  {progress.done}/{progress.total}
                </span>
                <span className="sr-only">{progressText(progress)}</span>
              </span>
            )}
          </span>
          {note.excerpt !== "" && <span className="note-row-excerpt">{note.excerpt}</span>}
          <span className="note-row-meta">
            {showProject && <span className="note-row-project">{projectLabel(note.folder)}</span>}
            {showProject && time !== "" && <span aria-hidden="true"> · </span>}
            {time !== "" && <span>{time}</span>}
          </span>
        </button>
      </li>
    );
  };

  return (
    <div
      ref={root}
      className="notes"
      onKeyDown={(event) => {
        if (event.key === "F2" && view === "note" && current?.id != null) {
          event.preventDefault();
          setRenaming(true);
        }
      }}
      onBlur={(event) => {
        // Leaving the panel counts as leaving the note: title from the first line, saved now.
        const next = event.relatedTarget;
        if (next instanceof Node && root.current?.contains(next)) return;
        if (currentRef.current?.id != null) void leave();
      }}
    >
      {view === "list" && (
        <>
          <div className="notes-toolbar">
            <label className="notes-search">
              <SearchIcon size={14} />
              <input
                ref={searchInput}
                type="search"
                value={query}
                placeholder={t("notes.search")}
                aria-label={t("notes.search")}
                onChange={(event) => {
                  setQuery(event.currentTarget.value);
                }}
                onKeyDown={(event) => {
                  if (event.key === "Escape") {
                    event.preventDefault();
                    event.stopPropagation();
                    if (query !== "") setQuery("");
                    else props.onEscape();
                  } else if (event.key === "ArrowDown") {
                    event.preventDefault();
                    moveRowFocus(-1, 1);
                  }
                }}
              />
              {query !== "" && (
                <button
                  type="button"
                  className="icon-button notes-search-clear"
                  aria-label={t("notes.searchClear")}
                  title={t("notes.searchClear")}
                  onClick={() => {
                    setQuery("");
                    searchInput.current?.focus();
                  }}
                >
                  <CloseIcon size={12} />
                </button>
              )}
            </label>
            <button
              type="button"
              className="icon-button"
              aria-label={t("notes.new")}
              title={`${t("notes.new")} (${formatShortcut(SHORTCUT_LABELS.quickNote, t)})`}
              onClick={newNote}
            >
              <PlusIcon />
            </button>
            <button
              ref={listMoreButton}
              type="button"
              className="icon-button"
              aria-label={t("notes.more")}
              title={t("notes.more")}
              aria-haspopup="menu"
              aria-expanded={menu?.kind === "list"}
              onClick={() => {
                openMenu("list", listMoreButton.current);
              }}
            >
              <MoreIcon />
            </button>
          </div>
          {!searching && (
            <div className="notes-scopes" role="group" aria-label={t("notes.scopeLabel")}>
              {scopeChips.map((chip) => {
                const on = sameScope(chip.scope, shownScope);
                return (
                  <button
                    key={
                      chip.scope.kind === "folder"
                        ? `folder:${folderId(chip.scope.folder)}`
                        : chip.scope.kind
                    }
                    type="button"
                    className={on ? "scope-chip on" : "scope-chip"}
                    aria-pressed={on}
                    title={chip.title}
                    onClick={() => {
                      setScope(chip.scope);
                    }}
                  >
                    {chip.label}
                  </button>
                );
              })}
            </div>
          )}
          <div className="notes-list-wrap">
            {searching && shownHits !== null && shownHits.length === 0 && (
              <p className="notes-message muted">{t("notes.noHits", { query: query.trim() })}</p>
            )}
            {!grouped && !searching && listed && list.length === 0 && (
              <div className="notes-empty">
                <p>
                  {scopeName !== null
                    ? t("notes.emptyProject", { folder: scopeName })
                    : t("notes.empty")}
                </p>
                <button type="button" className="primary-button" onClick={newNote}>
                  {t("notes.new")}
                </button>
                <p className="notes-empty-hint">
                  {t("notes.emptyHint", { keys: formatShortcut(SHORTCUT_LABELS.quickNote, t) })}
                </p>
              </div>
            )}
            {grouped && (
              <>
                <section className="notes-group" aria-labelledby={sharedHeadId}>
                  <div className="notes-group-head">
                    <LockIcon size={12} />
                    <span id={sharedHeadId} className="notes-group-title">
                      {t("sharedNotes.group")}
                    </span>
                    <span className="notes-group-hint">{t("sharedNotes.groupHint")}</span>
                    {sharedStatus?.state === "unlocked" && sharedPath !== undefined && (
                      <button
                        type="button"
                        className="icon-button"
                        aria-label={t("sharedNotes.new")}
                        title={t("sharedNotes.new")}
                        onClick={() => {
                          setQuery("");
                          void startDraft(sharedFolder({ path: sharedPath }), true);
                        }}
                      >
                        <PlusIcon size={14} />
                      </button>
                    )}
                  </div>
                  {groupLine !== null ? (
                    <div className="notes-group-line">
                      <p>{t(groupLine.text)}</p>
                      {groupLine.action !== null && project !== null && (
                        <button
                          type="button"
                          className="secondary-button"
                          onClick={() => {
                            setSharedDialog({
                              mode: groupLine.action === "turnOn" ? "setup" : "unlock",
                              folder: project,
                            });
                          }}
                        >
                          {groupLine.action === "turnOn"
                            ? t("sharedNotes.turnOn")
                            : t("sharedNotes.unlock")}
                        </button>
                      )}
                    </div>
                  ) : sharedRows.length === 0 ? (
                    listed && <p className="notes-group-empty muted">{t("sharedNotes.empty")}</p>
                  ) : (
                    <ul className="notes-list" aria-labelledby={sharedHeadId}>
                      {sharedRows.map((note, index) => renderRow(note, index))}
                    </ul>
                  )}
                  {groupHints.map((hint) => (
                    <p key={hint.key} className="notes-group-note muted">
                      {t(hint.key, hint.params)}
                    </p>
                  ))}
                </section>
                <section className="notes-group" aria-labelledby={localHeadId}>
                  <div className="notes-group-head">
                    <span id={localHeadId} className="notes-group-title">
                      {t("sharedNotes.local")}
                    </span>
                    <button
                      type="button"
                      className="icon-button"
                      aria-label={t("notes.new")}
                      title={t("notes.new")}
                      onClick={newNote}
                    >
                      <PlusIcon size={14} />
                    </button>
                  </div>
                  {localRows.length === 0 ? (
                    listed && (
                      <p className="notes-group-empty muted">
                        {t("notes.emptyHint", {
                          keys: formatShortcut(SHORTCUT_LABELS.quickNote, t),
                        })}
                      </p>
                    )
                  ) : (
                    <ul className="notes-list" aria-labelledby={localHeadId}>
                      {localRows.map((note, index) => renderRow(note, sharedRows.length + index))}
                    </ul>
                  )}
                </section>
              </>
            )}
            {!grouped && rowsShown.length > 0 && (
              <ul className="notes-list" aria-label={t("notes.list")}>
                {rowsShown.map((note, index) => renderRow(note, index))}
              </ul>
            )}
          </div>
        </>
      )}

      {view === "trash" && (
        <>
          <div className="note-bar">
            <button
              ref={backButton}
              type="button"
              className="note-back"
              onClick={() => {
                setView("list");
                void reloadList();
                requestAnimationFrame(() => searchInput.current?.focus());
              }}
            >
              <ChevronIcon size={12} />
              <span>{t("notes.trash")}</span>
            </button>
          </div>
          <div className="notes-list-wrap">
            {trash !== null && trash.length === 0 && (
              <p className="notes-message muted">{t("notes.trashEmpty")}</p>
            )}
            {trash !== null && trash.length > 0 && (
              <ul className="notes-list trash-list" aria-label={t("notes.trash")}>
                {trash.map((item) => (
                  <li key={`${folderId(item.folder)}\n${item.id}`} className="trash-row">
                    <span className="trash-text">
                      <span className="note-row-title">{item.title}</span>
                      <span className="note-row-meta">
                        {mixed && <>{projectLabel(item.folder)} · </>}
                        {item.deletedAt !== null &&
                          t("notes.deletedAgo", {
                            time: relativeTime(item.deletedAt, now, language),
                          })}
                      </span>
                    </span>
                    <button
                      type="button"
                      className="secondary-button"
                      aria-label={t("notes.restoreNamed", { title: item.title })}
                      onClick={() => {
                        void restore(item.folder, item.id, item.title).then((restored) => {
                          if (restored === undefined) return;
                          setTrash((rows) => rows?.filter((row) => row !== item) ?? null);
                        });
                      }}
                    >
                      {t("notes.restore")}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </>
      )}

      {view === "note" && current !== null && (
        <div className="note-bar">
          <button
            ref={backButton}
            type="button"
            className="note-back"
            aria-label={t("notes.backToList")}
            title={t("notes.backToList")}
            onClick={() => void backToList(true)}
          >
            <ChevronIcon size={12} />
          </button>
          {renaming ? (
            <input
              className="note-rename"
              aria-label={t("notes.newTitle")}
              defaultValue={current.title}
              maxLength={200}
              autoFocus
              onFocus={(event) => {
                event.currentTarget.select();
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void commitRename(event.currentTarget.value).then(focusEditor);
                } else if (event.key === "Escape") {
                  event.preventDefault();
                  event.stopPropagation();
                  setRenaming(false);
                  focusEditor();
                }
              }}
              onBlur={(event) => {
                void commitRename(event.currentTarget.value);
              }}
            />
          ) : (
            <button
              type="button"
              className="note-title"
              disabled={current.id === null}
              title={current.id === null ? undefined : `${shownTitle}\n${t("notes.renameHint")}`}
              onClick={() => {
                setRenaming(true);
              }}
            >
              <span className={current.id === null && stats.autoTitle === null ? "muted" : ""}>
                {shownTitle}
              </span>
            </button>
          )}
          {stats.progress.total > 0 && (
            <span
              className={
                stats.progress.done === stats.progress.total
                  ? "note-progress complete"
                  : "note-progress"
              }
              title={progressText(stats.progress)}
            >
              <TaskIcon size={12} />
              <span aria-hidden="true">
                {stats.progress.done}/{stats.progress.total}
              </span>
              <span className="sr-only">{progressText(stats.progress)}</span>
            </span>
          )}
          <button
            ref={moreButton}
            type="button"
            className="icon-button"
            aria-label={t("notes.more")}
            title={t("notes.more")}
            aria-haspopup="menu"
            aria-expanded={menu?.kind === "more"}
            disabled={current.id === null}
            onClick={() => {
              openMenu("more", moreButton.current);
            }}
          >
            <MoreIcon />
          </button>
        </div>
      )}

      {error && (
        <div className="notes-error" role="alert">
          <span>{error}</span>
          <button
            type="button"
            className="icon-button"
            aria-label={t("error.dismiss")}
            onClick={() => {
              setError(null);
            }}
          >
            <CloseIcon size={12} />
          </button>
        </div>
      )}

      <div className="note-body" hidden={view !== "note" || current === null}>
        {preview && (
          <div
            className="note-preview selectable"
            role="region"
            tabIndex={0}
            aria-label={t("notes.previewOf", { title: shownTitle })}
            onClick={onPreviewClick}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                props.onEscape();
              }
            }}
            // Safe: markdownPreview.ts renders with html: false, without links or images; the only
            // elements it adds are the task checkboxes.
            dangerouslySetInnerHTML={{ __html: previewHtml ?? "" }}
          />
        )}
        <div className="note-editor-host" hidden={preview}>
          <NoteEditor
            loadKey={loadKey}
            text={text}
            label={t("notes.editorLabel", { title: shownTitle })}
            onChange={onEditorChange}
            onSelection={setSelected}
            onLoaded={onEditorLoaded}
            onEscape={props.onEscape}
            editorRef={editor}
          />
        </div>
        <div className="note-footer">
          <span className="note-words">{t("notes.words", { count: stats.words })}</span>
          <button
            type="button"
            className={preview ? "icon-button toggle on" : "icon-button toggle"}
            aria-pressed={preview}
            aria-label={t("notes.preview")}
            title={t("notes.preview")}
            disabled={current?.id == null}
            onClick={togglePreview}
          >
            <EyeIcon />
          </button>
          <div className="send-group">
            <button
              type="button"
              className="secondary-button send-button"
              disabled={!defaultTarget || stats.empty}
              title={defaultTarget ? `${sendLabel}\n${t("notes.sendHint")}` : sendLabel}
              onClick={() => {
                send(defaultTarget?.id ?? null);
              }}
            >
              <SendIcon size={14} />
              <span className="send-label">{sendLabel}</span>
            </button>
            {sessions.length > 1 && (
              <button
                ref={sendMenuButton}
                type="button"
                className="secondary-button send-pick"
                aria-label={t("notes.sendPick")}
                title={t("notes.sendPick")}
                aria-haspopup="menu"
                aria-expanded={menu?.kind === "send"}
                onClick={() => {
                  openMenu("send", sendMenuButton.current);
                }}
              >
                <ChevronIcon size={12} />
              </button>
            )}
          </div>
        </div>
      </div>

      {toast && (
        <div className="notes-toast" role="status">
          <span>{toast.message}</span>
          {toast.action && (
            <button type="button" className="secondary-button" onClick={toast.action.run}>
              {toast.action.label}
            </button>
          )}
          <button
            type="button"
            className="icon-button"
            aria-label={t("error.dismiss")}
            onClick={() => {
              setToast(null);
            }}
          >
            <CloseIcon size={12} />
          </button>
        </div>
      )}

      {menu && (
        <PopupMenu
          items={
            menu.kind === "more" ? moreItems : menu.kind === "send" ? sendItems : listMoreItems
          }
          label={menu.kind === "send" ? t("notes.sendPick") : t("notes.more")}
          position={{ x: menu.x, y: menu.y }}
          anchor={
            menu.kind === "more"
              ? moreButton
              : menu.kind === "send"
                ? sendMenuButton
                : listMoreButton
          }
          onClose={(reason) => {
            const kind = menu.kind;
            setMenu(null);
            if (reason === "dismiss") {
              (kind === "more"
                ? moreButton
                : kind === "send"
                  ? sendMenuButton
                  : listMoreButton
              ).current?.focus();
            }
          }}
        />
      )}
      <div className="sr-only" role="status">
        {toast ? "" : announcement}
      </div>
      {sharedDialog && (
        <SharedNotesDialog
          mode={sharedDialog.mode}
          folder={sharedDialog.folder}
          onDone={(status) => {
            setSharedDialog(null);
            if (projectId !== null) setShared({ project: projectId, status });
            requestAnimationFrame(() => searchInput.current?.focus());
          }}
          onClose={() => {
            setSharedDialog(null);
            requestAnimationFrame(() => searchInput.current?.focus());
          }}
        />
      )}
    </div>
  );
}

import { useCallback, useEffect, useRef, useState } from "react";

import { api, errorText, type NoteFolder } from "./api";
import { DICTIONARIES, LANGUAGES, useT } from "./i18n";
import { CloseIcon, EyeIcon, FolderIcon, PlusIcon, TrashIcon } from "./icons";
import { NoteEditor } from "./NoteEditor";
import {
  cleanTitle,
  folderId,
  folderLabel,
  isAutoTitle,
  MAX_NOTE_BYTES,
  moveNote,
  neighbourAfterDelete,
  parseNotes,
  sameFolder,
  titleFromText,
  untitledTitle,
  type NoteFolderInfo,
  type NoteInfo,
} from "./notes";
import { utf8Length } from "./workspace";

/** Value of the scope picker: the active session's folder or a fixed scope. */
export type NoteScope = { follow: true } | { follow: false; folder: NoteFolder };

interface Props {
  /** Scope being shown. */
  folder: NoteFolder;
  following: boolean;
  /** Folder of the active session, `null` without a session. */
  sessionFolder: NoteFolder | null;
  folders: readonly NoteFolderInfo[];
  width: number;
  /** Above the terminal instead of beside it (window too narrow, see placeNotes). */
  overlay?: boolean;
  /** Goes up when the editor should get focus (opened via shortcut or menu). */
  focusToken: number;
  onScope: (scope: NoteScope) => void;
  /** Notes created, deleted or renamed: reload the list of scopes. */
  onChanged: () => void;
  onClose: () => void;
  onFocusTerminal: () => void;
}

const SAVE_DELAY_MS = 500;
/** Change the title from the first line only after a typing pause, or the tab flickers. */
const AUTO_TITLE_DELAY_MS = 900;
/** How long deleting a note can be undone; only then does the backend delete it. */
const UNDO_DELETE_MS = 5000;

/** "Note" in every language: a title created in another language also counts as new. */
const UNTITLED_BASES = LANGUAGES.map((language) => DICTIONARIES[language]["notes.untitled"]);

/** Last opened note per scope, while Hangar is running. */
const lastNote = new Map<string, string>();
/** Preview on or off, while Hangar is running. */
let lastPreview = false;

/** Pending save of all open notes panels, e.g. before quitting. */
const flushers = new Set<() => Promise<void>>();

export function flushNotes(): Promise<void> {
  return Promise.all([...flushers].map((flush) => flush())).then(() => undefined);
}

interface PendingWrite {
  folder: NoteFolder;
  id: string;
  text: string;
}

/**
 * Notes panel to the right of the terminal: the scope at the top, the notes below it as tabs,
 * then the Markdown editor or the preview. Saving is automatic, shortly after the last input
 * and on switching.
 */
export function NotesPanel(props: Props) {
  const { folder, folders, sessionFolder, following } = props;
  const t = useT();
  const [notes, setNotes] = useState<NoteInfo[]>([]);
  /** Scope whose list is loaded; for any other the panel shows no empty state yet. */
  const [listedFid, setListedFid] = useState<string | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [loadKey, setLoadKey] = useState("");
  const [preview, setPreview] = useState(lastPreview);
  const [previewHtml, setPreviewHtml] = useState<string | null>(null);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  /** Note just deleted, as long as undo is possible; until then it is only hidden. */
  const [undoNote, setUndoNote] = useState<{ id: string; title: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const [dragView, setDragView] = useState<{ id: string; index: number } | null>(null);

  const folderRef = useRef(folder);
  const pending = useRef<PendingWrite | null>(null);
  const timer = useRef<number | null>(null);
  /** Note whose content is in the editor right now (may briefly lag behind `activeId`). */
  const editorNote = useRef<string | null>(null);
  /**
   * Scope of the note in the editor. After a scope switch, the old note stays in the editor until
   * the new list has loaded; input during that time belongs to its own scope, not the new one
   * (`folderRef`), or the text would end up under the wrong ID in the wrong folder.
   */
  const editorFolder = useRef<NoteFolder>(folder);
  /** Scope per load, until the editor confirms it with `onLoaded`. */
  const loadingFolder = useRef<{ key: string; folder: NoteFolder } | null>(null);
  const currentText = useRef("");
  const loadCounter = useRef(0);
  const focusEditor = useRef<(() => void) | null>(null);
  const focusAfterLoad = useRef(false);
  const tabs = useRef(new Map<string, HTMLButtonElement>());
  const drag = useRef<{ id: string; startX: number; mids: { id: string; mid: number }[] } | null>(
    null,
  );
  const dragMoved = useRef(false);
  /** Counts load requests; only the latest may fill the editor (quick clicking through tabs). */
  const openGeneration = useRef(0);
  /** Delete that goes to the backend after UNDO_DELETE_MS; carries its own scope along. */
  const pendingDelete = useRef<{ folder: NoteFolder; id: string; timer: number } | null>(null);
  /** Text the current title would be derived from (isAutoTitle). */
  const titleSource = useRef("");
  const titleTimer = useRef<number | null>(null);
  const notesRef = useRef(notes);
  const renamingRef = useRef(renamingId);
  const onChangedRef = useRef(props.onChanged);
  const fid = folderId(folder);
  const listed = listedFid === fid;
  const visibleNotes = undoNote ? notes.filter((note) => note.id !== undoNote.id) : notes;

  useEffect(() => {
    folderRef.current = folder;
    notesRef.current = notes;
    renamingRef.current = renamingId;
    onChangedRef.current = props.onChanged;
  });

  /** Run a pending delete right away (deadline passed, scope switch, quit). */
  const commitDelete = useCallback(async () => {
    const job = pendingDelete.current;
    if (!job) return;
    pendingDelete.current = null;
    window.clearTimeout(job.timer);
    setUndoNote((current) => (current?.id === job.id ? null : current));
    try {
      await api.deleteNote(job.folder, job.id);
      if (sameFolder(job.folder, folderRef.current)) {
        setNotes((list) => list.filter((note) => note.id !== job.id));
      }
      onChangedRef.current();
    } catch (e) {
      setError(errorText(e));
    }
  }, []);

  const flush = useCallback(async () => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
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
    } catch (e) {
      setError(t("notes.saveFailed", { error: errorText(e) }));
    }
  }, [t]);

  useEffect(() => {
    const flushAll = () => Promise.all([flush(), commitDelete()]).then(() => undefined);
    flushers.add(flushAll);
    return () => {
      flushers.delete(flushAll);
      if (titleTimer.current !== null) window.clearTimeout(titleTimer.current);
      void flushAll();
    };
  }, [flush, commitDelete]);

  /** Loads a note into the editor. `known`: the content is already known (new, empty note). */
  const openNote = useCallback(
    async (id: string | null, known?: string) => {
      openGeneration.current += 1;
      const generation = openGeneration.current;
      await flush();
      if (generation !== openGeneration.current) return;
      if (titleTimer.current !== null) {
        window.clearTimeout(titleTimer.current);
        titleTimer.current = null;
      }
      setActiveId(id);
      setRenamingId(null);
      setPreviewHtml(null);
      if (id === null) {
        editorNote.current = null;
        currentText.current = "";
        setText("");
        return;
      }
      lastNote.set(folderId(folderRef.current), id);
      let content = known;
      if (content === undefined) {
        try {
          const value = await api.readNote(folderRef.current, id);
          content = typeof value === "string" ? value : "";
        } catch (e) {
          if (generation === openGeneration.current) setError(errorText(e));
          return;
        }
        // Another note was requested in the meantime: this response is stale.
        if (generation !== openGeneration.current) return;
      }
      titleSource.current = content;
      currentText.current = content;
      setText(content);
      loadCounter.current += 1;
      const key = `${id}#${String(loadCounter.current)}`;
      loadingFolder.current = { key, folder: folderRef.current };
      setLoadKey(key);
    },
    [flush],
  );

  const reloadList = useCallback(async (): Promise<NoteInfo[]> => {
    const list = parseNotes(await api.listNotes(folderRef.current));
    setNotes(list);
    setListedFid(folderId(folderRef.current));
    return list;
  }, []);

  // Scope switched: save old changes, load the list, show the note that was open last.
  useEffect(() => {
    let current = true;
    // A pending save belongs to the old scope; it carries its own folder along.
    // Likewise a pending delete: the scope switch counts as confirming it.
    void Promise.resolve()
      .then(() => Promise.all([flush(), commitDelete()]))
      .then(() => api.listNotes(folderRef.current))
      .then((value) => {
        if (!current) return;
        const list = parseNotes(value);
        setNotes(list);
        setListedFid(fid);
        const remembered = lastNote.get(fid);
        const pick = list.find((note) => note.id === remembered)?.id ?? list[0]?.id ?? null;
        void openNote(pick);
      })
      .catch((e: unknown) => {
        if (current) setError(errorText(e));
      });
    return () => {
      current = false;
    };
  }, [fid, flush, commitDelete, openNote]);

  // Back in the window: if the note was changed elsewhere, the disk wins (only with no pending input).
  useEffect(() => {
    const onFocus = () => {
      const id = editorNote.current;
      const noteFolder = editorFolder.current;
      if (id === null || pending.current !== null) return;
      api
        .readNote(noteFolder, id)
        .then((value) => {
          if (typeof value !== "string" || value === currentText.current) return;
          if (editorNote.current !== id || pending.current !== null) return;
          titleSource.current = value;
          currentText.current = value;
          setText(value);
          loadCounter.current += 1;
          const key = `${id}#${String(loadCounter.current)}`;
          loadingFolder.current = { key, folder: noteFolder };
          setLoadKey(key);
        })
        .catch(() => undefined);
    };
    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
    };
  }, []);

  useEffect(() => {
    if (props.focusToken === 0) return;
    if (focusEditor.current && activeId !== null && !preview) focusEditor.current();
    else focusAfterLoad.current = true;
    // React to new requests only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.focusToken]);

  useEffect(() => {
    lastPreview = preview;
    if (!preview) return;
    let current = true;
    import("./markdownPreview")
      .then(({ renderMarkdown }) => {
        if (current) setPreviewHtml(renderMarkdown(currentText.current));
      })
      .catch(() => {
        if (current) setError(t("notes.editorFailed"));
      });
    return () => {
      current = false;
    };
  }, [preview, loadKey, t]);

  const onEditorChange = (next: string) => {
    const id = editorNote.current;
    if (id === null) return;
    currentText.current = next;
    pending.current = { folder: editorFolder.current, id, text: next };
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void flush(), SAVE_DELAY_MS);
    if (titleTimer.current !== null) window.clearTimeout(titleTimer.current);
    const noteFolder = editorFolder.current;
    titleTimer.current = window.setTimeout(() => {
      titleTimer.current = null;
      void followFirstLine(noteFolder, id, next);
    }, AUTO_TITLE_DELAY_MS);
  };

  /** The title follows the first line as long as you have not picked one yourself. */
  const followFirstLine = async (noteFolder: NoteFolder, id: string, next: string) => {
    if (editorNote.current !== id || renamingRef.current === id) return;
    const note = notesRef.current.find((n) => n.id === id);
    if (!note || !isAutoTitle(note.title, UNTITLED_BASES, titleSource.current)) return;
    const title = titleFromText(next);
    if (title === null || title === note.title) return;
    try {
      await api.renameNote(noteFolder, id, title);
      titleSource.current = next;
      if (sameFolder(noteFolder, folderRef.current)) {
        setNotes((list) => list.map((n) => (n.id === id ? { ...n, title } : n)));
      }
    } catch {
      // Then the current title stays; Hangar tries again on the next keystroke.
    }
  };

  const onEditorLoaded = (key: string) => {
    const id = key.split("#")[0] ?? "";
    editorNote.current = id === "" ? null : id;
    const loading = loadingFolder.current;
    if (loading?.key === key) editorFolder.current = loading.folder;
    if (focusAfterLoad.current && !preview) {
      focusAfterLoad.current = false;
      focusEditor.current?.();
    }
  };

  const createNote = async () => {
    try {
      await flush();
      const title = untitledTitle(
        t("notes.untitled"),
        notes.map((note) => note.title),
      );
      const id = await api.createNote(folderRef.current, title);
      if (typeof id !== "string") return;
      await reloadList();
      setPreview(false);
      focusAfterLoad.current = true;
      await openNote(id, "");
      props.onChanged();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const commitRename = async (id: string, value: string) => {
    setRenamingId(null);
    const note = notes.find((n) => n.id === id);
    if (!note || value.trim() === note.title) {
      tabs.current.get(id)?.focus();
      return;
    }
    const title = cleanTitle(value);
    if (title === null) {
      setError(t("notes.invalidTitle"));
      tabs.current.get(id)?.focus();
      return;
    }
    try {
      await api.renameNote(folderRef.current, id, title);
      setNotes((list) => list.map((n) => (n.id === id ? { ...n, title } : n)));
      props.onChanged();
    } catch (e) {
      setError(errorText(e));
    }
    tabs.current.get(id)?.focus();
  };

  /**
   * Delete without asking, but with undo: the note vanishes from the tabs at once, and the
   * backend moves it to the trash only after UNDO_DELETE_MS. The backend cannot restore notes;
   * this way the note stays untouched until then, and undo simply shows it again.
   */
  const requestDelete = async (id: string) => {
    const note = notes.find((n) => n.id === id);
    if (!note) return;
    // Save pending input so undo brings back the latest state.
    await flush();
    await commitDelete();
    const ids = visibleNotes.map((n) => n.id);
    const next = neighbourAfterDelete(ids, id);
    const timer = window.setTimeout(() => void commitDelete(), UNDO_DELETE_MS);
    pendingDelete.current = { folder: folderRef.current, id, timer };
    setUndoNote({ id, title: note.title });
    setAnnouncement(t("notes.deleted", { title: note.title }));
    if (activeId === id) await openNote(next);
    if (next !== null) tabs.current.get(next)?.focus();
  };

  const undoDelete = () => {
    const job = pendingDelete.current;
    if (!job) return;
    pendingDelete.current = null;
    window.clearTimeout(job.timer);
    setUndoNote(null);
    if (sameFolder(job.folder, folderRef.current)) {
      void openNote(job.id);
      requestAnimationFrame(() => tabs.current.get(job.id)?.focus());
    }
  };

  const reorder = (id: string, target: number) => {
    const ids = moveNote(
      visibleNotes.map((note) => note.id),
      id,
      target,
    );
    const byId = new Map(notes.map((note) => [note.id, note]));
    // A note that was just deleted (and can still be undone) stays at the end of the list.
    const hidden = notes.filter((note) => !ids.includes(note.id));
    setNotes([...ids.flatMap((noteId) => byId.get(noteId) ?? []), ...hidden]);
    const note = byId.get(id);
    if (note) {
      setAnnouncement(
        t("notes.moved", { title: note.title, position: ids.indexOf(id) + 1, count: ids.length }),
      );
    }
    api.reorderNotes(folderRef.current, ids).catch((e: unknown) => {
      setError(errorText(e));
    });
  };

  const selectTab = (id: string, focus: boolean) => {
    // Clicking the active tab rereads it too: that is how outside changes come in.
    void openNote(id);
    if (focus) tabs.current.get(id)?.focus();
  };

  const onTabKey = (event: React.KeyboardEvent, id: string) => {
    if (renamingId !== null) return;
    const ids = visibleNotes.map((note) => note.id);
    const index = ids.indexOf(id);
    const move = event.ctrlKey && event.shiftKey;
    let target: number;
    switch (event.key) {
      case "ArrowLeft":
        target = index - 1;
        break;
      case "ArrowRight":
        target = index + 1;
        break;
      case "Home":
        target = 0;
        break;
      case "End":
        target = ids.length - 1;
        break;
      case "F2":
        event.preventDefault();
        setRenamingId(id);
        return;
      case "Delete":
        event.preventDefault();
        void requestDelete(id);
        return;
      case "Enter":
      case "ArrowDown":
        event.preventDefault();
        if (!preview) focusEditor.current?.();
        return;
      default:
        return;
    }
    event.preventDefault();
    if (target < 0 || target >= ids.length) return;
    if (move) {
      reorder(id, target);
      requestAnimationFrame(() => tabs.current.get(id)?.focus());
      return;
    }
    const next = ids[target];
    if (next !== undefined) selectTab(next, true);
  };

  const onTabPointerDown = (event: React.PointerEvent<HTMLButtonElement>, id: string) => {
    if (event.button !== 0 || renamingId !== null) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    dragMoved.current = false;
    drag.current = {
      id,
      startX: event.clientX,
      mids: visibleNotes.map((note) => {
        const rect = tabs.current.get(note.id)?.getBoundingClientRect();
        return { id: note.id, mid: rect ? rect.left + rect.width / 2 : 0 };
      }),
    };
  };

  const onTabPointerMove = (event: React.PointerEvent<HTMLButtonElement>) => {
    const current = drag.current;
    if (!current) return;
    if (!dragMoved.current && Math.abs(event.clientX - current.startX) < 6) return;
    dragMoved.current = true;
    const index = current.mids.filter((m) => m.id !== current.id && m.mid < event.clientX).length;
    setDragView((view) =>
      view?.id === current.id && view.index === index ? view : { id: current.id, index },
    );
  };

  const endDrag = (commit: boolean) => {
    const current = drag.current;
    drag.current = null;
    if (current && commit && dragView && dragMoved.current) {
      const from = visibleNotes.findIndex((note) => note.id === current.id);
      if (dragView.index !== from) reorder(current.id, dragView.index);
    }
    setDragView(null);
  };

  const shown = dragView
    ? moveNote(
        visibleNotes.map((note) => note.id),
        dragView.id,
        dragView.index,
      ).flatMap((id) => visibleNotes.filter((note) => note.id === id))
    : visibleNotes;
  const noNotes = listed && visibleNotes.length === 0;
  const activeNote = notes.find((note) => note.id === activeId);
  const general = t("notes.general");

  // Scope picker: the active session's folder, No folder, further scopes that have notes.
  const options: { value: string; label: string; scope: NoteScope }[] = [];
  if (sessionFolder !== null) {
    options.push({
      value: "follow",
      label: t("notes.activeFolder", { folder: folderLabel(sessionFolder, general) }),
      scope: { follow: true },
    });
  }
  const seen = new Set(sessionFolder === null ? [] : [folderId(sessionFolder)]);
  const addFolder = (other: NoteFolder) => {
    const id = folderId(other);
    if (seen.has(id)) return;
    seen.add(id);
    options.push({
      value: `folder:${id}`,
      label: folderLabel(other, general),
      scope: { follow: false, folder: other },
    });
  };
  addFolder({ path: null });
  if (!following) addFolder(folder);
  for (const info of folders) if (info.noteCount > 0) addFolder({ path: info.path });
  const selected =
    following && sessionFolder !== null && sameFolder(folder, sessionFolder)
      ? "follow"
      : `folder:${fid}`;

  return (
    <aside
      className={props.overlay === true ? "notes floating" : "notes"}
      style={{ width: props.width }}
      aria-label={t("notes.title")}
    >
      <div className="notes-header">
        <select
          className="select notes-scope"
          aria-label={t("notes.scope")}
          title={folder.path ?? general}
          value={selected}
          onChange={(event) => {
            const option = options.find((o) => o.value === event.currentTarget.value);
            if (option) props.onScope(option.scope);
          }}
        >
          {options.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
        <button
          type="button"
          className={preview ? "icon-button toggle on" : "icon-button toggle"}
          aria-pressed={preview}
          aria-label={t("notes.preview")}
          title={t("notes.preview")}
          disabled={activeId === null}
          onClick={() => {
            setPreview((value) => !value);
          }}
        >
          <EyeIcon />
        </button>
        <button
          type="button"
          className="icon-button"
          aria-label={t("notes.openFolder")}
          title={t("notes.openFolder")}
          onClick={() => {
            api.revealNotesFolder(folderRef.current).catch((e: unknown) => {
              setError(errorText(e));
            });
          }}
        >
          <FolderIcon />
        </button>
        {/* Delete applies to the open note and sits apart from the tabs: no accidental hits. */}
        <button
          type="button"
          className="icon-button"
          aria-label={
            activeNote ? t("notes.deleteNamed", { title: activeNote.title }) : t("notes.delete")
          }
          title={
            activeNote ? t("notes.deleteNamed", { title: activeNote.title }) : t("notes.delete")
          }
          disabled={!activeNote}
          onClick={() => {
            if (activeNote) void requestDelete(activeNote.id);
          }}
        >
          <TrashIcon />
        </button>
        <button
          type="button"
          className="icon-button"
          aria-label={t("notes.hide")}
          title={t("notes.hide")}
          onClick={props.onClose}
        >
          <CloseIcon />
        </button>
      </div>

      <div className="note-tabbar">
        <div
          className={dragView ? "note-tabs dragging-active" : "note-tabs"}
          role="tablist"
          aria-label={t("notes.title")}
        >
          {shown.map((note) => {
            const active = note.id === activeId;
            const className = [
              "note-tab",
              active ? "active" : "",
              dragView?.id === note.id ? "dragging" : "",
            ]
              .filter(Boolean)
              .join(" ");
            return (
              <div key={note.id} className={className} role="presentation">
                {renamingId === note.id ? (
                  <input
                    className="note-rename"
                    aria-label={t("notes.newTitle")}
                    defaultValue={note.title}
                    maxLength={200}
                    autoFocus
                    onFocus={(event) => {
                      event.currentTarget.select();
                    }}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        event.preventDefault();
                        void commitRename(note.id, event.currentTarget.value);
                      } else if (event.key === "Escape") {
                        event.preventDefault();
                        event.stopPropagation();
                        setRenamingId(null);
                        tabs.current.get(note.id)?.focus();
                      }
                    }}
                    onBlur={(event) => {
                      void commitRename(note.id, event.currentTarget.value);
                    }}
                  />
                ) : (
                  <button
                    type="button"
                    role="tab"
                    id={`note-tab-${note.id}`}
                    className="note-tab-main"
                    aria-selected={active}
                    aria-controls="note-panel"
                    tabIndex={active || (activeId === null && note === shown[0]) ? 0 : -1}
                    title={`${note.title}\n${t("notes.tabHint")}`}
                    ref={(element) => {
                      if (element) tabs.current.set(note.id, element);
                      else tabs.current.delete(note.id);
                    }}
                    onClick={() => {
                      if (dragMoved.current) {
                        dragMoved.current = false;
                        return;
                      }
                      selectTab(note.id, false);
                    }}
                    onDoubleClick={() => {
                      setRenamingId(note.id);
                    }}
                    onKeyDown={(event) => {
                      onTabKey(event, note.id);
                    }}
                    onPointerDown={(event) => {
                      onTabPointerDown(event, note.id);
                    }}
                    onPointerMove={onTabPointerMove}
                    onPointerUp={() => {
                      endDrag(true);
                    }}
                    onPointerCancel={() => {
                      endDrag(false);
                    }}
                  >
                    {note.title}
                  </button>
                )}
              </div>
            );
          })}
        </div>
        {/* Without notes, "New note" stands large in the empty panel, not twice. */}
        {!noNotes && (
          <button
            type="button"
            className="icon-button note-new"
            aria-label={t("notes.new")}
            title={t("notes.new")}
            onClick={() => void createNote()}
          >
            <PlusIcon />
          </button>
        )}
      </div>

      {undoNote && (
        <div className="notes-undo">
          <span>{t("notes.deleted", { title: undoNote.title })}</span>
          <button type="button" className="secondary-button" onClick={undoDelete}>
            {t("notes.undo")}
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

      <div
        className="note-body"
        id="note-panel"
        role="tabpanel"
        aria-labelledby={activeNote ? `note-tab-${activeNote.id}` : undefined}
        aria-label={activeNote ? undefined : t("notes.title")}
      >
        {noNotes && (
          <div className="notes-empty">
            <p>{t("notes.empty")}</p>
            <button type="button" className="primary-button" onClick={() => void createNote()}>
              {t("notes.new")}
            </button>
          </div>
        )}
        {preview && activeId !== null && (
          <div
            className="note-preview selectable"
            tabIndex={0}
            aria-label={t("notes.previewOf", { title: activeNote?.title ?? "" })}
            // Safe: markdownPreview.ts renders with html: false and without links or images.
            dangerouslySetInnerHTML={{ __html: previewHtml ?? "" }}
          />
        )}
        <div className="note-editor-host" hidden={preview || activeId === null}>
          <NoteEditor
            loadKey={loadKey}
            text={text}
            label={t("notes.editorLabel", { title: activeNote?.title ?? "" })}
            onChange={onEditorChange}
            onLoaded={onEditorLoaded}
            onEscape={props.onFocusTerminal}
            focusRef={focusEditor}
          />
        </div>
      </div>
      <div className="sr-only" role="status">
        {announcement}
      </div>
    </aside>
  );
}

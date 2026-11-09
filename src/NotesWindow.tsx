// The detached notes window (`index.html?window=notes`): only the notes, nothing else of the app.
// It may only listen to events; what it asks of the main window goes through `notes_to_main`
// (notesBridge.ts). Language, theme and the session context come from localStorage, which it
// shares with the main window.

import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { api, type NoteFolder } from "./api";
import { createTranslator, detectLanguage, I18nContext, setCurrentLanguage } from "./i18n";
import { PinIcon } from "./icons";
import { parseNoteFolders, type NoteFolderInfo } from "./notes";
import {
  CONTEXT_KEY,
  loadContext,
  loadWindowPrefs,
  NOTES_COMMAND,
  parseNotesCommand,
  saveWindowPrefs,
  WINDOW_PREFS_KEY,
} from "./notesBridge";
import { flushNotes, NotesPanel, type NotesHandle, type NotesRequest } from "./NotesPanel";
import { folderKeyOf } from "./notesScope";
import { loadSettings } from "./settings";
import { focusCycleStep, matchShortcut, nextFocusZone } from "./shortcuts";
import { applyDocumentTheme, resolveTheme, systemPrefersDark } from "./theme";

function readSettings() {
  return loadSettings(detectLanguage(navigator.language));
}

export function NotesWindow() {
  const [settings, setSettings] = useState(readSettings);
  const [context, setContext] = useState(loadContext);
  const [onTop, setOnTop] = useState(() => loadWindowPrefs().onTop);
  const [folders, setFolders] = useState<NoteFolderInfo[]>([]);
  const [request, setRequest] = useState<NotesRequest | null>(null);
  const [refreshToken, setRefreshToken] = useState(0);
  const handle = useRef<NotesHandle | null>(null);
  const tokens = useRef(0);
  /** A new request for the panel, with a token it has not seen. */
  const ask = useCallback((make: (token: number) => NotesRequest) => {
    tokens.current += 1;
    setRequest(make(tokens.current));
  }, []);
  /** The app is quitting: closing this window then does not dock the notes back. */
  const quitting = useRef(false);
  const t = useMemo(() => createTranslator(settings.language), [settings.language]);

  useEffect(() => {
    setCurrentLanguage(settings.language);
    document.documentElement.lang = settings.language;
    document.title = `${t("notes.title")} · Hangar`;
    applyDocumentTheme(resolveTheme(settings.theme, systemPrefersDark()));
  }, [settings, t]);

  const refreshFolders = useCallback(async (): Promise<NoteFolderInfo[]> => {
    try {
      const list = parseNoteFolders(await api.listNoteFolders());
      setFolders(list);
      return list;
    } catch {
      return [];
    }
  }, []);

  useEffect(() => {
    void Promise.resolve().then(refreshFolders);
  }, [refreshFolders]);

  // The main window writes settings and the session context; the storage event brings them here.
  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key === CONTEXT_KEY) setContext(loadContext());
      else if (event.key === WINDOW_PREFS_KEY) setOnTop(loadWindowPrefs().onTop);
      else setSettings(readSettings());
    };
    window.addEventListener("storage", onStorage);
    return () => {
      window.removeEventListener("storage", onStorage);
    };
  }, []);

  // Commands from the main window.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    listen(NOTES_COMMAND, (event) => {
      const command = parseNotesCommand(event.payload);
      if (!command) return;
      switch (command.kind) {
        case "quickNote":
          ask((token) => ({ token, kind: "quick" }));
          return;
        case "capture":
          ask((token) => ({ token, kind: "capture", text: command.text, source: command.source }));
          return;
        case "refresh":
          void refreshFolders();
          setRefreshToken((value) => value + 1);
          return;
        case "flush":
          quitting.current = true;
          // The answer tells the main window that everything is saved.
          void flushNotes().then(() =>
            api.notesToMain({ kind: "notesChanged", folderKey: "general" }),
          );
          return;
        case "close":
          void flushNotes()
            .catch(() => undefined)
            .then(() => getCurrentWindow().close());
          return;
      }
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refreshFolders, ask]);

  // Closing this window brings the notes back to the side of the main window.
  useEffect(() => {
    const onUnload = () => {
      void flushNotes();
      if (!quitting.current) saveWindowPrefs({ ...loadWindowPrefs(), detached: false });
    };
    window.addEventListener("pagehide", onUnload);
    return () => {
      window.removeEventListener("pagehide", onUnload);
    };
  }, []);

  // Shortcuts that make sense here: quick note and F6 between list and editor.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (matchShortcut(event) === "quickNote") {
        event.preventDefault();
        ask((token) => ({ token, kind: "quick" }));
        return;
      }
      const step = focusCycleStep(event);
      const panel = handle.current;
      if (step === null || !panel) return;
      event.preventDefault();
      const zone = nextFocusZone(panel.zones(), panel.zone(), step);
      if (zone) panel.focusZone(zone);
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
    };
  }, [ask]);

  const toggleOnTop = () => {
    const next = !onTop;
    setOnTop(next);
    saveWindowPrefs({ ...loadWindowPrefs(), onTop: next });
    getCurrentWindow()
      .setAlwaysOnTop(next)
      .catch(() => undefined);
  };

  const onChanged = (folder: NoteFolder) => {
    void refreshFolders().then((list) => {
      const key = folderKeyOf(folder, list) ?? "general";
      api.notesToMain({ kind: "notesChanged", folderKey: key }).catch(() => undefined);
    });
  };

  const onSend = (text: string, sessionId: number | null) => {
    api
      .notesToMain({ kind: "sendToTerminal", text, sessionId })
      .then(() => api.notesToMain({ kind: "showMain" }))
      .catch(() => undefined);
  };

  const sessionFolder: NoteFolder | null =
    context.sessionFolder === null ? null : { path: context.sessionFolder };

  return (
    <I18nContext.Provider value={t}>
      <div className="notes-window">
        <div className="side-header">
          <h1 className="notes-window-title">{t("notes.title")}</h1>
          <button
            type="button"
            className={onTop ? "icon-button toggle on" : "icon-button toggle"}
            aria-pressed={onTop}
            aria-label={t("notes.onTop")}
            title={t("notes.onTop")}
            onClick={toggleOnTop}
          >
            <PinIcon />
          </button>
        </div>
        <NotesPanel
          language={settings.language}
          sessionFolder={sessionFolder}
          folders={folders}
          sessions={context.sessions}
          activeSessionId={context.activeId}
          request={request}
          refreshToken={refreshToken}
          handle={handle}
          onSend={onSend}
          onChanged={onChanged}
          onEscape={() => undefined}
        />
      </div>
    </I18nContext.Provider>
  );
}

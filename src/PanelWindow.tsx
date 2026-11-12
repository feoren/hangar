// The detached right panel (`index.html?window=panel`, ADR 0007): the tabs Notes, Changes and
// Files in a window of their own. The main window stays the single source of truth: it pushes the
// active session's context (`panel-context`) and commands (`panel-command`); this window only
// listens and asks through `panel_to_main` (panelBridge.ts). Language and theme come from
// localStorage, which it shares with the main window.

import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { api, type NoteFolder } from "./api";
import { ChangesPanel } from "./ChangesPanel";
import { claimsOf, overlapsBySession, type Overlap } from "./claims";
import { FilesPanel } from "./FilesPanel";
import type { ChangesView } from "./gitHistory";
import { createTranslator, detectLanguage, I18nContext, setCurrentLanguage } from "./i18n";
import { PinIcon } from "./icons";
import { SIDE_TAB_LABELS, type SideTab } from "./layout";
import { parseNoteFolders, type NoteFolderInfo } from "./notes";
import { flushNotes, NotesPanel, type NotesHandle, type NotesRequest } from "./NotesPanel";
import { folderKeyOf } from "./notesScope";
import {
  EMPTY_PANEL_CONTEXT,
  loadWindowPrefs,
  PANEL_COMMAND,
  PANEL_CONTEXT,
  panelSessionName,
  parsePanelCommand,
  parsePanelContext,
  updateWindowPrefs,
  WINDOW_PREFS_KEY,
} from "./panelBridge";
import { loadSettings } from "./settings";
import { focusCycleStep, matchShortcut, nextFocusZone } from "./shortcuts";
import { SidePanel } from "./SidePanel";
import { teammateOverlapsBySession, type TeammateOverlap } from "./team";
import { applyDocumentTheme, resolveTheme, subscribeSystemTheme, systemPrefersDark } from "./theme";

/** Moving or resizing saves the place after this quiet time. */
const BOUNDS_SAVE_MS = 400;
const NO_OVERLAPS: readonly Overlap[] = [];
const NO_TEAMMATES: readonly TeammateOverlap[] = [];

function readSettings() {
  return loadSettings(detectLanguage(navigator.language));
}

/** Listens to a Tauri event for the lifetime of the effect that calls it. */
function useTauriEvent(name: string, handler: (payload: unknown) => void, onReady?: () => void) {
  const current = useRef(handler);
  useEffect(() => {
    current.current = handler;
  });
  const ready = useRef(onReady);
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    listen(name, (event) => {
      current.current(event.payload);
    })
      .then((stop) => {
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
        ready.current?.();
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [name]);
}

export function PanelWindow() {
  const [settings, setSettings] = useState(readSettings);
  const [context, setContext] = useState(EMPTY_PANEL_CONTEXT);
  const [onTop, setOnTop] = useState(() => loadWindowPrefs().onTop);
  const [tab, setTab] = useState<SideTab>(() => loadWindowPrefs().tab);
  const [folders, setFolders] = useState<NoteFolderInfo[]>([]);
  const [request, setRequest] = useState<NotesRequest | null>(null);
  const [refreshToken, setRefreshToken] = useState(0);
  const [changesFocusToken, setChangesFocusToken] = useState(0);
  const [filesFocusToken, setFilesFocusToken] = useState(0);
  const [changesView, setChangesView] = useState<ChangesView>("open");
  const handle = useRef<NotesHandle | null>(null);
  const tokens = useRef(0);
  /** A new request for the notes, with a token they have not seen. */
  const ask = useCallback((make: (token: number) => NotesRequest) => {
    tokens.current += 1;
    setRequest(make(tokens.current));
  }, []);
  /** The app is quitting: closing this window then does not dock the panel back. */
  const quitting = useRef(false);
  const t = useMemo(() => createTranslator(settings.language), [settings.language]);
  /** "System" theme: follows the system mode while the window is open, as the main window does. */
  const systemDark = useSyncExternalStore(subscribeSystemTheme, systemPrefersDark);

  useEffect(() => {
    setCurrentLanguage(settings.language);
    document.documentElement.lang = settings.language;
    document.title = `${t(SIDE_TAB_LABELS[tab])} · Hangar`;
    applyDocumentTheme(resolveTheme(settings.theme, systemDark));
  }, [settings, t, tab, systemDark]);

  /** Shows a tab and gives it the focus, as the main window's shortcuts do. */
  const showTab = useCallback((next: SideTab) => {
    setTab(next);
    updateWindowPrefs({ tab: next });
    if (next === "changes") setChangesFocusToken((value) => value + 1);
    if (next === "files") setFilesFocusToken((value) => value + 1);
  }, []);

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

  // The main window writes settings; the storage event brings them here.
  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key === WINDOW_PREFS_KEY) setOnTop(loadWindowPrefs().onTop);
      else setSettings(readSettings());
    };
    window.addEventListener("storage", onStorage);
    return () => {
      window.removeEventListener("storage", onStorage);
    };
  }, []);

  // The context: once listening, this window asks for it; afterwards every change arrives.
  useTauriEvent(
    PANEL_CONTEXT,
    (payload) => {
      const next = parsePanelContext(payload);
      if (next) setContext(next);
    },
    () => {
      api.panelToMain({ kind: "ready" }).catch(() => undefined);
    },
  );

  useTauriEvent(PANEL_COMMAND, (payload) => {
    const command = parsePanelCommand(payload);
    if (!command) return;
    switch (command.kind) {
      case "quickNote":
        showTab("notes");
        ask((token) => ({ token, kind: "quick" }));
        return;
      case "capture":
        showTab("notes");
        ask((token) => ({ token, kind: "capture", text: command.text, source: command.source }));
        return;
      case "show":
        showTab(command.tab);
        if (command.tab === "notes") ask((token) => ({ token, kind: "focus" }));
        return;
      case "refresh":
        void refreshFolders();
        setRefreshToken((value) => value + 1);
        return;
      case "flush":
        quitting.current = true;
        // The answer tells the main window that everything is saved.
        void flushNotes().then(() =>
          api.panelToMain({ kind: "notesChanged", folderKey: "general" }),
        );
        return;
    }
  });

  // Closing this window docks the panel back into the main window. The handler is awaited before
  // Tauri destroys the window, so pending input is written first; `pagehide` gave no such
  // guarantee. A failed save must not keep the window from closing.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    getCurrentWindow()
      .onCloseRequested(async () => {
        await flushNotes().catch(() => undefined);
        if (!quitting.current) updateWindowPrefs({ detached: false });
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
  }, []);

  // Where the window is, in logical pixels, for the next time it opens. The move and resize
  // events only say that something changed; screenX and innerWidth are already logical.
  useEffect(() => {
    let timer: number | undefined;
    const save = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        if (window.innerWidth <= 0 || window.innerHeight <= 0) return;
        updateWindowPrefs({
          bounds: {
            x: window.screenX,
            y: window.screenY,
            width: window.innerWidth,
            height: window.innerHeight,
          },
        });
      }, BOUNDS_SAVE_MS);
    };
    const stops: (() => void)[] = [];
    let disposed = false;
    const keep = (stop: () => void) => {
      if (disposed) stop();
      else stops.push(stop);
    };
    const current = getCurrentWindow();
    current
      .onMoved(save)
      .then(keep)
      .catch(() => undefined);
    current
      .onResized(save)
      .then(keep)
      .catch(() => undefined);
    return () => {
      disposed = true;
      window.clearTimeout(timer);
      for (const stop of stops) stop();
    };
  }, []);

  // Shortcuts that make sense here: quick note, the changes, F6 between list and editor.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const shortcut = matchShortcut(event);
      if (shortcut === "quickNote") {
        event.preventDefault();
        showTab("notes");
        ask((token) => ({ token, kind: "quick" }));
        return;
      }
      if (shortcut === "toggleChanges") {
        event.preventDefault();
        showTab("changes");
        return;
      }
      const step = focusCycleStep(event);
      const panel = handle.current;
      if (step === null || !panel || tab !== "notes") return;
      event.preventDefault();
      const zone = nextFocusZone(panel.zones(), panel.zone(), step);
      if (zone) panel.focusZone(zone);
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
    };
  }, [ask, showTab, tab]);

  const toggleOnTop = () => {
    const next = !onTop;
    setOnTop(next);
    updateWindowPrefs({ onTop: next });
    api.setPanelOnTop(next).catch(() => undefined);
  };

  const dock = () => {
    getCurrentWindow()
      .close()
      .catch(() => undefined);
  };

  const onChanged = (folder: NoteFolder) => {
    void refreshFolders().then((list) => {
      const key = folderKeyOf(folder, list) ?? "general";
      api.panelToMain({ kind: "notesChanged", folderKey: key }).catch(() => undefined);
    });
  };

  const onSend = (text: string, sessionId: number | null) => {
    api
      .panelToMain({ kind: "sendToTerminal", text, sessionId })
      .then(() => api.panelToMain({ kind: "showMain" }))
      .catch(() => undefined);
  };

  const { activeId, cwd, sessions, claims, activities } = context;
  const sendTargets = useMemo(
    () => sessions.filter((s) => s.running).map((s) => ({ id: s.id, title: s.title })),
    [sessions],
  );
  const overlapsById = useMemo(() => overlapsBySession(claims), [claims]);
  const teammatesById = useMemo(() => teammateOverlapsBySession(activities), [activities]);
  const nameOf = useCallback((id: number) => panelSessionName(sessions, id), [sessions]);
  const sessionFolder: NoteFolder | null = cwd === null ? null : { path: cwd };

  return (
    <I18nContext.Provider value={t}>
      <div className="panel-window">
        <SidePanel
          tab={tab}
          onTab={showTab}
          mode="window"
          onDock={dock}
          onEscape={() => undefined}
          actions={
            <button
              type="button"
              className={onTop ? "icon-button toggle on" : "icon-button toggle"}
              aria-pressed={onTop}
              aria-label={t("side.onTop")}
              title={t("side.onTop")}
              onClick={toggleOnTop}
            >
              <PinIcon />
            </button>
          }
          notes={
            <NotesPanel
              language={settings.language}
              sessionFolder={sessionFolder}
              folders={folders}
              sessions={sendTargets}
              activeSessionId={activeId}
              request={request}
              refreshToken={refreshToken}
              handle={handle}
              onSend={onSend}
              onChanged={onChanged}
              onEscape={() => undefined}
            />
          }
          changes={
            <ChangesPanel
              claims={activeId === null ? null : claimsOf(claims, activeId)}
              overlaps={
                activeId === null ? NO_OVERLAPS : (overlapsById.get(activeId) ?? NO_OVERLAPS)
              }
              teammates={
                activeId === null ? NO_TEAMMATES : (teammatesById.get(activeId) ?? NO_TEAMMATES)
              }
              nameOf={nameOf}
              onShowSession={(id) => {
                api.panelToMain({ kind: "showSession", sessionId: id }).catch(() => undefined);
              }}
              cwd={cwd}
              trigger={context.trigger}
              focusToken={changesFocusToken}
              view={changesView}
              onView={setChangesView}
              language={settings.language}
            />
          }
          files={
            <FilesPanel
              session={activeId === null || cwd === null ? null : { id: activeId, cwd }}
              claims={claims}
              activities={activities}
              nameOf={nameOf}
              focusToken={filesFocusToken}
              onMention={(text) => {
                onSend(text, null);
              }}
            />
          }
        />
      </div>
    </I18nContext.Provider>
  );
}

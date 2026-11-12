// The file viewer window (`index.html?window=file`, opened by `open_file_viewer`): one file of a
// session's folder, read-only, with line numbers and highlighting as in a code editor, a preview
// for Markdown and images, and the file's changes against the last commit. It may only read its
// own file (`file_viewer_content`, `file_viewer_stamp`, `file_viewer_diff`), open a checked web
// link (`file_viewer_open_link`), listen to events and copy text. The file is read again when it
// changes on disk (live refresh, `pollStamps`). Language, app theme, viewer theme and font come
// from the settings in localStorage, which it shares with the main window; the `storage` event
// brings changes over while it is open.

import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";

import {
  api,
  errorText,
  parseFileContent,
  parseFileStamp,
  parseHeadDiff,
  type FileContent,
  type HeadDiff,
} from "./api";
import { writeClipboard } from "./clipboard";
import {
  diffNote,
  editorPhrases,
  initialMode,
  MISSING_STAMP,
  pollStamps,
  stampKey,
  viewerBody,
  viewerModes,
  diffWorks,
  refreshInterval,
  type ViewerMode,
} from "./fileViewer";
import { formatBytes } from "./format";
import { DIFF_PAGE_LINES, lineKind, splitDiff } from "./gitDiff";
import { createTranslator, detectLanguage, I18nContext, setCurrentLanguage, useT } from "./i18n";
import {
  CheckIcon,
  CopyIcon,
  EyeIcon,
  FileIcon,
  RestartIcon,
  SearchIcon,
  SourceControlIcon,
} from "./icons";
import { revealInvisible } from "./invisible";
import { Revealed } from "./Revealed";
import { loadSettings } from "./settings";
import { fontStack } from "./terminalLook";
import { applyDocumentTheme, resolveTheme, subscribeSystemTheme, systemPrefersDark } from "./theme";
import type { FileView } from "./viewerEditor";
import { viewerCssVariables, viewerPalette } from "./viewerThemes";

/** How long the copy button shows its check mark. */
const COPIED_MS = 1500;
/** How long a link that did not open stays in the window. */
const LINK_ERROR_MS = 6000;

function readSettings() {
  return loadSettings(detectLanguage(navigator.language));
}

export function FileViewerWindow() {
  const [settings, setSettings] = useState(readSettings);
  const systemDark = useSyncExternalStore(subscribeSystemTheme, systemPrefersDark);
  const mode = resolveTheme(settings.theme, systemDark);
  const t = useMemo(() => createTranslator(settings.language), [settings.language]);

  useEffect(() => {
    const onStorage = () => {
      setSettings(readSettings());
    };
    window.addEventListener("storage", onStorage);
    return () => {
      window.removeEventListener("storage", onStorage);
    };
  }, []);

  // Before paint, so a theme switch never shows one frame in the old colors.
  useLayoutEffect(() => {
    setCurrentLanguage(settings.language);
    document.documentElement.lang = settings.language;
    applyDocumentTheme(mode);
    const style = document.documentElement.style;
    const variables = viewerCssVariables(viewerPalette(settings.viewerTheme, mode));
    for (const [name, value] of Object.entries(variables)) style.setProperty(name, value);
    style.setProperty("--viewer-font", fontStack(settings.terminalFont));
  }, [settings.language, settings.viewerTheme, settings.terminalFont, mode]);

  return (
    <I18nContext.Provider value={t}>
      <FileViewer language={settings.language} />
    </I18nContext.Provider>
  );
}

/** The changes against the last commit, read when the diff mode is switched on. */
type DiffLoad =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; diff: HeadDiff }
  | { kind: "failed"; message: string };

function FileViewer({ language }: { language: string }) {
  const t = useT();
  const [content, setContent] = useState<FileContent | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reloading, setReloading] = useState(false);
  const [copied, setCopied] = useState(false);
  /** The chosen mode; `null` until the user picks one (then `initialMode` decides). */
  const [chosenMode, setChosenMode] = useState<ViewerMode | null>(null);
  const [diff, setDiff] = useState<DiffLoad>({ kind: "idle" });
  const [linkError, setLinkError] = useState<string | null>(null);
  const view = useRef<FileView | null>(null);
  /** The stamp of what is shown, for the live refresh. */
  const shownStamp = useRef<string | null>(null);
  /** A read is under way: the reload button and the live refresh wait for it. */
  const busy = useRef(false);
  const mode = chosenMode ?? (content === null ? "code" : initialMode(content));
  const modeRef = useRef(mode);
  useEffect(() => {
    modeRef.current = mode;
  });

  const loadDiff = useCallback(async () => {
    // A diff on screen stays while it is read again.
    setDiff((current) => (current.kind === "ready" ? current : { kind: "loading" }));
    try {
      const next = parseHeadDiff(await api.fileViewerDiff());
      if (next === null) throw new Error(t("error.unknown"));
      setDiff({ kind: "ready", diff: next });
    } catch (failure: unknown) {
      setDiff({ kind: "failed", message: errorText(failure) });
    }
  }, [t]);

  /** The diff was asked for once, to learn whether the diff switch can work at all. */
  const diffProbed = useRef(false);
  /** The content shown, for the live refresh's pace. */
  const contentRef = useRef<FileContent | null>(null);

  const load = useCallback(async () => {
    if (busy.current) return;
    busy.current = true;
    setReloading(true);
    let text = false;
    try {
      const next = parseFileContent(await api.fileViewerContent());
      if (next === null) throw new Error(t("error.unknown"));
      setContent(next);
      setError(null);
      contentRef.current = next;
      shownStamp.current = stampKey(next);
      text = next.text !== null;
    } catch (failure: unknown) {
      setError(errorText(failure));
      shownStamp.current = MISSING_STAMP;
    } finally {
      busy.current = false;
      setReloading(false);
    }
    // The first text read also asks git once: outside a repository, or before its first
    // commit, the diff switch stays hidden.
    if (modeRef.current === "diff" || (text && !diffProbed.current)) {
      diffProbed.current = true;
      await loadDiff();
    }
  }, [t, loadDiff]);
  const loadRef = useRef(load);
  useEffect(() => {
    loadRef.current = load;
  });

  useEffect(() => {
    void Promise.resolve().then(load);
    // Once when the window opens; afterwards the reload button and the live refresh read again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Live refresh: the file is read again when it changed on disk. Stops with the window.
  useEffect(
    () =>
      pollStamps({
        read: async () => {
          const stamp = parseFileStamp(await api.fileViewerStamp());
          if (stamp === null) throw new Error(t("error.unknown"));
          return stampKey(stamp);
        },
        shown: () => shownStamp.current,
        reload: () => {
          void loadRef.current();
        },
        paused: () => document.hidden || busy.current,
        intervalMs: () => refreshInterval(contentRef.current),
      }),
    [t],
  );

  // The diff may change without the file changing (a commit, a checkout): while it is shown it
  // is read again when the window comes back to the front, not on a timer.
  useEffect(() => {
    if (mode !== "diff") return;
    const again = () => {
      if (!document.hidden) void loadDiff();
    };
    window.addEventListener("focus", again);
    document.addEventListener("visibilitychange", again);
    return () => {
      window.removeEventListener("focus", again);
      document.removeEventListener("visibilitychange", again);
    };
  }, [mode, loadDiff]);

  // The WebView's own menu offers "Reload" and "Inspect"; neither belongs here. Fields (the
  // search panel) keep their edit menu.
  useEffect(() => {
    const onContextMenu = (event: MouseEvent) => {
      if (event.target instanceof Element && event.target.closest("input, textarea")) return;
      event.preventDefault();
    };
    window.addEventListener("contextmenu", onContextMenu);
    return () => {
      window.removeEventListener("contextmenu", onContextMenu);
    };
  }, []);

  useEffect(() => {
    if (linkError === null) return;
    const timer = window.setTimeout(() => {
      setLinkError(null);
    }, LINK_ERROR_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [linkError]);

  const openLink = (url: string) => {
    setLinkError(null);
    api.fileViewerOpenLink(url).catch((failure: unknown) => {
      setLinkError(t("viewer.linkFailed", { error: errorText(failure) }));
    });
  };

  const switchMode = (next: ViewerMode) => {
    const target = mode === next ? "code" : next;
    setChosenMode(target);
    if (target === "diff") void loadDiff();
  };

  useEffect(() => {
    if (content !== null) document.title = content.name;
  }, [content]);

  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => {
      setCopied(false);
    }, COPIED_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [copied]);

  // Ctrl+F also from the header: the search lives in the code view.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!event.ctrlKey || event.shiftKey || event.altKey || event.metaKey) return;
      if (event.code !== "KeyF" || view.current === null) return;
      if (event.target instanceof Element && event.target.closest(".cm-editor")) return;
      event.preventDefault();
      view.current.find();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  const copyPath = () => {
    if (content === null) return;
    writeClipboard(content.path)
      .then(() => {
        setCopied(true);
      })
      .catch(() => undefined);
  };

  const body = viewerBody(content, error, mode);
  const modes = viewerModes(
    error === null ? content : null,
    // While the diff shows, its switch stays, so a failed read can still be left.
    mode === "diff" || (diff.kind === "ready" && diffWorks(diff.diff)),
  );
  return (
    <div className="viewer-window">
      <header className="viewer-header">
        <span className="viewer-file-icon" aria-hidden="true">
          <FileIcon size={14} />
        </span>
        {content !== null && (
          <>
            <h1 className="viewer-name" title={revealInvisible(content.path)}>
              {revealInvisible(content.name)}
            </h1>
            <span className="viewer-path" title={revealInvisible(content.path)}>
              {revealInvisible(content.relativePath)}
            </span>
            <span className="viewer-size">{formatBytes(content.size, language)}</span>
          </>
        )}
        <span className="viewer-actions">
          {modes.preview && (
            <button
              type="button"
              className="viewer-button"
              aria-label={t("viewer.preview")}
              title={t("viewer.preview")}
              aria-pressed={mode === "preview"}
              onClick={() => {
                switchMode("preview");
              }}
            >
              <EyeIcon size={15} />
            </button>
          )}
          {modes.diff && (
            <button
              type="button"
              className="viewer-button"
              aria-label={t("viewer.diff")}
              title={t("viewer.diff")}
              aria-pressed={mode === "diff"}
              onClick={() => {
                switchMode("diff");
              }}
            >
              <SourceControlIcon size={15} />
            </button>
          )}
          {body.kind === "text" && (
            <button
              type="button"
              className="viewer-button"
              aria-label={t("viewer.find")}
              title={t("viewer.find")}
              onClick={() => view.current?.find()}
            >
              <SearchIcon size={15} />
            </button>
          )}
          {content !== null && (
            <button
              type="button"
              className="viewer-button"
              aria-label={t("viewer.copyPath")}
              title={copied ? t("viewer.copied") : t("viewer.copyPath")}
              onClick={copyPath}
            >
              {copied ? <CheckIcon size={15} /> : <CopyIcon size={15} />}
            </button>
          )}
          <button
            type="button"
            className={reloading ? "viewer-button spinning" : "viewer-button"}
            aria-label={t("viewer.reload")}
            title={t("viewer.reload")}
            // Not `disabled`: a focused button that disables itself drops the keyboard focus.
            aria-disabled={reloading}
            onClick={() => {
              if (!reloading) void load();
            }}
          >
            <RestartIcon size={15} />
          </button>
        </span>
        <span className="sr-only" role="status">
          {copied ? t("viewer.copied") : ""}
        </span>
      </header>
      {(body.kind === "text" || body.kind === "markdown") && body.truncated && (
        <p className="viewer-hint">{t("viewer.truncated")}</p>
      )}
      {linkError !== null && (
        <p className="viewer-hint selectable" role="alert">
          {linkError}
        </p>
      )}
      {body.kind === "loading" && (
        <p className="viewer-message" role="status">
          {t("viewer.loading")}
        </p>
      )}
      {body.kind === "failed" && (
        <div className="viewer-message" role="alert">
          <p className="viewer-message-title">{t("viewer.failed")}</p>
          <p className="viewer-message-detail selectable">{body.message}</p>
        </div>
      )}
      {body.kind === "binary" && <p className="viewer-message">{t("viewer.binary")}</p>}
      {body.kind === "imageTooLarge" && (
        <p className="viewer-message">{t("viewer.imageTooLarge")}</p>
      )}
      {body.kind === "text" && content !== null && (
        <CodeView text={body.text} fileName={content.name} handle={view} />
      )}
      {body.kind === "markdown" && <MarkdownView text={body.text} onOpenLink={openLink} />}
      {body.kind === "image" && content !== null && (
        <div className="viewer-image">
          {/* A data address only: nothing is fetched, and an SVG drawn by <img> runs no script. */}
          <img src={body.src} alt={revealInvisible(content.name)} draggable={false} />
        </div>
      )}
      {body.kind === "diff" && <DiffView load={diff} />}
    </div>
  );
}

/** The rendered Markdown of the file; http(s) links open in the browser through the backend. */
function MarkdownView({ text, onOpenLink }: { text: string; onOpenLink: (url: string) => void }) {
  const t = useT();
  const [html, setHtml] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let live = true;
    import("./markdownPreview")
      .then(({ renderViewerMarkdown }) => {
        if (live) setHtml(renderViewerMarkdown(text));
      })
      .catch(() => {
        if (live) setFailed(true);
      });
    return () => {
      live = false;
    };
  }, [text]);

  const linkOf = (target: EventTarget | null): string | null => {
    if (!(target instanceof Element)) return null;
    return target.closest<HTMLElement>("[data-href]")?.dataset.href ?? null;
  };

  if (failed) {
    return (
      <div className="viewer-message" role="alert">
        <p className="viewer-message-title">{t("viewer.failed")}</p>
      </div>
    );
  }
  if (html === null) return <p className="viewer-message">{t("viewer.loading")}</p>;
  return (
    <div
      className="note-preview viewer-markdown selectable"
      // Safe: markdownPreview.ts renders with html: false, no <a>, no images; links carry their
      // address only in data-href, which goes through the backend's check.
      dangerouslySetInnerHTML={{ __html: html }}
      onClick={(event) => {
        const href = linkOf(event.target);
        if (href === null) return;
        event.preventDefault();
        onOpenLink(href);
      }}
      onKeyDown={(event) => {
        if (event.key !== "Enter") return;
        const href = linkOf(event.target);
        if (href === null) return;
        event.preventDefault();
        onOpenLink(href);
      }}
    />
  );
}

/** The file's changes against the last commit, as the changes panel shows a diff. */
function DiffView({ load }: { load: DiffLoad }) {
  const t = useT();
  const [shown, setShown] = useState(DIFF_PAGE_LINES);
  const lines = useMemo(
    () => (load.kind === "ready" ? (splitDiff(load.diff.diff)[0]?.lines ?? []) : []),
    [load],
  );
  if (load.kind === "idle" || load.kind === "loading") {
    return (
      <p className="viewer-message" role="status">
        {t("viewer.loading")}
      </p>
    );
  }
  if (load.kind === "failed") {
    return (
      <div className="viewer-message" role="alert">
        <p className="viewer-message-title">{t("viewer.diffFailed")}</p>
        <p className="viewer-message-detail selectable">{load.message}</p>
      </div>
    );
  }
  const note = diffNote(load.diff);
  if (note !== null) return <p className="viewer-message">{t(note)}</p>;
  if (lines.length === 0) return <p className="viewer-message">{t("changes.binary")}</p>;
  const rest = lines.length - shown;
  return (
    <>
      {load.diff.truncated && <p className="viewer-hint">{t("changes.truncated")}</p>}
      <div className="viewer-diff selectable">
        <div className="diff">
          {lines.slice(0, shown).map((line, index) => (
            // Lines have no identity of their own; the list only grows at the end.
            <div key={index} className={`diff-line ${lineKind(line)}`}>
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
      </div>
    </>
  );
}

/**
 * Wrapper around the CodeMirror view. Created once; a reload only hands it the new text, so the
 * scroll position and the search stay.
 */
function CodeView(props: {
  text: string;
  fileName: string;
  handle: React.RefObject<FileView | null>;
}) {
  const { text, fileName, handle } = props;
  const t = useT();
  const host = useRef<HTMLDivElement>(null);
  const [ready, setReady] = useState(false);
  const [failed, setFailed] = useState(false);
  const latest = useRef({ text, fileName, t });
  useEffect(() => {
    latest.current = { text, fileName, t };
  });
  /** The text the view shows. */
  const shown = useRef<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    let created: FileView | null = null;
    import("./viewerEditor")
      .then(({ createFileView }) => {
        const element = host.current;
        if (cancelled || !element) return;
        const { text: initial, fileName: name, t: translate } = latest.current;
        created = createFileView(element, {
          text: initial,
          fileName: name,
          label: translate("viewer.label", { name: revealInvisible(name) }),
          phrases: editorPhrases(translate),
        });
        shown.current = initial;
        handle.current = created;
        setReady(true);
        created.focus();
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
      handle.current = null;
      created?.destroy();
    };
    // Once per mount; new text comes in below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!ready || shown.current === text) return;
    shown.current = text;
    handle.current?.setText(text);
  }, [text, ready, handle]);

  if (failed) {
    return (
      <div className="viewer-message" role="alert">
        <p className="viewer-message-title">{t("viewer.failed")}</p>
      </div>
    );
  }
  return (
    <div className="viewer-code" ref={host}>
      {!ready && <p className="viewer-message">{t("viewer.loading")}</p>}
    </div>
  );
}

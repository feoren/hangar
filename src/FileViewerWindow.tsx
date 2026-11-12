// The file viewer window (`index.html?window=file`, opened by `open_file_viewer`): one file of a
// session's folder, read-only, with line numbers and highlighting as in a code editor. It may only
// read its own file (`file_viewer_content`), listen to events and copy text. Language, app theme,
// viewer theme and font come from the settings in localStorage, which it shares with the main
// window; the `storage` event brings changes over while it is open.

import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";

import { api, errorText, parseFileContent, type FileContent } from "./api";
import { writeClipboard } from "./clipboard";
import { editorPhrases, viewerBody } from "./fileViewer";
import { formatBytes } from "./format";
import { createTranslator, detectLanguage, I18nContext, setCurrentLanguage, useT } from "./i18n";
import { CheckIcon, CopyIcon, FileIcon, RestartIcon, SearchIcon } from "./icons";
import { revealInvisible } from "./invisible";
import { loadSettings } from "./settings";
import { fontStack } from "./terminalLook";
import { applyDocumentTheme, resolveTheme, subscribeSystemTheme, systemPrefersDark } from "./theme";
import type { FileView } from "./viewerEditor";
import { viewerCssVariables, viewerPalette } from "./viewerThemes";

/** How long the copy button shows its check mark. */
const COPIED_MS = 1500;

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

function FileViewer({ language }: { language: string }) {
  const t = useT();
  const [content, setContent] = useState<FileContent | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reloading, setReloading] = useState(false);
  const [copied, setCopied] = useState(false);
  const view = useRef<FileView | null>(null);

  const load = useCallback(async () => {
    setReloading(true);
    try {
      const next = parseFileContent(await api.fileViewerContent());
      if (next === null) throw new Error(t("error.unknown"));
      setContent(next);
      setError(null);
    } catch (failure: unknown) {
      setError(errorText(failure));
    } finally {
      setReloading(false);
    }
  }, [t]);

  useEffect(() => {
    void Promise.resolve().then(load);
    // Once when the window opens; afterwards only the reload button reads again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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

  const body = viewerBody(content, error);
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
      {body.kind === "text" && body.truncated && (
        <p className="viewer-hint">{t("viewer.truncated")}</p>
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
      {body.kind === "text" && content !== null && (
        <CodeView text={body.text} fileName={content.name} handle={view} />
      )}
    </div>
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

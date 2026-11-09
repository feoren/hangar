import { useEffect, useRef, useState } from "react";

import { useT } from "./i18n";
import type { MarkdownEditor } from "./markdownEditor";

interface Props {
  /** Changes as soon as another note (or the same one fresh from disk) has been loaded. */
  loadKey: string;
  text: string;
  label: string;
  onChange: (text: string) => void;
  onSelection: (selected: boolean) => void;
  onEscape: () => void;
  /** The editor now shows the content for `loadKey`. */
  onLoaded: (loadKey: string) => void;
  /** Receives the editor once it is ready, `null` when it is gone. */
  editorRef: React.RefObject<MarkdownEditor | null>;
}

/**
 * Wrapper around CodeMirror. The editor owns the text; React only passes it in when a note is
 * loaded, so typing does not trigger setting the content again (cursor and history stay).
 */
export function NoteEditor(props: Props) {
  const { loadKey, text, label, onChange, onSelection, onEscape, onLoaded, editorRef } = props;
  const t = useT();
  const host = useRef<HTMLDivElement>(null);
  const [failed, setFailed] = useState(false);
  const [ready, setReady] = useState(false);
  /** `loadKey` whose content is in the editor. */
  const loaded = useRef<string | null>(null);
  // Current values for the editor, which receives its callbacks only once.
  const latest = useRef({ text, label, loadKey, onChange, onSelection, onEscape, onLoaded });
  useEffect(() => {
    latest.current = { text, label, loadKey, onChange, onSelection, onEscape, onLoaded };
  });

  useEffect(() => {
    let cancelled = false;
    let editor: MarkdownEditor | null = null;
    import("./markdownEditor")
      .then(({ createMarkdownEditor }) => {
        const element = host.current;
        if (cancelled || !element) return;
        editor = createMarkdownEditor(element, {
          text: latest.current.text,
          label: latest.current.label,
          placeholder: t("notes.placeholder"),
          taskLabel: t("notes.task"),
          onChange: (next) => {
            latest.current.onChange(next);
          },
          onSelection: (selected) => {
            latest.current.onSelection(selected);
          },
          onEscape: () => {
            latest.current.onEscape();
          },
        });
        editorRef.current = editor;
        loaded.current = latest.current.loadKey;
        latest.current.onLoaded(latest.current.loadKey);
        setReady(true);
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
      editorRef.current = null;
      editor?.destroy();
    };
    // Once per mount; text and label come in via `loadKey`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // On creation the text is already in the editor; after that, set it again on every note switch.
  useEffect(() => {
    if (!ready || loaded.current === loadKey) return;
    loaded.current = loadKey;
    editorRef.current?.load(latest.current.text, latest.current.label);
    latest.current.onLoaded(loadKey);
  }, [loadKey, ready, editorRef]);

  if (failed) return <p className="notes-message error-text">{t("notes.editorFailed")}</p>;
  return (
    <div className="note-editor" ref={host}>
      {!ready && <p className="notes-message muted">{t("notes.loading")}</p>}
    </div>
  );
}

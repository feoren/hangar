import { useState } from "react";

import { Dialog } from "./Dialog";
import { useT } from "./i18n";
import { CloseIcon } from "./icons";
import { MAX_QUEUED_CHARS } from "./messageQueue";
import { editorKeyAction, editorPosition } from "./queueEditor";

/** Size the editor is placed with (styles.css `.dialog.queue-editor`); the height is generous. */
const EDITOR_SIZE = { width: 380, height: 270 };

interface Props {
  /** Name of the session, under the heading. */
  title: string;
  /** The queued text when editing; empty for a new message. */
  initial: string;
  /** Where the session menu was; `null` (from quick access): centered. */
  anchor: { x: number; y: number } | null;
  onSave: (text: string) => void;
  /** Only while a message is queued. */
  onDiscard: (() => void) | null;
  /** Escape, a click outside or Cancel. The caller returns the focus to the terminal. */
  onClose: () => void;
}

/**
 * The small editor of a queued message, opened next to the session's row from its menu: one field,
 * Enter queues, Shift+Enter makes a new line, Escape cancels. One message per session; saving
 * replaces the one queued before.
 */
export function QueueDialog({ title, initial, anchor, onSave, onDiscard, onClose }: Props) {
  const t = useT();
  const [text, setText] = useState(initial);
  const ready = text.trim() !== "";
  const place = editorPosition(anchor, EDITOR_SIZE, {
    width: window.innerWidth,
    height: window.innerHeight,
  });

  const save = () => {
    if (ready) onSave(text);
  };

  return (
    <Dialog
      labelledBy="queue-title"
      className={place === null ? "queue-editor" : "queue-editor anchored"}
      style={
        place === null
          ? undefined
          : { left: `${String(place.left)}px`, top: `${String(place.top)}px` }
      }
      onClose={onClose}
    >
      <div className="dialog-header">
        <h2 id="queue-title">
          {t("queue.title")}
          <span className="marker-dialog-session">{title}</span>
        </h2>
        <button
          type="button"
          className="icon-button"
          aria-label={t("common.close")}
          title={t("common.close")}
          onClick={onClose}
        >
          <CloseIcon />
        </button>
      </div>
      <textarea
        className="text-input queue-text"
        aria-label={t("queue.label")}
        aria-describedby="queue-help queue-keys"
        placeholder={t("queue.placeholder")}
        rows={3}
        maxLength={MAX_QUEUED_CHARS}
        value={text}
        spellCheck={false}
        data-autofocus
        onFocus={(event) => {
          // Editing goes on where the text ends, not before its first letter.
          const end = event.currentTarget.value.length;
          event.currentTarget.setSelectionRange(end, end);
        }}
        onChange={(event) => {
          setText(event.currentTarget.value);
        }}
        onKeyDown={(event) => {
          const action = editorKeyAction({
            key: event.key,
            shiftKey: event.shiftKey,
            ctrlKey: event.ctrlKey,
            altKey: event.altKey,
            metaKey: event.metaKey,
            isComposing: event.nativeEvent.isComposing,
          });
          if (action !== "save") return;
          event.preventDefault();
          save();
        }}
      />
      <p id="queue-help" className="muted">
        {t("queue.help")}
      </p>
      <p id="queue-keys" className="muted">
        {t("queue.keys")}
      </p>
      <div className="dialog-actions">
        {onDiscard !== null && (
          <button type="button" className="secondary-button queue-discard" onClick={onDiscard}>
            {t("queue.discardShort")}
          </button>
        )}
        <button type="button" className="secondary-button" onClick={onClose}>
          {t("common.cancel")}
        </button>
        <button type="button" className="primary-button" disabled={!ready} onClick={save}>
          {t("queue.save")}
        </button>
      </div>
    </Dialog>
  );
}

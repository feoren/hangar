import { useState } from "react";

import { Dialog } from "./Dialog";
import { useT } from "./i18n";
import { CloseIcon } from "./icons";
import {
  MARKER_COLOR_NAMES,
  MARKER_COLORS,
  MARKER_EMOJIS,
  NO_MARKER,
  parseEmoji,
  type Marker,
} from "./marker";

interface Props {
  title: string;
  marker: Marker | undefined;
  /** Every change applies at once, so the sidebar shows it while the dialog is open. */
  onChange: (marker: Marker) => void;
  onClose: () => void;
}

/**
 * Color and emoji of a session. A small dialog with a row of colors (none plus eight), a few
 * emoji to pick and a field for any other emoji. Changes apply immediately.
 */
export function MarkerDialog({ title, marker, onChange, onClose }: Props) {
  const t = useT();
  const current = marker ?? NO_MARKER;
  const [emojiText, setEmojiText] = useState(current.emoji ?? "");
  const emojiValid = parseEmoji(emojiText) !== undefined;

  const setColor = (color: string | null) => {
    onChange({ ...current, color });
  };
  const setEmoji = (text: string) => {
    setEmojiText(text);
    const emoji = parseEmoji(text);
    if (emoji !== undefined) onChange({ ...current, emoji });
  };

  return (
    <Dialog labelledBy="marker-title" className="marker-dialog" onClose={onClose}>
      <div className="dialog-header">
        <h2 id="marker-title">
          {t("marker.title")}
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

      <fieldset className="settings-group marker-group">
        <legend>{t("marker.color")}</legend>
        <div className="swatches marker-swatches">
          <label className="swatch marker-none" title={t("marker.none")}>
            <input
              type="radio"
              name="marker-color"
              aria-label={t("marker.none")}
              checked={current.color === null}
              data-autofocus={current.color === null ? true : undefined}
              onChange={() => {
                setColor(null);
              }}
            />
            <span className="swatch-color" aria-hidden="true" />
          </label>
          {MARKER_COLORS.map((entry) => (
            <label key={entry.id} className="swatch" title={t(MARKER_COLOR_NAMES[entry.id])}>
              <input
                type="radio"
                name="marker-color"
                aria-label={t(MARKER_COLOR_NAMES[entry.id])}
                checked={current.color === entry.color}
                data-autofocus={current.color === entry.color ? true : undefined}
                onChange={() => {
                  setColor(entry.color);
                }}
              />
              <span className="swatch-color" style={{ background: entry.color }} />
            </label>
          ))}
        </div>
      </fieldset>

      <fieldset className="settings-group marker-group">
        <legend id="marker-emoji-legend">{t("marker.emoji")}</legend>
        <div className="marker-emoji-row">
          <input
            aria-labelledby="marker-emoji-legend"
            className="text-input marker-emoji-input"
            value={emojiText}
            placeholder={t("marker.emojiPlaceholder")}
            aria-invalid={!emojiValid}
            aria-describedby={emojiValid ? undefined : "marker-emoji-error"}
            autoComplete="off"
            spellCheck={false}
            onChange={(event) => {
              setEmoji(event.currentTarget.value);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                onClose();
              }
            }}
          />
          <div className="marker-emoji-picks" role="group" aria-label={t("marker.emoji")}>
            {MARKER_EMOJIS.map((emoji) => (
              <button
                key={emoji}
                type="button"
                className={current.emoji === emoji ? "marker-emoji-pick on" : "marker-emoji-pick"}
                aria-pressed={current.emoji === emoji}
                onClick={() => {
                  setEmoji(current.emoji === emoji ? "" : emoji);
                }}
              >
                {emoji}
              </button>
            ))}
          </div>
        </div>
        {!emojiValid && (
          <p id="marker-emoji-error" className="muted error-text" role="alert">
            {t("marker.emojiInvalid")}
          </p>
        )}
      </fieldset>

      <div className="dialog-actions">
        <button
          type="button"
          className="secondary-button"
          disabled={current.color === null && current.emoji === null}
          onClick={() => {
            setEmojiText("");
            onChange(NO_MARKER);
          }}
        >
          {t("marker.clear")}
        </button>
        <button type="button" className="primary-button" onClick={onClose}>
          {t("marker.done")}
        </button>
      </div>
    </Dialog>
  );
}

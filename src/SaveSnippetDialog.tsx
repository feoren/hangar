import { useId, useState } from "react";

import { Dialog } from "./Dialog";
import { useT } from "./i18n";
import { CloseIcon } from "./icons";
import {
  SNIPPET_LIMIT,
  SNIPPET_MAX_LENGTH,
  snippetLabel,
  snippetTextFromSelection,
  snippetWithText,
  type Snippet,
} from "./snippets";

interface Props {
  /** The selection, already cleaned (snippetTextFromSelection). */
  text: string;
  snippets: readonly Snippet[];
  /** Gets the cleaned text; the caller stores it and closes. */
  onSave: (text: string) => void;
  /** Settings at the snippets, where an existing one can be found and edited. */
  onShowSnippets: () => void;
  onClose: () => void;
}

/**
 * "Save as snippet" from a terminal selection: the text to check and edit, the name it will carry
 * in menus (its first line, as for every snippet) and a hint instead of a second copy when the
 * same text is stored already.
 */
export function SaveSnippetDialog({ text, snippets, onSave, onShowSnippets, onClose }: Props) {
  const t = useT();
  const id = useId();
  const [draft, setDraft] = useState(text);
  const cleaned = snippetTextFromSelection(draft);
  const existing = snippetWithText(snippets, cleaned);
  const full = snippets.length >= SNIPPET_LIMIT;
  const ready = cleaned !== "" && existing === undefined && !full;
  const name = snippetLabel({ text: cleaned });
  const hintId = `${id}-hint`;
  const hint =
    existing !== undefined
      ? t("snippets.exists", { name: snippetLabel(existing) })
      : full
        ? t("snippets.full", { count: SNIPPET_LIMIT })
        : null;

  return (
    <Dialog labelledBy={`${id}-title`} className="template-dialog" onClose={onClose}>
      <div className="dialog-header">
        <h2 id={`${id}-title`}>{t("snippets.saveSelection")}</h2>
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
      <p className="muted">{t("snippets.saveHelp")}</p>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (ready) onSave(cleaned);
        }}
      >
        <label className="template-name">
          <span>{t("snippets.textLabel")}</span>
          <textarea
            className="text-input snippet-text snippet-save-text"
            rows={6}
            maxLength={SNIPPET_MAX_LENGTH}
            value={draft}
            spellCheck={false}
            aria-describedby={hint === null ? undefined : hintId}
            data-autofocus
            onChange={(event) => {
              setDraft(event.currentTarget.value);
            }}
          />
        </label>
        {name !== "" && <p className="muted snippet-save-name">{t("snippets.nameIn", { name })}</p>}
        {hint !== null && (
          <p id={hintId} className="hint" role="status">
            {hint}
          </p>
        )}
        <div className="dialog-actions">
          {hint !== null && (
            <button type="button" className="secondary-button" onClick={onShowSnippets}>
              {t("snippets.show")}
            </button>
          )}
          <button type="button" className="secondary-button" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary-button" disabled={!ready}>
            {t("snippets.saveButton")}
          </button>
        </div>
      </form>
    </Dialog>
  );
}

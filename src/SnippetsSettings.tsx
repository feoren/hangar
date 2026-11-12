import { Fragment, useEffect, useRef } from "react";

import { useT, type MessageKey } from "./i18n";
import { SNIPPET_VARIABLES, usedVariables, type SnippetVariable } from "./snippetVariables";
import { ArrowDownIcon, ArrowUpIcon, PlusIcon, TrashIcon } from "./icons";
import { formatShortcut, SHORTCUT_LABELS } from "./shortcuts";
import {
  moveSnippet,
  newSnippetId,
  SNIPPET_LIMIT,
  SNIPPET_MAX_LENGTH,
  type Snippet,
} from "./snippets";

const VARIABLE_HELP: Record<SnippetVariable, MessageKey> = {
  file: "snippets.varFile",
  branch: "snippets.varBranch",
  selection: "snippets.varSelection",
  diff: "snippets.varDiff",
  folder: "snippets.varFolder",
};

interface Props {
  snippets: Snippet[];
  onChange: (snippets: Snippet[]) => void;
}

/**
 * Settings, Snippets: one text field per snippet, saved while typing; move up, move down and
 * delete as small icon buttons. Adding puts the cursor into the new field.
 */
export function SnippetsSettings({ snippets, onChange }: Props) {
  const t = useT();
  const fields = useRef(new Map<string, HTMLTextAreaElement>());
  /** Field that gets the focus after the next render (added, deleted, moved to an edge). */
  const pendingFocus = useRef<string | null>(null);
  const setFocusId = (id: string) => {
    pendingFocus.current = id;
  };

  useEffect(() => {
    const id = pendingFocus.current;
    if (id === null) return;
    pendingFocus.current = null;
    fields.current.get(id)?.focus();
  });

  const update = (id: string, text: string) => {
    onChange(snippets.map((snippet) => (snippet.id === id ? { ...snippet, text } : snippet)));
  };

  const add = () => {
    const id = newSnippetId(snippets);
    onChange([...snippets, { id, text: "" }]);
    setFocusId(id);
  };

  const remove = (index: number) => {
    const next = snippets.filter((_, position) => position !== index);
    onChange(next);
    // Focus to the neighbour, so the keyboard does not fall out of the dialog.
    const neighbour = next[index] ?? next[index - 1];
    if (neighbour) setFocusId(neighbour.id);
  };

  return (
    <section className="settings-section" aria-labelledby="snippets-help">
      <p id="snippets-help" className="muted settings-intro">
        {t("snippets.help", { keys: formatShortcut(SHORTCUT_LABELS.launcher, t) })}
      </p>
      <div className="snippet-variables">
        <p className="muted">{t("snippets.variables")}</p>
        <dl>
          {SNIPPET_VARIABLES.map((name) => (
            <Fragment key={name}>
              <dt>
                <code>{`{{${name}}}`}</code>
              </dt>
              <dd className="muted">{t(VARIABLE_HELP[name])}</dd>
            </Fragment>
          ))}
        </dl>
      </div>
      {snippets.length === 0 && <p className="muted">{t("snippets.empty")}</p>}
      <ol className="snippet-list">
        {snippets.map((snippet, index) => {
          const label = t("snippets.text", { number: index + 1 });
          // Not an error: unknown names are sent as written, the hint only catches typos.
          const unknown = usedVariables(snippet.text).unknown;
          const hintId = `snippet-unknown-${snippet.id}`;
          return (
            <li key={snippet.id} className="snippet-row">
              <div className="snippet-field">
                <textarea
                  ref={(element) => {
                    if (element) fields.current.set(snippet.id, element);
                    else fields.current.delete(snippet.id);
                  }}
                  className="text-input snippet-text"
                  aria-label={label}
                  aria-describedby={unknown.length > 0 ? hintId : undefined}
                  rows={2}
                  maxLength={SNIPPET_MAX_LENGTH}
                  value={snippet.text}
                  spellCheck={false}
                  onChange={(event) => {
                    update(snippet.id, event.currentTarget.value);
                  }}
                />
                {unknown.length > 0 && (
                  <p id={hintId} className="muted snippet-unknown">
                    {t("snippets.unknownHint", {
                      names: unknown.map((name) => `{{${name}}}`).join(", "),
                    })}
                  </p>
                )}
              </div>
              <div className="snippet-actions">
                <button
                  type="button"
                  className="icon-button"
                  aria-label={`${t("snippets.moveUp")}: ${label}`}
                  title={t("snippets.moveUp")}
                  disabled={index === 0}
                  onClick={() => {
                    onChange(moveSnippet(snippets, snippet.id, -1));
                    // At the top the button turns off; the keyboard moves to the text instead.
                    if (index === 1) setFocusId(snippet.id);
                  }}
                >
                  <ArrowUpIcon />
                </button>
                <button
                  type="button"
                  className="icon-button"
                  aria-label={`${t("snippets.moveDown")}: ${label}`}
                  title={t("snippets.moveDown")}
                  disabled={index === snippets.length - 1}
                  onClick={() => {
                    onChange(moveSnippet(snippets, snippet.id, 1));
                    if (index === snippets.length - 2) setFocusId(snippet.id);
                  }}
                >
                  <ArrowDownIcon />
                </button>
                <button
                  type="button"
                  className="icon-button"
                  aria-label={`${t("snippets.delete")}: ${label}`}
                  title={t("snippets.delete")}
                  onClick={() => {
                    remove(index);
                  }}
                >
                  <TrashIcon />
                </button>
              </div>
            </li>
          );
        })}
      </ol>
      <div className="settings-row">
        <button
          type="button"
          className="secondary-button with-icon"
          disabled={snippets.length >= SNIPPET_LIMIT}
          onClick={add}
        >
          <PlusIcon />
          {t("snippets.add")}
        </button>
      </div>
    </section>
  );
}

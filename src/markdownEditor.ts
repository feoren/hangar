// Markdown editor for notes (CodeMirror 6). Loaded only when the notes panel opens
// (dynamic import in NoteEditor.tsx), so Hangar's startup does not wait for it.
//
// Deliberately without Tab indentation: Tab leaves the editor like any other input field, so
// you can get in and out again by keyboard.

import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap, placeholder } from "@codemirror/view";
import { tags } from "@lezer/highlight";

export interface MarkdownEditor {
  /** New content with an empty undo history (another note was loaded). */
  load: (text: string, label: string) => void;
  focus: () => void;
  destroy: () => void;
}

export interface EditorOptions {
  text: string;
  label: string;
  placeholder: string;
  onChange: (text: string) => void;
  /** Escape in the editor: back to the terminal. */
  onEscape: () => void;
}

// Colors only via tokens from styles.css, so light and dark switch without a new editor.
const highlight = HighlightStyle.define([
  { tag: tags.heading1, fontWeight: "700", fontSize: "1.25em", color: "var(--md-heading)" },
  { tag: tags.heading2, fontWeight: "700", fontSize: "1.12em", color: "var(--md-heading)" },
  {
    tag: [tags.heading3, tags.heading4, tags.heading5, tags.heading6],
    fontWeight: "700",
    color: "var(--md-heading)",
  },
  { tag: tags.strong, fontWeight: "700" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.strikethrough, textDecoration: "line-through" },
  { tag: [tags.link, tags.url], color: "var(--accent)" },
  { tag: tags.monospace, color: "var(--md-code)", fontFamily: "var(--mono)" },
  { tag: tags.quote, color: "var(--muted)", fontStyle: "italic" },
  { tag: [tags.processingInstruction, tags.contentSeparator, tags.meta], color: "var(--muted)" },
]);

const theme = EditorView.theme({
  "&": { height: "100%", color: "var(--text)", backgroundColor: "transparent" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "inherit", lineHeight: "1.55", overflow: "auto" },
  ".cm-content": { padding: "10px 14px 40px", caretColor: "var(--text)" },
  ".cm-line": { padding: "0" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--text)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
    backgroundColor: "var(--md-selection)",
  },
  ".cm-placeholder": { color: "var(--muted)" },
});

export function createMarkdownEditor(parent: HTMLElement, options: EditorOptions): MarkdownEditor {
  const state = (text: string, label: string) =>
    EditorState.create({
      doc: text,
      extensions: [
        history(),
        keymap.of([
          {
            key: "Escape",
            run: () => {
              options.onEscape();
              return true;
            },
          },
          ...defaultKeymap,
          ...historyKeymap,
        ]),
        markdown({ base: markdownLanguage }),
        syntaxHighlighting(highlight),
        EditorView.lineWrapping,
        placeholder(options.placeholder),
        EditorView.contentAttributes.of({ "aria-label": label, spellcheck: "true" }),
        theme,
        EditorView.updateListener.of((update) => {
          if (update.docChanged) options.onChange(update.state.doc.toString());
        }),
      ],
    });

  const view = new EditorView({ state: state(options.text, options.label), parent });
  return {
    load: (text, label) => {
      view.setState(state(text, label));
    },
    focus: () => {
      view.focus();
    },
    destroy: () => {
      view.destroy();
    },
  };
}

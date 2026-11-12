// Read-only code view of the file viewer window (CodeMirror 6). Loaded only in that window
// (dynamic import in FileViewerWindow.tsx). Colors come from CSS variables (viewerThemes.ts), so a
// new theme or a switch between light and dark only sets other variables on the window.
//
// The content is read-only but still focusable: arrows, selection, Ctrl+A, copying and the search
// (Ctrl+F) work as in an editor. The language comes from the file name; its parser loads lazily.

import { defaultKeymap } from "@codemirror/commands";
import { languages } from "@codemirror/language-data";
import {
  bracketMatching,
  codeFolding,
  foldGutter,
  foldKeymap,
  HighlightStyle,
  LanguageDescription,
  syntaxHighlighting,
} from "@codemirror/language";
import {
  highlightSelectionMatches,
  openSearchPanel,
  search,
  searchKeymap,
} from "@codemirror/search";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import {
  drawSelection,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  keymap,
  lineNumbers,
} from "@codemirror/view";
import { tags, type Tag } from "@lezer/highlight";

import { MORE_SPECIAL_CHARS } from "./invisible";
import type { TokenKind } from "./viewerThemes";

export interface FileView {
  /** New content (reload); the scroll position and the cursor stay where they can. */
  setText: (text: string) => void;
  focus: () => void;
  /** Opens the search field (as Ctrl+F does). */
  find: () => void;
  destroy: () => void;
}

export interface FileViewOptions {
  text: string;
  /** The file name, for the language. */
  fileName: string;
  /** Accessible name of the content. */
  label: string;
  /** CodeMirror's own texts (search panel, folding), translated (`EditorState.phrases`). */
  phrases: Record<string, string>;
}

const TOKEN_TAGS: Record<TokenKind, readonly Tag[]> = {
  comment: [tags.comment, tags.lineComment, tags.blockComment, tags.docComment],
  keyword: [tags.keyword, tags.definitionKeyword, tags.modifier, tags.operatorKeyword],
  control: [tags.controlKeyword, tags.moduleKeyword],
  string: [tags.string, tags.special(tags.string), tags.character, tags.docString],
  number: [tags.number, tags.integer, tags.float],
  constant: [tags.bool, tags.null, tags.atom, tags.constant(tags.name), tags.unit],
  function: [tags.function(tags.variableName), tags.function(tags.propertyName), tags.macroName],
  variable: [tags.variableName, tags.definition(tags.variableName), tags.labelName],
  property: [tags.propertyName, tags.definition(tags.propertyName)],
  type: [tags.typeName, tags.className, tags.namespace, tags.standard(tags.typeName)],
  operator: [tags.operator, tags.derefOperator, tags.escape],
  tag: [tags.tagName, tags.angleBracket],
  attribute: [tags.attributeName, tags.attributeValue],
  regexp: [tags.regexp],
  heading: [tags.heading],
  link: [tags.link, tags.url],
  meta: [tags.meta, tags.annotation, tags.processingInstruction, tags.self],
  invalid: [tags.invalid],
};

const highlight = HighlightStyle.define([
  ...Object.entries(TOKEN_TAGS).map(([kind, tag]) => ({
    tag: [...tag],
    color: `var(--viewer-tok-${kind})`,
  })),
  { tag: [tags.comment, tags.lineComment, tags.blockComment], fontStyle: "italic" },
  { tag: tags.heading, fontWeight: "700" },
  { tag: tags.strong, fontWeight: "700" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.strikethrough, textDecoration: "line-through" },
  { tag: tags.link, textDecoration: "underline" },
]);

const theme = EditorView.theme({
  "&": {
    height: "100%",
    color: "var(--viewer-fg)",
    backgroundColor: "var(--viewer-bg)",
    fontSize: "13px",
  },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": {
    fontFamily: "var(--viewer-font)",
    lineHeight: "1.55",
    overflow: "auto",
  },
  ".cm-content": { padding: "6px 0 40px", caretColor: "var(--viewer-cursor)" },
  ".cm-line": { padding: "0 16px 0 6px" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--viewer-cursor)", borderLeftWidth: "2px" },
  ".cm-selectionBackground": { backgroundColor: "var(--viewer-selection)" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground": {
    backgroundColor: "var(--viewer-selection)",
  },
  ".cm-content ::selection": { backgroundColor: "transparent" },
  ".cm-activeLine": { backgroundColor: "var(--viewer-active-line)" },
  ".cm-gutters": {
    color: "var(--viewer-gutter-fg)",
    backgroundColor: "var(--viewer-gutter-bg)",
    border: "none",
  },
  ".cm-lineNumbers .cm-gutterElement": { padding: "0 12px 0 18px", minWidth: "32px" },
  ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--viewer-gutter-active)" },
  ".cm-foldGutter .cm-gutterElement": {
    width: "14px",
    paddingRight: "2px",
    cursor: "pointer",
    opacity: "0.75",
  },
  ".cm-foldPlaceholder": {
    margin: "0 2px",
    padding: "0 4px",
    color: "var(--viewer-gutter-active)",
    backgroundColor: "var(--viewer-match)",
    border: "1px solid var(--viewer-accent)",
    borderRadius: "3px",
  },
  ".cm-searchMatch": { backgroundColor: "var(--viewer-match)", borderRadius: "2px" },
  ".cm-searchMatch.cm-searchMatch-selected": {
    outline: "1px solid var(--viewer-accent)",
  },
  ".cm-selectionMatch": { backgroundColor: "var(--viewer-match)" },
  "&.cm-focused .cm-matchingBracket": {
    backgroundColor: "transparent",
    outline: "1px solid var(--viewer-gutter-fg)",
  },
  ".cm-specialChar": { color: "var(--viewer-tok-invalid)" },
  ".cm-panels": {
    color: "var(--viewer-fg)",
    backgroundColor: "var(--viewer-gutter-bg)",
  },
  ".cm-panels.cm-panels-top": { borderBottom: "1px solid var(--viewer-border)" },
  ".cm-panel.cm-search": { padding: "6px 34px 6px 10px", fontFamily: "inherit", fontSize: "12px" },
  ".cm-panel.cm-search input, .cm-panel.cm-search button": {
    fontFamily: "inherit",
    fontSize: "12px",
  },
  ".cm-textfield": {
    padding: "3px 6px",
    color: "var(--viewer-fg)",
    backgroundColor: "var(--viewer-bg)",
    border: "1px solid var(--viewer-border)",
    borderRadius: "4px",
  },
  ".cm-textfield:focus": { outline: "1px solid var(--viewer-accent)", outlineOffset: "-1px" },
  ".cm-button": {
    padding: "3px 8px",
    color: "var(--viewer-fg)",
    backgroundImage: "none",
    backgroundColor: "var(--viewer-bg)",
    border: "1px solid var(--viewer-border)",
    borderRadius: "4px",
  },
  ".cm-button:hover": { backgroundColor: "var(--viewer-active-line)" },
  ".cm-panel.cm-search label": { fontSize: "12px" },
  ".cm-panel.cm-search [name=close]": {
    top: "6px",
    right: "8px",
    fontSize: "16px",
    color: "var(--viewer-muted)",
    cursor: "pointer",
  },
});

/** The language of a file by its name (and extension); `null`: shown as plain text. */
function languageOf(fileName: string): LanguageDescription | null {
  return LanguageDescription.matchFilename(languages, fileName);
}

export function createFileView(parent: HTMLElement, options: FileViewOptions): FileView {
  const language = new Compartment();
  const extensions: Extension[] = [
    EditorState.readOnly.of(true),
    EditorState.phrases.of(options.phrases),
    lineNumbers(),
    highlightActiveLineGutter(),
    foldGutter(),
    codeFolding(),
    drawSelection(),
    highlightActiveLine(),
    highlightSpecialChars({ addSpecialChars: MORE_SPECIAL_CHARS }),
    highlightSelectionMatches(),
    bracketMatching(),
    search({ top: true }),
    keymap.of([...searchKeymap, ...foldKeymap, ...defaultKeymap]),
    syntaxHighlighting(highlight),
    language.of([]),
    EditorView.contentAttributes.of({ "aria-label": options.label, spellcheck: "false" }),
    theme,
  ];
  const view = new EditorView({
    state: EditorState.create({ doc: options.text, extensions }),
    parent,
  });

  let destroyed = false;
  const description = languageOf(options.fileName);
  if (description !== null) {
    description
      .load()
      .then((support) => {
        if (!destroyed) view.dispatch({ effects: language.reconfigure(support) });
      })
      // Without its parser the file stays plain text.
      .catch(() => undefined);
  }

  return {
    setText: (text) => {
      const top = view.scrollDOM.scrollTop;
      const left = view.scrollDOM.scrollLeft;
      const head = Math.min(view.state.selection.main.head, text.length);
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: text },
        selection: { anchor: head },
      });
      // After the new lines are measured, back to where the reader was.
      requestAnimationFrame(() => {
        if (destroyed) return;
        view.scrollDOM.scrollTop = top;
        view.scrollDOM.scrollLeft = left;
      });
    },
    focus: () => {
      view.focus();
    },
    find: () => {
      view.focus();
      openSearchPanel(view);
    },
    destroy: () => {
      destroyed = true;
      view.destroy();
    },
  };
}

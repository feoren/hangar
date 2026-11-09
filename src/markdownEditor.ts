// Markdown editor for notes (CodeMirror 6). Loaded only when the notes panel opens
// (dynamic import in NoteEditor.tsx), so Hangar's startup does not wait for it.
//
// Tab indents only list items (Tab and Shift+Tab, as in other editors); anywhere else Tab leaves
// the editor like any other input field, so you can get in and out again by keyboard.
// Task boxes (`- [ ]`) show as checkboxes you can click; the text stays plain Markdown.

import {
  defaultKeymap,
  history,
  historyKeymap,
  indentLess,
  indentMore,
} from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { HighlightStyle, syntaxHighlighting, syntaxTree } from "@codemirror/language";
import { EditorSelection, EditorState, type Range } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  highlightSpecialChars,
  keymap,
  placeholder,
  ViewPlugin,
  WidgetType,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";
import { tags } from "@lezer/highlight";

import { isListLine, taskMarkOffset } from "./noteText";

export interface MarkdownEditor {
  /** New content with an empty undo history (another note was loaded); cursor at the end. */
  load: (text: string, label: string) => void;
  /** Focus with the cursor where it is, scrolled into view. */
  focus: () => void;
  /** Selected text, empty without a selection. */
  selection: () => string;
  /** Replaces the content as one undoable step (output added at the end), cursor at the end. */
  setText: (next: string) => void;
  /** Ticks or unticks the task on line `index` (0-based); from the preview. */
  toggleTask: (index: number) => void;
  destroy: () => void;
}

export interface EditorOptions {
  text: string;
  label: string;
  placeholder: string;
  /** Accessible name of a task checkbox. */
  taskLabel: string;
  onChange: (text: string) => void;
  /** Whether text is selected; called only when that changes. */
  onSelection: (selected: boolean) => void;
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

/**
 * Invisible characters the editor draws as a marker, so a note cannot read one way and run
 * another once sent to a session. CodeMirror's default set (@codemirror/view 6.43) already has
 * C0 and C1 controls, U+200B, U+200E, U+200F, U+202D, U+202E, U+2066, U+2067, U+2069 and the
 * byte order mark; this adds the other bidi embeddings and isolates (U+202A to U+202C, U+2068)
 * and the word joiner and invisible operators (U+2060 to U+2064), the rest of src/invisible.ts.
 * U+200C and U+200D stay unmarked: they join emoji and letters in Persian or Indic scripts, and
 * a marker would break that text. The send warning (appPaste.ts) still names them.
 */
const MORE_SPECIAL_CHARS = /[\u202a-\u202c\u2068\u2060-\u2064]/;

class TaskWidget extends WidgetType {
  constructor(
    readonly checked: boolean,
    readonly label: string,
  ) {
    super();
  }

  override eq(other: TaskWidget): boolean {
    return other.checked === this.checked && other.label === this.label;
  }

  toDOM(): HTMLElement {
    const box = document.createElement("input");
    box.type = "checkbox";
    box.className = "cm-task";
    box.checked = this.checked;
    box.setAttribute("aria-label", this.label);
    // The text stays the single source: the box is drawn, the click changes "[ ]" in the text.
    box.tabIndex = -1;
    return box;
  }

  override ignoreEvent(): boolean {
    return false;
  }
}

function taskDecorations(view: EditorView, label: string): DecorationSet {
  const ranges: Range<Decoration>[] = [];
  for (const { from, to } of view.visibleRanges) {
    syntaxTree(view.state).iterate({
      from,
      to,
      enter: (node) => {
        if (node.name !== "TaskMarker") return;
        const mark = view.state.sliceDoc(node.from, node.to);
        const checked = /\[[xX]\]/.test(mark);
        // A bullet before the box goes too ("- [ ]" reads as one checkbox); numbers stay.
        const line = view.state.doc.lineAt(node.from);
        const bullet = /^(\s*)[-*+]\s+\[/.exec(line.text);
        const start =
          bullet?.[1] !== undefined && line.from + bullet[0].length - 1 === node.from
            ? line.from + bullet[1].length
            : node.from;
        ranges.push(
          Decoration.replace({ widget: new TaskWidget(checked, label) }).range(start, node.to),
        );
      },
    });
  }
  return Decoration.set(ranges, true);
}

/** Flips "[ ]" and "[x]" at `from` (start of a task marker). */
function flipTask(view: EditorView, from: number): boolean {
  const mark = view.state.sliceDoc(from, from + 3);
  if (!/^\[[ xX]\]$/.test(mark)) return false;
  view.dispatch({
    changes: { from: from + 1, to: from + 2, insert: mark[1] === " " ? "x" : " " },
    userEvent: "input.toggle",
  });
  return true;
}

function taskPlugin(label: string) {
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = taskDecorations(view, label);
      }
      update(update: ViewUpdate) {
        if (
          update.docChanged ||
          update.viewportChanged ||
          syntaxTree(update.startState) !== syntaxTree(update.state)
        ) {
          this.decorations = taskDecorations(update.view, label);
        }
      }
    },
    {
      decorations: (plugin) => plugin.decorations,
      eventHandlers: {
        mousedown: (event, view) => {
          const target = event.target;
          if (!(target instanceof HTMLElement) || !target.classList.contains("cm-task")) {
            return false;
          }
          event.preventDefault();
          // The widget may start at the bullet; the box itself is the "[" on that line.
          const line = view.state.doc.lineAt(view.posAtDOM(target));
          const offset = taskMarkOffset(line.text);
          return offset !== null && flipTask(view, line.from + offset - 1);
        },
      },
    },
  );
}

/** Tab and Shift+Tab indent list items; elsewhere `false`, so Tab moves the focus on. */
function indentList(more: boolean) {
  return (view: EditorView): boolean => {
    const { state } = view;
    const lines = state.selection.ranges.flatMap((range) => {
      const first = state.doc.lineAt(range.from).number;
      const last = state.doc.lineAt(range.to).number;
      return Array.from({ length: last - first + 1 }, (_, i) => state.doc.line(first + i).text);
    });
    if (!lines.every(isListLine)) return false;
    return more ? indentMore(view) : indentLess(view);
  };
}

export function createMarkdownEditor(parent: HTMLElement, options: EditorOptions): MarkdownEditor {
  let selected = false;
  const state = (text: string, label: string) =>
    EditorState.create({
      doc: text,
      selection: EditorSelection.cursor(text.length),
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
          { key: "Tab", run: indentList(true), shift: indentList(false) },
          ...defaultKeymap,
          ...historyKeymap,
        ]),
        markdown({ base: markdownLanguage }),
        syntaxHighlighting(highlight),
        highlightSpecialChars({ addSpecialChars: MORE_SPECIAL_CHARS }),
        taskPlugin(options.taskLabel),
        EditorView.lineWrapping,
        placeholder(options.placeholder),
        EditorView.contentAttributes.of({ "aria-label": label, spellcheck: "true" }),
        theme,
        EditorView.updateListener.of((update) => {
          if (update.docChanged) options.onChange(update.state.doc.toString());
          if (update.selectionSet || update.docChanged) {
            const now = !update.state.selection.main.empty;
            if (now !== selected) {
              selected = now;
              options.onSelection(now);
            }
          }
        }),
      ],
    });

  const view = new EditorView({ state: state(options.text, options.label), parent });
  const reveal = () => {
    view.dispatch({ effects: EditorView.scrollIntoView(view.state.selection.main.head) });
  };
  return {
    load: (text, label) => {
      view.setState(state(text, label));
      if (selected) {
        selected = false;
        options.onSelection(false);
      }
    },
    focus: () => {
      view.focus();
      reveal();
    },
    selection: () => {
      const { from, to } = view.state.selection.main;
      return view.state.sliceDoc(from, to);
    },
    setText: (next) => {
      const end = view.state.doc.length;
      view.dispatch({
        changes: { from: 0, to: end, insert: next },
        selection: EditorSelection.cursor(next.length),
        scrollIntoView: true,
        userEvent: "input.capture",
      });
    },
    toggleTask: (index) => {
      const number = index + 1;
      if (number < 1 || number > view.state.doc.lines) return;
      const line = view.state.doc.line(number);
      const offset = taskMarkOffset(line.text);
      if (offset !== null) flipTask(view, line.from + offset - 1);
    },
    destroy: () => {
      view.destroy();
    },
  };
}

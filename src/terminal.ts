// One xterm.js instance per session. Lives outside React so scrollback and state survive
// switching in the sidebar; React only attaches it to the DOM.

import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon, type ISearchOptions } from "@xterm/addon-search";
import { SerializeAddon } from "@xterm/addon-serialize";
import { Terminal } from "@xterm/xterm";

import { api, Channel, type SessionEvent, type SessionFallback, type TermSize } from "./api";
import {
  AttentionTracker,
  systemClock,
  type AttentionConfig,
  type AttentionReason,
  type AttentionState,
} from "./attention";
import { readClipboard, writeClipboard } from "./clipboard";
import { pasteFromClipboard } from "./clipboardPaste";
import { DevServerDetector } from "./devServer";
import {
  clipboardWriteText,
  menuSelection,
  ProgramSelection,
  watchPresses,
} from "./programSelection";
import { t } from "./i18n";
import { FONT_SIZE } from "./layout";
import type { ShellKind } from "./pathQuote";
import { PathChecker } from "./pathCheck";
import { PromptLineTracker } from "./promptTitle";
import { SCROLLBACK_LINES } from "./settings";
import type { Activity } from "./statusLine";
import { tailLines } from "./overview";
import { PEEK_COLUMNS, plainLines, styledTail, type PeekContent, type PeekLine } from "./peekStyle";
import { bufferText, htmlDocument, type BufferRow } from "./sessionExport";
import {
  columnOf,
  relocate,
  rowOfOffset,
  type CellText,
  type LineMatch,
  type RowSource,
} from "./sessionSearch";
import { arrowsToColumn, clickMovesCursor, type ClickCell } from "./terminalClick";
import { terminalKeyAction } from "./terminalKeys";
import { TerminalLinks, type LinkRequest } from "./terminalLinkProvider";
import { guardSecondaryClick } from "./terminalMouse";
import { setupUnicode } from "./terminalUnicode";
import {
  createFontLoader,
  DEFAULT_TERMINAL_LOOK,
  fontStack,
  sameLook,
  schemeColors,
  searchColors,
  type TerminalLook,
} from "./terminalLook";
import { safeLinkUrl, sanitizePaste } from "./termSafety";
import { MINIMUM_CONTRAST, type Theme } from "./theme";
import { newSessionKey, trimScrollback } from "./workspace";

/**
 * For this long after the first output of a restored terminal, "clear screen" pushes the content
 * into the scrollback instead of erasing it. ConPTY clears the screen once at startup; without
 * this, the restored text would be overwritten at once. Afterwards the usual behavior applies
 * again, so agents that redraw often do not fill the scrollback with copies.
 */
const RESTORE_ERASE_GRACE_MS = 1500;

/** Shared by all terminals, so each bundled font loads once. */
const fontLoader = createFontLoader((query) => document.fonts.load(query));

/** Shared by all terminals: a path printed in two sessions is looked up once. */
const pathChecker = new PathChecker((paths) => api.statPaths(paths));

export interface TerminalOptions {
  fontSize?: number;
  attention?: AttentionConfig;
  theme?: Theme;
  /** Color scheme, font and cursor (Settings); defaults to the previous look. */
  look?: TerminalLook;
  /** Lines of scrollback in xterm; the same number that is saved (Settings). */
  scrollback?: number;
  /** Stable key; if missing, a new one is created (new session). */
  key?: string;
}

/** Result of a search: `index` is 0 based, -1 when there are too many hits to number them. */
export interface SearchResults {
  index: number;
  count: number;
}

export interface FindOptions {
  caseSensitive: boolean;
  /** Towards older output (up). */
  backwards: boolean;
  /**
   * While typing: start again from the newest output instead of moving on from the current hit,
   * so a longer term still finds the hit the shorter one had.
   */
  incremental: boolean;
}

/** More hits than this are counted but not highlighted (cost per redraw). */
const SEARCH_HIGHLIGHT_LIMIT = 2000;

export class SessionTerminal {
  readonly term: Terminal;
  /** Stable key for the workspace and scrollback; stays the same across restarts. */
  readonly key: string;
  private readonly fitAddon = new FitAddon();
  private readonly serializeAddon = new SerializeAddon();
  private readonly searchAddon = new SearchAddon({ highlightLimit: SEARCH_HIGHLIGHT_LIMIT });
  private searchListener: ((results: SearchResults) => void) | null = null;
  /** What a program with mouse tracking selected itself (programSelection.ts). */
  private readonly programSelection = new ProgramSelection();
  private theme: Theme;
  private look: TerminalLook;
  /** New output since the scrollback was last saved. */
  dirty = false;
  /** Time of the last output (performance.now), so saving waits until things are quiet. */
  lastOutputAt = 0;
  private restoreGrace: "off" | "waiting" | "timer" = "off";
  private id: number | null = null;
  /** Acknowledgements that arrive before the backend has delivered the ID. */
  private pendingAck = 0;
  /**
   * Input before the ID is known. Important: ConPTY queries the cursor position via ESC[6n right
   * at startup and blocks until it gets an answer. If xterm's reply were lost, the session would
   * stay empty.
   */
  private pendingInput = "";
  /** Last write to the backend; broadcast waits for it before sending Enter. */
  private lastWrite: Promise<unknown> = Promise.resolve();
  private opened = false;
  private disposed = false;
  /** `undefined`: still running. Set as soon as the exit event arrives. */
  exitCode: number | null | undefined = undefined;
  onExit: ((code: number | null) => void) | null = null;
  /**
   * The newest `resumeFallback` (conversation id and sub-agent flag of the normal start). Kept,
   * because the event may arrive before `create_session` has answered and the session exists.
   */
  fallback: SessionFallback | undefined = undefined;
  onFallback: ((fallback: SessionFallback) => void) | null = null;
  /** Click on a link (OSC 8) with an allowed address; only http and https get this far. */
  onLink: ((url: string) => void) | null = null;
  /** Click on an address or existing path in the output (terminalLinkProvider.ts). */
  onOpenLink: ((request: LinkRequest) => void) | null = null;
  /** The session's folder, for relative paths in the output; the app knows the session. */
  cwd: () => string | null = () => null;
  private readonly links: TerminalLinks;
  /** The clipboard held an image but no text, and saving the image failed. */
  onImagePasteFailed: (() => void) | null = null;
  /**
   * Lines the user submits with Enter, while the app may still name the session after its first
   * task (promptTitle.ts); `null` stops the tracking.
   */
  onPromptLine: ((line: string) => void) | null = null;
  private readonly promptLines = new PromptLineTracker();
  /**
   * The newest local dev server address in the current process's output changed (devServer.ts);
   * `null` when the process ended or started again. Called on changes only, never per chunk.
   */
  onDevServer: ((url: string | null) => void) | null = null;
  private readonly devServer = new DevServerDetector();
  /** The user typed, pasted or sent text since `takeTyped` last asked. */
  private typedSinceTaken = false;
  /** How the program in the session reads a pasted image path; the app knows its profile. */
  shellKind: () => ShellKind = () => "agent";
  /** An image paste of this session is waiting for the backend (clipboardPaste.ts). */
  imagePasteInFlight = false;
  /** Active session in a focused window; the app sets it as soon as the ID is known. */
  isWatched: () => boolean = () => false;
  readonly attention: AttentionTracker;
  /** Start of the current working phase (Date.now) for the status line; `null` otherwise. */
  private workingSince: number | null = null;
  private attentionListener:
    ((state: AttentionState, reason: AttentionReason | null) => void) | null = null;

  constructor(options: TerminalOptions = {}) {
    const {
      fontSize = FONT_SIZE.default,
      attention = { enabled: false, quietMs: 3000 },
      theme = "dark",
      look = DEFAULT_TERMINAL_LOOK,
      scrollback = SCROLLBACK_LINES.default,
      key = newSessionKey(),
    } = options;
    this.key = key;
    this.theme = theme;
    this.look = look;
    this.attention = new AttentionTracker(
      systemClock,
      attention,
      () => this.isWatched(),
      (state, reason) => {
        // Only state changes arrive here, so this costs nothing per output chunk.
        if (state !== "working") this.workingSince = null;
        else this.workingSince ??= Date.now();
        this.attentionListener?.(state, reason);
      },
    );
    this.term = new Terminal({
      allowProposedApi: true, // for the Unicode grapheme addon (emoji widths)
      cursorBlink: look.cursorBlink,
      cursorStyle: look.cursorStyle,
      // Until a bundled font has loaded, the previous stack; applyFont switches once it is ready.
      fontFamily: fontStack(fontLoader.isReady(look.font) ? look.font : "default"),
      fontSize,
      lineHeight: 1.15,
      scrollback,
      theme: schemeColors(look.scheme, theme),
      minimumContrastRatio: MINIMUM_CONTRAST[theme],
      // Replaces xterm's default, which asks via confirm() and opened the address in the WebView
      // via window.open. xterm does not even let schemes other than http and https through here.
      linkHandler: {
        activate: (event, text) => {
          event.preventDefault();
          const url = safeLinkUrl(text);
          if (url !== null) this.onLink?.(url);
        },
        allowNonHttpProtocols: false,
      },
    });
    this.links = new TerminalLinks(this.term, {
      cwd: () => this.cwd(),
      checker: pathChecker,
      open: (request) => {
        this.onOpenLink?.(request);
      },
    });
    this.term.loadAddon(this.fitAddon);
    this.applyFont();
    setupUnicode(this.term);
    this.term.loadAddon(this.serializeAddon);
    this.term.loadAddon(this.searchAddon);
    this.searchAddon.onDidChangeResults(({ resultIndex, resultCount }) => {
      this.searchListener?.({ index: resultIndex, count: resultCount });
    });

    this.term.onData((data) => {
      this.send(data);
      this.trackPrompt(data);
    });
    // Real key presses only, not xterm's automatic replies (cursor position, focus).
    this.term.onKey(() => {
      this.typedSinceTaken = true;
      this.attention.typed();
      this.programSelection.keyTyped();
    });
    // xterm has no OSC 52 handler: the clipboard write is only read here, not carried out, and
    // `false` leaves it to any later handler.
    this.term.parser.registerOscHandler(52, (data) => {
      const text = clipboardWriteText(data);
      if (text !== null) this.programSelection.clipboardWritten(text);
      return false;
    });
    this.term.onBell(() => {
      this.attention.bell();
    });
    this.term.onResize(({ cols, rows }) => {
      this.attention.resized();
      if (this.id !== null) api.resize(this.id, { cols, rows }).catch(ignore);
    });
    this.term.attachCustomKeyEventHandler((event) => this.handleKey(event));
  }

  /** A new channel per process start: a Tauri channel belongs to exactly one backend call. */
  createChannel(): Channel<SessionEvent> {
    // A new process: an address from the old one belongs to a server that is gone.
    this.forgetDevServer();
    const channel = new Channel<SessionEvent>();
    channel.onmessage = (event) => {
      this.handleEvent(event);
    };
    return channel;
  }

  /** Times for the status line, sampled by the sidebar on its clock tick. */
  activity(now: number = Date.now()): Activity {
    return {
      workingSince: this.workingSince,
      // lastOutputAt is on the performance clock; the status line works with wall clock times.
      lastOutput: this.lastOutputAt > 0 ? now - (performance.now() - this.lastOutputAt) : null,
    };
  }

  /** Reports state changes only; calls the listener right away with the current state. */
  setAttentionListener(
    listener: (state: AttentionState, reason: AttentionReason | null) => void,
  ): void {
    this.attentionListener = listener;
    listener(this.attention.state, this.attention.reason);
  }

  setId(id: number): void {
    this.id = id;
    if (this.pendingInput) {
      api.write(id, this.pendingInput).catch(ignore);
      this.pendingInput = "";
    }
    if (this.pendingAck > 0) {
      api.ack(id, this.pendingAck).catch(ignore);
      this.pendingAck = 0;
    }
  }

  size(): TermSize {
    return { cols: this.term.cols, rows: this.term.rows };
  }

  mount(host: HTMLElement): void {
    if (!this.opened) {
      this.term.open(host);
      this.opened = true;
      // Many TUIs redraw on focus change (focus reports); that is not work.
      const onFocusChange = () => {
        this.attention.focusChanged();
      };
      this.term.textarea?.addEventListener("focus", onFocusChange);
      this.term.textarea?.addEventListener("blur", onFocusChange);
      // Shift+Insert goes through the paste event, not through pasteClipboard. In the capture
      // phase, so xterm's own handler never sees the raw text.
      const onPaste = (event: ClipboardEvent) => {
        this.handlePaste(event);
      };
      this.term.textarea?.addEventListener("paste", onPaste, { capture: true });
      // The right button opens the app's terminal menu and is never reported to the program,
      // which could paste on it (terminalMouse.ts). Lives as long as the terminal's element.
      if (this.term.element) {
        watchPresses(this.term.element, this.programSelection);
        guardSecondaryClick(this.term.element, () => {
          this.term.focus();
        });
        this.watchClicks(this.term.element);
        this.links.watch(this.term.element);
      }
    } else if (this.term.element && this.term.element.parentElement !== host) {
      host.appendChild(this.term.element);
    }
    this.fit();
  }

  /**
   * A plain click on the cursor's line of an agent's prompt moves the cursor there
   * (terminalClick.ts). A drag, a double click or a modifier keeps xterm's own handling.
   */
  private watchClicks(element: HTMLElement): void {
    let press: { x: number; y: number } | null = null;
    element.addEventListener("mousedown", (event) => {
      this.links.takeOpened();
      press =
        event.button === 0 && event.detail === 1 ? { x: event.clientX, y: event.clientY } : null;
    });
    element.addEventListener("mouseup", (event) => {
      const start = press;
      press = null;
      if (start === null || event.button !== 0) return;
      if (Math.abs(event.clientX - start.x) > 3 || Math.abs(event.clientY - start.y) > 3) return;
      const modifier = event.altKey || event.shiftKey || event.ctrlKey || event.metaKey;
      const { clientX, clientY } = event;
      // A click that opened a link (xterm handled it before this listener) moves nothing.
      if (this.links.takeOpened()) return;
      // xterm settles its selection in its own mouseup handling; read it afterwards.
      setTimeout(() => {
        this.moveCursorToClick(clientX, clientY, modifier);
      }, 0);
    });
  }

  private moveCursorToClick(clientX: number, clientY: number, modifier: boolean): void {
    const term = this.term;
    const context = {
      shell: this.shellKind(),
      mouseTracking: term.modes.mouseTrackingMode,
      selected: term.hasSelection(),
      modifier,
    };
    if (!clickMovesCursor(context)) return;
    const screen = term.element?.querySelector(".xterm-screen");
    if (!screen || term.cols === 0 || term.rows === 0) return;
    const rect = screen.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return;
    const col = Math.floor(((clientX - rect.left) / rect.width) * term.cols);
    const row = Math.floor(((clientY - rect.top) / rect.height) * term.rows);
    if (col < 0 || col >= term.cols || row < 0 || row >= term.rows) return;
    const buffer = term.buffer.active;
    // Only the cursor's own line: other lines would need Up and Down, which page the history.
    if (buffer.viewportY + row !== buffer.baseY + buffer.cursorY) return;
    const line = buffer.getLine(buffer.baseY + buffer.cursorY);
    if (!line) return;
    const cells: ClickCell[] = [];
    for (let x = 0; x < line.length; x += 1)
      cells.push({ width: line.getCell(x)?.getWidth() ?? 1 });
    const keys = arrowsToColumn(cells, buffer.cursorX, col, term.modes.applicationCursorKeysMode);
    if (keys !== "") this.send(keys);
  }

  fit(): void {
    const host = this.term.element?.parentElement;
    if (!host || host.clientWidth === 0 || host.clientHeight === 0) return;
    this.fitAddon.fit();
  }

  focus(): void {
    this.term.focus();
  }

  /** Shortening drops the oldest lines at once; lengthening applies to new output. */
  setScrollback(lines: number): void {
    if (this.term.options.scrollback === lines) return;
    this.term.options.scrollback = lines;
  }

  setFontSize(size: number): void {
    if (this.term.options.fontSize === size) return;
    this.term.options.fontSize = size;
    this.fit();
  }

  /**
   * Light or dark, color scheme, font and cursor. xterm only redraws; content and scrollback
   * stay. A new font changes the cell size, so the terminal fits itself again.
   */
  setLook(look: TerminalLook, theme: Theme): void {
    if (theme === this.theme && sameLook(look, this.look)) return;
    const fontChanged = look.font !== this.look.font;
    this.theme = theme;
    this.look = look;
    const options = this.term.options;
    options.theme = schemeColors(look.scheme, theme);
    options.minimumContrastRatio = MINIMUM_CONTRAST[theme];
    options.cursorStyle = look.cursorStyle;
    options.cursorBlink = look.cursorBlink;
    if (fontChanged) this.applyFont();
  }

  /**
   * Sets the chosen font once it can be measured; until then the current one stays. Setting the
   * same stack again would not make xterm measure anew, hence the wait instead of a later reset.
   */
  private applyFont(): void {
    const font = this.look.font;
    const apply = (): void => {
      if (this.disposed || this.look.font !== font) return;
      const family = fontStack(font);
      if (this.term.options.fontFamily === family) return;
      this.term.options.fontFamily = family;
      this.fit();
    };
    if (fontLoader.isReady(font)) apply();
    else void fontLoader.load(font).then(apply);
  }

  /** Background of the current scheme, for the HTML export. */
  background(): string {
    return schemeColors(this.look.scheme, this.theme).background;
  }

  /** Reports hit counts of the running search; `null` stops the reports. */
  setSearchListener(listener: ((results: SearchResults) => void) | null): void {
    this.searchListener = listener;
  }

  /** Highlights all hits and selects the next one; an empty term clears the search. */
  find(term: string, options: FindOptions): void {
    if (term === "") {
      this.clearSearch();
      this.searchListener?.({ index: -1, count: 0 });
      return;
    }
    const colors = searchColors(this.look.scheme, this.theme);
    const search: ISearchOptions = {
      caseSensitive: options.caseSensitive,
      incremental: options.incremental,
      decorations: {
        matchBackground: colors.match,
        matchOverviewRuler: colors.match,
        activeMatchBackground: colors.active,
        activeMatchColorOverviewRuler: colors.active,
      },
    };
    if (!options.backwards) {
      this.searchAddon.findNext(term, search);
      return;
    }
    // Without a selection, the search starts at the bottom (newest output).
    if (options.incremental) this.term.clearSelection();
    this.searchAddon.findPrevious(term, search);
  }

  /** Start term for a new search: the selected text, if it is short and on one line. */
  searchSeed(): string {
    const text = this.term.getSelection();
    return text.length > 0 && text.length <= 200 && !/[\r\n]/.test(text) ? text : "";
  }

  clearSearch(): void {
    this.searchAddon.clearDecorations();
  }

  /**
   * The last `count` rows with content in their colors, for the session peek. Falls back to plain
   * text when the cells cannot be read.
   */
  peekContent(count: number): PeekContent {
    const colors = schemeColors(this.look.scheme, this.theme);
    let lines: PeekLine[];
    try {
      lines = styledTail(
        this.term.buffer.active,
        count,
        colors,
        PEEK_COLUMNS,
        MINIMUM_CONTRAST[this.theme],
      );
    } catch {
      lines = plainLines(tailLines(this.rowSource(), count));
    }
    return { lines, background: colors.background, foreground: colors.foreground };
  }

  /**
   * The visible buffer as rows for the search across sessions and the overview. Reads lazily: the
   * caller decides how many rows to touch and when to pause.
   */
  rowSource(): RowSource {
    const buffer = this.term.buffer.active;
    return {
      length: buffer.length,
      row: (index) => {
        const line = buffer.getLine(index);
        return line === undefined
          ? undefined
          : { text: line.translateToString(false), wrapped: line.isWrapped };
      },
    };
  }

  /**
   * Scrolls to a hit of the search across sessions and selects it. The line is looked up again
   * by its text, since new output may have moved it; `false` when it is gone.
   */
  revealMatch(match: Pick<LineMatch, "row" | "text" | "start" | "length">): boolean {
    const buffer = this.term.buffer.active;
    const line = relocate(this.rowSource(), match.row, match.text);
    if (line === null) return false;
    const cellsOf = (row: number): CellText[] => {
      const bufferLine = buffer.getLine(row);
      if (!bufferLine) return [];
      const cells: CellText[] = [];
      for (let column = 0; column < bufferLine.length; column += 1) {
        const cell = bufferLine.getCell(column);
        cells.push({ chars: cell?.getChars() ?? "", width: cell?.getWidth() ?? 1 });
      }
      return cells;
    };
    const from = rowOfOffset(line, match.start);
    const to = rowOfOffset(line, match.start + match.length);
    const startColumn = columnOf(cellsOf(from.row), from.offset);
    const endColumn = columnOf(cellsOf(to.row), to.offset);
    const length = (to.row - from.row) * this.term.cols + endColumn - startColumn;
    this.term.scrollToLine(Math.max(0, from.row - Math.floor(this.term.rows / 2)));
    this.term.select(startColumn, from.row, Math.max(1, length));
    return true;
  }

  clear(): void {
    this.term.clear();
  }

  /** Whether the terminal menu has a selection to copy or add to a note. */
  hasSelection(): boolean {
    return this.selectionText() !== "";
  }

  /**
   * The selected text, empty without a selection: xterm's own selection, else what a program
   * with mouse tracking selected itself ("Copy", "Add selection to note").
   */
  selectionText(): string {
    return menuSelection(this.term.getSelection(), this.programSelection.current);
  }

  selectAll(): void {
    this.term.selectAll();
  }

  copySelection(): void {
    const text = this.selectionText();
    if (text) writeClipboard(text).catch(ignore);
  }

  /**
   * Goes through term.paste so bracketed paste applies when the program has turned it on.
   * Control characters are stripped first (sanitizePaste), or the text could end bracketed paste.
   * Without text, the path of the clipboard's image goes in (clipboardPaste.ts).
   */
  pasteClipboard(): void {
    readClipboard()
      // Reading text fails when the clipboard holds only an image.
      .catch(() => "")
      .then((text) => this.pasteClipboardContent(text))
      .catch(ignore);
  }

  /** Text pastes synchronously, before the first await; only the image path waits. */
  private async pasteClipboardContent(text: string): Promise<void> {
    const result = await pasteFromClipboard(this, text, () => api.pasteClipboardImage());
    if (result === "failed") this.onImagePasteFailed?.();
  }

  /**
   * Whether the program has turned on bracketed paste (DECSET 2004). Without it `term.paste`
   * turns line breaks into carriage returns, so pasted lines run (appPaste.ts).
   */
  bracketedPaste(): boolean {
    return this.term.modes.bracketedPasteMode;
  }

  /** Text from the app (dropped paths): the same path as pasting, with bracketed paste. */
  pasteText(text: string): void {
    const clean = sanitizePaste(text);
    // An image path can arrive after the session was closed.
    if (clean === "" || this.disposed) return;
    this.typedSinceTaken = true;
    this.attention.typed();
    this.term.paste(clean);
  }

  /**
   * Like `pasteText`, but resolves once the backend has the text. Used by "Send to several",
   * which sends Enter afterwards as a separate write.
   */
  pasteAndWait(text: string): Promise<void> {
    this.pasteText(text);
    return this.lastWrite.then(ignore, ignore);
  }

  /** Enter, as if typed: runs what was pasted before. */
  submit(): void {
    this.attention.typed();
    this.typedSinceTaken = true;
    this.send("\r");
    this.trackPrompt("\r");
  }

  /**
   * Whether the user gave the session input since the last call; resets. Work that follows input
   * is a task the user asked for, unlike a program's own start-up output.
   */
  takeTyped(): boolean {
    const typed = this.typedSinceTaken;
    this.typedSinceTaken = false;
    return typed;
  }

  /** Only while the app listens; typed input is small, so this costs nothing measurable. */
  private trackPrompt(data: string): void {
    if (this.onPromptLine === null) return;
    for (const line of this.promptLines.feed(data)) this.emitPromptLine(line);
  }

  /** Read anew per line: the listener may stop itself after the first one. */
  private emitPromptLine(line: string): void {
    this.onPromptLine?.(line);
  }

  private forgetDevServer(): void {
    if (this.devServer.reset()) this.onDevServer?.(null);
  }

  private send(data: string): void {
    if (this.id === null) {
      this.pendingInput += data;
      return;
    }
    this.lastWrite = api.write(this.id, data).catch(ignore);
  }

  private handlePaste(event: ClipboardEvent): void {
    event.preventDefault();
    event.stopImmediatePropagation();
    this.pasteClipboardContent(event.clipboardData?.getData("text/plain") ?? "").catch(ignore);
  }

  /**
   * Scrollback as escape sequences: at most `lines` lines of scrollback plus the screen, and under
   * the backend's limit. Without modes (mouse, bracketed paste) and without the alternate buffer:
   * they belong to the old process and would disturb the new program.
   */
  serialize(lines: number): string {
    return trimScrollback(
      this.serializeAddon.serialize({
        scrollback: lines,
        excludeModes: true,
        excludeAltBuffer: true,
      }),
    );
  }

  /** Whole scrollback as plain text, soft wrapped rows joined into their lines. */
  exportText(eol: string): string {
    const buffer = this.term.buffer.normal;
    const rows: BufferRow[] = [];
    for (let index = 0; index < buffer.length; index += 1) {
      const line = buffer.getLine(index);
      if (!line) continue;
      // Keep trailing spaces of a row that continues on the next one: they belong to the text.
      const continues = buffer.getLine(index + 1)?.isWrapped === true;
      rows.push({ text: line.translateToString(!continues), wrapped: line.isWrapped });
    }
    return bufferText(rows, eol);
  }

  /** Whole scrollback with colors as a standalone HTML file. */
  exportHtml(title: string): string {
    const fragment = this.serializeAddon.serializeAsHTML({ includeGlobalBackground: true });
    return htmlDocument(fragment, title, this.background());
  }

  /**
   * Writes saved scrollback with a dimmed separator line below it before the process starts.
   * The separator goes below the last written line, even if the old cursor was further up
   * (full screen programs).
   */
  restore(data: string, banner: string): Promise<void> {
    this.restoreGrace = "waiting";
    this.term.options.scrollOnEraseInDisplay = true;
    return new Promise((resolve) => {
      this.term.write(`${data}\x1b[0m`, () => {
        this.moveBelowContent();
        this.term.write(`\x1b[2m${banner}\x1b[0m\r\n`, resolve);
      });
    });
  }

  /** A notice line when there is no saved scrollback, e.g. "not installed". */
  writeNotice(text: string): void {
    this.term.write(`\x1b[2m${text}\x1b[0m\r\n`);
  }

  private moveBelowContent(): void {
    const buffer = this.term.buffer.active;
    let last = buffer.length - 1;
    while (last >= 0 && (buffer.getLine(last)?.translateToString(true).trim() ?? "") === "") {
      last -= 1;
    }
    if (last < 0) return;
    const row = last - buffer.baseY;
    if (row >= 0 && row < this.term.rows) {
      // Cursor to the start of the last written line, then one line down.
      this.term.write(`\x1b[${String(row + 1)};1H\r\n`);
    } else {
      this.term.write("\r\n");
    }
  }

  markRestarted(): void {
    this.exitCode = undefined;
    this.fallback = undefined;
    this.attention.reset();
    this.programSelection.reset();
    this.term.write(`\r\n\x1b[2m${t("terminal.restarted")}\x1b[0m\r\n`);
  }

  dispose(): void {
    this.disposed = true;
    this.onExit = null;
    this.onFallback = null;
    this.onLink = null;
    this.onOpenLink = null;
    this.links.dispose();
    this.onImagePasteFailed = null;
    this.onPromptLine = null;
    this.onDevServer = null;
    this.attentionListener = null;
    this.searchListener = null;
    this.attention.dispose();
    this.term.dispose();
  }

  private handleEvent(event: SessionEvent): void {
    if (event.type === "resumeFallback") {
      this.attention.reset();
      this.term.write(`\r\n\x1b[2m${t("terminal.resumeFallback")}\x1b[0m\r\n`);
      // Older backends send neither field: nothing changes then.
      if (event.cliSessionId !== undefined || event.subAgents !== undefined) {
        this.fallback = {
          cliSessionId: event.cliSessionId ?? null,
          subAgents: event.subAgents ?? false,
        };
        this.onFallback?.(this.fallback);
      }
      return;
    }
    if (event.type === "output") {
      this.dirty = true;
      this.lastOutputAt = performance.now();
      this.endRestoreGrace();
      this.attention.output(event.bytes);
      if (this.devServer.feed(event.data)) this.onDevServer?.(this.devServer.url);
      // Acknowledge as soon as xterm has processed the chunk: this is the backpressure to the backend.
      this.term.write(event.data, () => {
        this.acknowledge(event.bytes);
      });
      return;
    }
    this.exitCode = event.code;
    this.programSelection.reset();
    this.forgetDevServer();
    const code = event.code === null ? t("terminal.unknownCode") : String(event.code);
    this.term.write(`\r\n\x1b[2m${t("terminal.exited", { code })}\x1b[0m\r\n`);
    this.onExit?.(event.code);
    this.attention.exited();
  }

  private endRestoreGrace(): void {
    if (this.restoreGrace !== "waiting") return;
    this.restoreGrace = "timer";
    window.setTimeout(() => {
      this.restoreGrace = "off";
      if (!this.disposed) this.term.options.scrollOnEraseInDisplay = false;
    }, RESTORE_ERASE_GRACE_MS);
  }

  private acknowledge(bytes: number): void {
    if (this.id === null) {
      this.pendingAck += bytes;
      return;
    }
    api.ack(this.id, bytes).catch(ignore);
  }

  /** `false` means xterm does not handle the key. Ctrl+C without a selection goes to the process. */
  private handleKey(event: KeyboardEvent): boolean {
    const action = terminalKeyAction(
      event,
      this.term.hasSelection(),
      focusCycleActive,
      this.shellKind() === "agent",
    );
    switch (action.kind) {
      case "app":
        // App shortcuts (session switching, sidebar, font) belong to the app, not to the process.
        return false;
      case "copy":
        this.copySelection();
        if (action.clearSelection) this.term.clearSelection();
        event.preventDefault();
        return false;
      case "paste":
        // No preventDefault: the native paste event reaches handlePaste. Reading the clipboard
        // from script here failed silently in WebView2, so Ctrl+V pasted nothing.
        return false;
      case "newline":
        // Also keeps the browser from putting a line break into xterm's hidden textarea.
        event.preventDefault();
        if (action.send !== null) {
          this.typedSinceTaken = true;
          this.attention.typed();
          this.send(action.send);
        }
        return false;
      case "terminal":
        return true;
    }
  }
}

function ignore(): void {
  // Errors for single keys or acknowledgements are harmless: the session reports its own exit.
}

/** F6 moves the focus between terminal and notes (App); then no terminal passes it on. */
let focusCycleActive = false;

export function setFocusCycleActive(active: boolean): void {
  focusCycleActive = active;
}

/** Registry of all terminal instances. Deliberately not in React state (not serializable). */
export const terminals = new Map<number, SessionTerminal>();

// One xterm.js instance per session. Lives outside React so scrollback and state survive
// switching in the sidebar; React only attaches it to the DOM.

import { FitAddon } from "@xterm/addon-fit";
import { SerializeAddon } from "@xterm/addon-serialize";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { Terminal } from "@xterm/xterm";

import { api, Channel, type SessionEvent, type TermSize } from "./api";
import {
  AttentionTracker,
  systemClock,
  type AttentionConfig,
  type AttentionState,
} from "./attention";
import { t } from "./i18n";
import { FONT_SIZE } from "./layout";
import { matchShortcut } from "./shortcuts";
import { SCROLLBACK_LINES } from "./settings";
import { safeLinkUrl, sanitizePaste } from "./termSafety";
import { MINIMUM_CONTRAST, TERMINAL_THEMES, type Theme } from "./theme";
import { newSessionKey, trimScrollback } from "./workspace";

/**
 * For this long after the first output of a restored terminal, "clear screen" pushes the content
 * into the scrollback instead of erasing it. ConPTY clears the screen once at startup; without
 * this, the restored text would be overwritten at once. Afterwards the usual behavior applies
 * again, so agents that redraw often do not fill the scrollback with copies.
 */
const RESTORE_ERASE_GRACE_MS = 1500;

export interface TerminalOptions {
  fontSize?: number;
  attention?: AttentionConfig;
  theme?: Theme;
  /** Lines of scrollback in xterm; the same number that is saved (Settings). */
  scrollback?: number;
  /** Stable key; if missing, a new one is created (new session). */
  key?: string;
}

export class SessionTerminal {
  readonly term: Terminal;
  /** Stable key for the workspace and scrollback; stays the same across restarts. */
  readonly key: string;
  private readonly fitAddon = new FitAddon();
  private readonly serializeAddon = new SerializeAddon();
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
  private opened = false;
  private disposed = false;
  /** `undefined`: still running. Set as soon as the exit event arrives. */
  exitCode: number | null | undefined = undefined;
  onExit: ((code: number | null) => void) | null = null;
  /** Click on a link (OSC 8) with an allowed address; only http and https get this far. */
  onLink: ((url: string) => void) | null = null;
  /** Active session in a focused window; the app sets it as soon as the ID is known. */
  isWatched: () => boolean = () => false;
  readonly attention: AttentionTracker;
  private attentionListener: ((state: AttentionState) => void) | null = null;

  constructor(options: TerminalOptions = {}) {
    const {
      fontSize = FONT_SIZE.default,
      attention = { enabled: false, quietMs: 3000 },
      theme = "dark",
      scrollback = SCROLLBACK_LINES.default,
      key = newSessionKey(),
    } = options;
    this.key = key;
    this.attention = new AttentionTracker(
      systemClock,
      attention,
      () => this.isWatched(),
      (state) => this.attentionListener?.(state),
    );
    this.term = new Terminal({
      allowProposedApi: true, // for the Unicode 11 addon (emoji widths)
      cursorBlink: true,
      fontFamily: '"Cascadia Mono", "Cascadia Code", Consolas, "DejaVu Sans Mono", monospace',
      fontSize,
      lineHeight: 1.15,
      scrollback,
      theme: TERMINAL_THEMES[theme],
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
    this.term.loadAddon(this.fitAddon);
    this.term.loadAddon(new Unicode11Addon());
    this.term.loadAddon(this.serializeAddon);
    this.term.unicode.activeVersion = "11";

    this.term.onData((data) => {
      if (this.id === null) {
        this.pendingInput += data;
        return;
      }
      api.write(this.id, data).catch(ignore);
    });
    // Real key presses only, not xterm's automatic replies (cursor position, focus).
    this.term.onKey(() => {
      this.attention.typed();
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
    const channel = new Channel<SessionEvent>();
    channel.onmessage = (event) => {
      this.handleEvent(event);
    };
    return channel;
  }

  /** Reports state changes only; calls the listener right away with the current state. */
  setAttentionListener(listener: (state: AttentionState) => void): void {
    this.attentionListener = listener;
    listener(this.attention.state);
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
    } else if (this.term.element && this.term.element.parentElement !== host) {
      host.appendChild(this.term.element);
    }
    this.fit();
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

  /** Switch between light and dark. xterm only redraws; content and scrollback stay. */
  setTheme(theme: Theme): void {
    if (this.term.options.theme === TERMINAL_THEMES[theme]) return;
    this.term.options.theme = TERMINAL_THEMES[theme];
    this.term.options.minimumContrastRatio = MINIMUM_CONTRAST[theme];
  }

  clear(): void {
    this.term.clear();
  }

  hasSelection(): boolean {
    return this.term.hasSelection();
  }

  selectAll(): void {
    this.term.selectAll();
  }

  copySelection(): void {
    const text = this.term.getSelection();
    if (text) navigator.clipboard.writeText(text).catch(ignore);
  }

  /**
   * Goes through term.paste so bracketed paste applies when the program has turned it on.
   * Control characters are stripped first (sanitizePaste), or the text could end bracketed paste.
   */
  pasteClipboard(): void {
    navigator.clipboard
      .readText()
      .then((text) => {
        this.pasteText(text);
      })
      .catch(ignore);
  }

  private pasteText(text: string): void {
    const clean = sanitizePaste(text);
    if (clean === "") return;
    this.attention.typed();
    this.term.paste(clean);
  }

  private handlePaste(event: ClipboardEvent): void {
    event.preventDefault();
    event.stopImmediatePropagation();
    this.pasteText(event.clipboardData?.getData("text/plain") ?? "");
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
    this.attention.reset();
    this.term.write(`\r\n\x1b[2m${t("terminal.restarted")}\x1b[0m\r\n`);
  }

  dispose(): void {
    this.disposed = true;
    this.onExit = null;
    this.onLink = null;
    this.attentionListener = null;
    this.attention.dispose();
    this.term.dispose();
  }

  private handleEvent(event: SessionEvent): void {
    if (event.type === "resumeFallback") {
      this.attention.reset();
      this.term.write(`\r\n\x1b[2m${t("terminal.resumeFallback")}\x1b[0m\r\n`);
      return;
    }
    if (event.type === "output") {
      this.dirty = true;
      this.lastOutputAt = performance.now();
      this.endRestoreGrace();
      this.attention.output(event.bytes);
      // Acknowledge as soon as xterm has processed the chunk: this is the backpressure to the backend.
      this.term.write(event.data, () => {
        this.acknowledge(event.bytes);
      });
      return;
    }
    this.exitCode = event.code;
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
    // App shortcuts (session switching, sidebar, font) belong to the app, not to the process.
    if (matchShortcut(event)) return false;
    if (event.type !== "keydown" || !event.ctrlKey || event.altKey || event.metaKey) return true;

    const copy = event.code === "KeyC" && (event.shiftKey || this.term.hasSelection());
    if (copy) {
      this.copySelection();
      if (!event.shiftKey) this.term.clearSelection();
      event.preventDefault();
      return false;
    }
    if (event.code === "KeyV") {
      this.pasteClipboard();
      event.preventDefault();
      return false;
    }
    return true;
  }
}

function ignore(): void {
  // Errors for single keys or acknowledgements are harmless: the session reports its own exit.
}

/** Registry of all terminal instances. Deliberately not in React state (not serializable). */
export const terminals = new Map<number, SessionTerminal>();

import { useEffect, useRef } from "react";

import { useT } from "./i18n";
import type { SessionTerminal } from "./terminal";

interface Props {
  terminal: SessionTerminal;
  /** Session ID; drag and drop finds the pane under the cursor by it (`data-session`). */
  sessionId: number;
  active: boolean;
  /** Files are being dragged over this pane: show where they will land. */
  dropping: boolean;
}

/**
 * All panes stay stacked in the DOM; inactive ones are merely invisible. That way they keep
 * their size, the scrollback survives, and switching needs no rebuild. xterm lives in its own
 * host element, so React never touches the nodes xterm creates.
 */
export function TerminalPane({ terminal, sessionId, active, dropping }: Props) {
  const t = useT();
  const host = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const element = host.current;
    if (!element) return;
    terminal.mount(element);
    const observer = new ResizeObserver(() => {
      terminal.fit();
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, [terminal]);

  useEffect(() => {
    if (!active) return;
    terminal.fit();
    terminal.focus();
  }, [active, terminal]);

  return (
    <div className={active ? "pane active" : "pane"} data-session={sessionId} aria-hidden={!active}>
      <div ref={host} className="pane-host" />
      {dropping && (
        <div className="drop-hint" aria-hidden="true">
          <span className="drop-hint-label">{t("drop.hint")}</span>
        </div>
      )}
    </div>
  );
}

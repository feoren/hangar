import { useEffect, useRef } from "react";

import type { SessionTerminal } from "./terminal";

interface Props {
  terminal: SessionTerminal;
  active: boolean;
}

/**
 * All panes stay stacked in the DOM; inactive ones are merely invisible. That way they keep
 * their size, the scrollback survives, and switching needs no rebuild.
 */
export function TerminalPane({ terminal, active }: Props) {
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

  return <div ref={host} className={active ? "pane active" : "pane"} aria-hidden={!active} />;
}

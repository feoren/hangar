import { useLayoutEffect, useRef } from "react";

interface Props {
  labelledBy: string;
  className?: string;
  /** Escape, a click outside or the close button. The caller returns focus to the terminal. */
  onClose: () => void;
  children: React.ReactNode;
}

/** Everything reachable via Tab; the focus trap wraps between the first and the last. */
const FOCUSABLE = [
  "button:not(:disabled)",
  "input:not(:disabled)",
  "select:not(:disabled)",
  "textarea:not(:disabled)",
  '[contenteditable]:not([contenteditable="false"])',
  "[href]",
  '[tabindex]:not([tabindex="-1"])',
].join(", ");

/** Modal dialog: focus moves in on open (element with `data-autofocus`), Tab stays inside. */
export function Dialog({ labelledBy, className, onClose, children }: Props) {
  const panel = useRef<HTMLDivElement>(null);

  // Layout effect, so focus is in place before the app hands it to the terminal next frame.
  useLayoutEffect(() => {
    const element = panel.current;
    if (!element) return;
    const target =
      element.querySelector<HTMLElement>("[data-autofocus]") ??
      element.querySelector<HTMLElement>(FOCUSABLE) ??
      element;
    target.focus();
  }, []);

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "Escape") {
      event.stopPropagation();
      onClose();
      return;
    }
    if (event.key !== "Tab" || !panel.current) return;
    const items = Array.from(panel.current.querySelectorAll<HTMLElement>(FOCUSABLE));
    const first = items[0];
    const last = items[items.length - 1];
    if (!first || !last) return;
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div
      className="overlay"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={panel}
        className={className ? `dialog ${className}` : "dialog"}
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
        tabIndex={-1}
        onKeyDown={onKeyDown}
      >
        {children}
      </div>
    </div>
  );
}

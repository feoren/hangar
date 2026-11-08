import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";

import { firstIndex, fitInViewport, menuCommand, secondaryCommand, type NavEntry } from "./menuNav";

/** Second button at the end of a row, e.g. the favorite star or "+ Worktree". */
export interface PopupSecondary {
  label: string;
  icon: React.ReactNode;
  /** Toggle state (favorite star): announced as a checkbox item. */
  checked?: boolean;
  /** The menu stays open (toggles); otherwise it closes like for the row. */
  keepOpen?: boolean;
  run: () => void;
}

export type PopupItem =
  | {
      type: "action";
      key: string;
      label: string;
      /** Dimmed suffix after the name, e.g. the version. */
      detail?: string;
      shortcut?: string;
      /** Leading image, e.g. a program's badge. */
      icon?: React.ReactNode;
      /** Tooltip, e.g. the full path behind a shortened label. */
      title?: string;
      disabled?: boolean;
      /** Marks the row (e.g. the current folder); read out as "checked". */
      current?: boolean;
      /** Also reachable with Right arrow, Shift+Enter and Shift+click on the row. */
      secondary?: PopupSecondary;
      run: () => void;
    }
  | { type: "separator"; key: string }
  /** Small caption above a group of entries; not focusable. */
  | { type: "heading"; key: string; label: string }
  /**
   * A short sentence in the menu, e.g. why an option is missing; not focusable. `describes`
   * names the key of the entry it explains: screen readers read it along with that entry.
   */
  | { type: "note"; key: string; label: string; describes?: string };

interface Props {
  items: readonly PopupItem[];
  /** Name of the menu for screen readers. */
  label: string;
  /** Top left corner in the window (clientX/clientY); the menu moves into the window if space is short. */
  position: { x: number; y: number };
  /**
   * Opened from a button near the bottom: if the menu does not fit below `position`, its bottom
   * edge goes to this y instead (above the button) rather than covering the button.
   */
  flipAbove?: number;
  className?: string;
  /** Button that opens the menu: clicking it toggles instead of closing and reopening. */
  anchor?: React.RefObject<HTMLElement | null>;
  /**
   * Closed after an action (`action`) or via Escape, Tab, a click outside, a window switch
   * (`dismiss`). The caller returns focus.
   */
  onClose: (reason: "action" | "dismiss") => void;
}

/**
 * Floating single-level menu (terminal context menu, program picker of the collapsed sidebar,
 * folder menu). Same keyboard control as the app menu: focus on the first entry, arrows, Home,
 * End, first letter, Enter, Escape; Right and Left reach a row's second button.
 */
export function PopupMenu({
  items,
  label,
  position,
  flipAbove,
  className,
  anchor,
  onClose,
}: Props) {
  const entries: NavEntry[] = items.map((item) =>
    item.type !== "action"
      ? null
      : { label: item.label, enabled: item.disabled !== true, submenu: false },
  );
  const [current, setCurrent] = useState(() => firstIndex(entries));
  const [onSecondary, setOnSecondary] = useState(false);
  const [placed, setPlaced] = useState(position);
  const panel = useRef<HTMLUListElement>(null);
  const buttons = useRef(new Map<number, HTMLButtonElement>());
  const secondaries = useRef(new Map<number, HTMLButtonElement>());
  const closeRef = useRef(onClose);
  const noteId = useId();
  /** Note ids per described entry key. */
  const notesFor = new Map<string, string[]>();
  items.forEach((item, index) => {
    if (item.type !== "note" || item.describes === undefined) return;
    const ids = notesFor.get(item.describes) ?? [];
    ids.push(`${noteId}-${String(index)}`);
    notesFor.set(item.describes, ids);
  });
  useEffect(() => {
    closeRef.current = onClose;
  });

  // Measure before the first paint and push into the window, so nothing jumps.
  useLayoutEffect(() => {
    const element = panel.current;
    if (element) {
      const rect = element.getBoundingClientRect();
      const fitsBelow = position.y + rect.height + 8 <= window.innerHeight;
      const start =
        !fitsBelow && flipAbove !== undefined
          ? { x: position.x, y: flipAbove - rect.height }
          : position;
      setPlaced(
        fitInViewport(
          start,
          { width: rect.width, height: rect.height },
          { width: window.innerWidth, height: window.innerHeight },
        ),
      );
    }
    const first = buttons.current.get(current);
    if (first) first.focus();
    else element?.focus();
    // Only on open.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const dismiss = () => {
      closeRef.current("dismiss");
    };
    const onPointer = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Node && panel.current?.contains(target)) return;
      if (target instanceof Node && anchor?.current?.contains(target)) return;
      dismiss();
    };
    document.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("blur", dismiss);
    window.addEventListener("resize", dismiss);
    return () => {
      document.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("blur", dismiss);
      window.removeEventListener("resize", dismiss);
    };
  }, [anchor]);

  const activate = (index: number) => {
    const item = items[index];
    if (!item || item.type !== "action" || item.disabled === true) return;
    onClose("action");
    item.run();
  };

  const runSecondary = (index: number) => {
    const item = items[index];
    if (!item || item.type !== "action" || item.disabled === true || !item.secondary) return;
    if (item.secondary.keepOpen !== true) onClose("action");
    item.secondary.run();
  };

  const focusRow = (index: number, secondary: boolean) => {
    setCurrent(index);
    setOnSecondary(secondary);
    (secondary ? secondaries.current : buttons.current).get(index)?.focus();
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    event.stopPropagation();
    const item = items[current];
    const hasSecondary = item?.type === "action" && item.secondary !== undefined;
    const second = secondaryCommand(event, hasSecondary, onSecondary);
    if (second !== null) {
      event.preventDefault();
      if (second === "enter") focusRow(current, true);
      else if (second === "leave") focusRow(current, false);
      else runSecondary(current);
      return;
    }
    const command = menuCommand(event, entries, current, false);
    switch (command.kind) {
      case "none":
        return;
      case "focus":
        focusRow(command.index, false);
        break;
      case "activate":
        activate(current);
        break;
      case "openSubmenu":
      case "closeSubmenu":
        return;
      case "closeAll":
        onClose("dismiss");
        break;
    }
    event.preventDefault();
  };

  return (
    <ul
      ref={panel}
      className={className ? `menu-panel popup-menu ${className}` : "menu-panel popup-menu"}
      role="menu"
      aria-label={label}
      tabIndex={-1}
      style={{ left: placed.x, top: placed.y }}
      onKeyDown={onKeyDown}
      onContextMenu={(event) => {
        event.preventDefault();
      }}
    >
      {items.map((item, index) => {
        if (item.type === "separator") {
          return <li key={item.key} role="separator" className="menu-separator" />;
        }
        if (item.type === "heading") {
          return (
            <li key={item.key} role="presentation" className="menu-heading">
              {item.label}
            </li>
          );
        }
        if (item.type === "note") {
          return (
            <li
              key={item.key}
              id={`${noteId}-${String(index)}`}
              role="presentation"
              className="menu-note"
            >
              {item.label}
            </li>
          );
        }
        const secondary = item.secondary;
        return (
          <li
            key={item.key}
            className={secondary ? "menu-entry with-secondary" : "menu-entry"}
            role="none"
          >
            <button
              type="button"
              role={item.current === undefined ? "menuitem" : "menuitemradio"}
              aria-checked={item.current}
              aria-describedby={notesFor.get(item.key)?.join(" ")}
              ref={(element) => {
                if (element) buttons.current.set(index, element);
                else buttons.current.delete(index);
              }}
              tabIndex={index === current && !onSecondary ? 0 : -1}
              className={item.current === true ? "menu-item current" : "menu-item"}
              title={item.title}
              disabled={item.disabled}
              onPointerEnter={() => {
                if (item.disabled === true) return;
                focusRow(index, false);
              }}
              onClick={(event) => {
                if (event.shiftKey && secondary) runSecondary(index);
                else activate(index);
              }}
            >
              {item.icon}
              <span className="menu-label">
                {item.label}
                {item.detail && (
                  <span className="menu-detail">
                    <span aria-hidden="true"> · </span>
                    {item.detail}
                  </span>
                )}
              </span>
              <span className="menu-hint">{item.shortcut ?? ""}</span>
            </button>
            {secondary && (
              <button
                type="button"
                role={secondary.checked === undefined ? "menuitem" : "menuitemcheckbox"}
                aria-checked={secondary.checked}
                aria-label={secondary.label}
                title={secondary.label}
                ref={(element) => {
                  if (element) secondaries.current.set(index, element);
                  else secondaries.current.delete(index);
                }}
                tabIndex={index === current && onSecondary ? 0 : -1}
                className={
                  secondary.checked === true
                    ? "menu-secondary icon-button on"
                    : "menu-secondary icon-button"
                }
                disabled={item.disabled}
                onPointerEnter={() => {
                  if (item.disabled === true) return;
                  focusRow(index, true);
                }}
                onClick={() => {
                  runSecondary(index);
                }}
              >
                {secondary.icon}
              </button>
            )}
          </li>
        );
      })}
    </ul>
  );
}

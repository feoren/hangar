import { useEffect, useLayoutEffect, useRef, useState } from "react";

import { firstIndex, fitInViewport, menuCommand, type NavEntry } from "./menuNav";

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
      disabled?: boolean;
      run: () => void;
    }
  | { type: "separator"; key: string };

interface Props {
  items: readonly PopupItem[];
  /** Name of the menu for screen readers. */
  label: string;
  /** Top left corner in the window (clientX/clientY); the menu moves into the window if space is short. */
  position: { x: number; y: number };
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
 * Floating single-level menu (terminal context menu, program picker of the collapsed sidebar).
 * Same keyboard control as the app menu: focus on the first entry, arrows, Home, End,
 * first letter, Enter, Escape.
 */
export function PopupMenu({ items, label, position, className, anchor, onClose }: Props) {
  const entries: NavEntry[] = items.map((item) =>
    item.type === "separator"
      ? null
      : { label: item.label, enabled: item.disabled !== true, submenu: false },
  );
  const [current, setCurrent] = useState(() => firstIndex(entries));
  const [placed, setPlaced] = useState(position);
  const panel = useRef<HTMLUListElement>(null);
  const buttons = useRef(new Map<number, HTMLButtonElement>());
  const closeRef = useRef(onClose);
  useEffect(() => {
    closeRef.current = onClose;
  });

  // Measure before the first paint and push into the window, so nothing jumps.
  useLayoutEffect(() => {
    const element = panel.current;
    if (element) {
      const rect = element.getBoundingClientRect();
      setPlaced(
        fitInViewport(
          position,
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

  const onKeyDown = (event: React.KeyboardEvent) => {
    event.stopPropagation();
    const command = menuCommand(event, entries, current, false);
    switch (command.kind) {
      case "none":
        return;
      case "focus":
        setCurrent(command.index);
        buttons.current.get(command.index)?.focus();
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
        return (
          <li key={item.key} className="menu-entry" role="none">
            <button
              type="button"
              role="menuitem"
              ref={(element) => {
                if (element) buttons.current.set(index, element);
                else buttons.current.delete(index);
              }}
              tabIndex={index === current ? 0 : -1}
              className="menu-item"
              disabled={item.disabled}
              onPointerEnter={() => {
                if (item.disabled === true) return;
                setCurrent(index);
                buttons.current.get(index)?.focus();
              }}
              onClick={() => {
                activate(index);
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
          </li>
        );
      })}
    </ul>
  );
}

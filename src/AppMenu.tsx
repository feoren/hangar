import { useEffect, useLayoutEffect, useRef, useState } from "react";

import { useT } from "./i18n";
import { MenuIcon } from "./icons";
import {
  firstIndex,
  INITIAL_SUBMENU,
  menuCommand,
  pointerMoved,
  submenuAfterFocus,
  type NavEntry,
  type PointerPoint,
} from "./menuNav";

export type MenuItem =
  | {
      type: "action";
      label: string;
      /** Dimmed suffix after the name, e.g. a program's version. */
      detail?: string;
      shortcut?: string;
      disabled?: boolean;
      run: () => void;
    }
  | { type: "submenu"; label: string; disabled?: boolean; items: MenuItem[] }
  | { type: "separator" };

export interface MenuGroup {
  label: string;
  items: MenuItem[];
}

interface Props {
  groups: MenuGroup[];
  /** After an action or Escape: focus back to the terminal. */
  onClose: () => void;
}

/** The keyboard navigation's view of the entries (menuNav.ts). */
function navEntries(items: readonly MenuItem[]): NavEntry[] {
  return items.map((item) =>
    item.type === "separator"
      ? null
      : {
          label: item.label,
          enabled: item.disabled !== true && (item.type === "action" || item.items.length > 0),
          submenu: item.type === "submenu",
        },
  );
}

/**
 * Menu behind the three-line icon: File, Edit, View, Help with submenus. Operated following
 * the WAI-ARIA menu pattern: on open the first entry has focus (no submenu opens by itself), Up
 * and Down arrows move, Right or Enter opens a submenu, Left closes it, Home and End jump, a
 * first letter jumps to the matching entry, Escape closes one level, Tab the whole menu. With the
 * mouse, a submenu opens on click or once the pointer moves onto its entry.
 */
export function AppMenu({ groups, onClose }: Props) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const t = useT();

  useEffect(() => {
    if (!open) return;
    const close = () => {
      setOpen(false);
      onClose();
    };
    const onPointer = (event: PointerEvent) => {
      if (event.target instanceof Node && root.current?.contains(event.target)) return;
      close();
    };
    // Escape without focus in the menu (e.g. after a click outside that did not take focus).
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !event.defaultPrevented) close();
    };
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("keydown", onKey);
    window.addEventListener("blur", close);
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", close);
    };
  }, [open, onClose]);

  const items: MenuItem[] = groups.map((group) => ({
    type: "submenu",
    label: group.label,
    items: group.items,
  }));

  return (
    <div className="app-menu" ref={root}>
      <button
        type="button"
        className="icon-button menu-button"
        aria-label={t("menu.label")}
        aria-haspopup="menu"
        aria-expanded={open}
        title={t("menu.label")}
        onClick={() => {
          setOpen((current) => !current);
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" && !open) {
            event.preventDefault();
            setOpen(true);
          }
        }}
      >
        <MenuIcon />
      </button>
      {open && (
        <MenuPanel
          items={items}
          label={t("menu.label")}
          root
          autoFocus
          onDone={() => {
            setOpen(false);
            onClose();
          }}
          onBack={() => undefined}
        />
      )}
    </div>
  );
}

function MenuPanel({
  items,
  label,
  root = false,
  autoFocus,
  onDone,
  onBack,
}: {
  items: MenuItem[];
  label: string;
  root?: boolean;
  /** Focus the first entry on mount (opening by keyboard or opening the menu). */
  autoFocus: boolean;
  onDone: () => void;
  /** Close the submenu via Left arrow or Escape; the parent entry takes focus. */
  onBack: () => void;
}) {
  const entries = navEntries(items);
  const [current, setCurrent] = useState(() => firstIndex(entries));
  const [openIndex, setOpenIndex] = useState<number | null>(INITIAL_SUBMENU);
  const [childFocus, setChildFocus] = useState(false);
  const buttons = useRef(new Map<number, HTMLButtonElement>());
  /** Last pointer position over this level, to tell real movement from a resting pointer. */
  const lastPointer = useRef<PointerPoint | null>(null);

  useLayoutEffect(() => {
    if (autoFocus) buttons.current.get(current)?.focus();
    // Only on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const focusAt = (index: number) => {
    setCurrent(index);
    buttons.current.get(index)?.focus();
    // Arrows only move; Right arrow or Enter opens a submenu.
    const next = submenuAfterFocus(openIndex, index);
    if (next !== openIndex) {
      setChildFocus(false);
      setOpenIndex(next);
    }
  };

  /** Pointer moved over an entry: it takes focus, a submenu entry opens its submenu. */
  const hover = (event: React.PointerEvent<HTMLLIElement>, index: number) => {
    // Moves inside an open submenu bubble up through this entry; that level handles them.
    if (
      event.target instanceof Element &&
      event.target.closest(".menu-entry") !== event.currentTarget
    ) {
      return;
    }
    const point = { x: event.clientX, y: event.clientY };
    const moved = pointerMoved(lastPointer.current, point);
    lastPointer.current = point;
    const item = items[index];
    if (!moved || item === undefined || item.type === "separator" || item.disabled === true) return;
    const wanted = item.type === "submenu" ? index : null;
    if (index === current && openIndex === wanted) return;
    setCurrent(index);
    buttons.current.get(index)?.focus();
    setChildFocus(false);
    setOpenIndex(wanted);
  };

  const openSubmenu = (index: number, focusChild: boolean) => {
    setCurrent(index);
    setChildFocus(focusChild);
    setOpenIndex(index);
  };

  const activate = (index: number) => {
    const item = items[index];
    if (!item || item.type !== "action" || item.disabled === true) return;
    onDone();
    item.run();
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    // Submenus sit in the DOM inside their parent entry: each level handles only its own
    // keys and does not let them bubble up.
    event.stopPropagation();
    const command = menuCommand(event, entries, current, !root);
    switch (command.kind) {
      case "none":
        return;
      case "focus":
        focusAt(command.index);
        break;
      case "activate":
        activate(current);
        break;
      case "openSubmenu":
        openSubmenu(current, true);
        break;
      case "closeSubmenu":
        onBack();
        break;
      case "closeAll":
        onDone();
        break;
    }
    // Tab only closes; focus then goes to the terminal, not to the next element.
    event.preventDefault();
  };

  return (
    <ul
      className={root ? "menu-panel menu-root" : "menu-panel"}
      role="menu"
      aria-label={label}
      onKeyDown={onKeyDown}
    >
      {items.map((item, index) => {
        if (item.type === "separator") {
          return <li key={`sep-${String(index)}`} role="separator" className="menu-separator" />;
        }
        const isOpen = item.type === "submenu" && openIndex === index;
        return (
          <li
            key={item.label}
            className="menu-entry"
            role="none"
            onPointerMove={(event) => {
              hover(event, index);
            }}
          >
            <button
              type="button"
              role="menuitem"
              ref={(element) => {
                if (element) buttons.current.set(index, element);
                else buttons.current.delete(index);
              }}
              tabIndex={index === current ? 0 : -1}
              className={isOpen ? "menu-item open" : "menu-item"}
              disabled={item.disabled}
              aria-haspopup={item.type === "submenu" ? "menu" : undefined}
              aria-expanded={item.type === "submenu" ? isOpen : undefined}
              onClick={(event) => {
                if (item.type === "submenu") {
                  // Triggered by keyboard (detail 0): go straight into the submenu.
                  openSubmenu(index, event.detail === 0);
                  return;
                }
                activate(index);
              }}
            >
              <span className="menu-label">
                {item.label}
                {item.type === "action" && item.detail && (
                  <span className="menu-detail">
                    <span aria-hidden="true"> · </span>
                    {item.detail}
                  </span>
                )}
              </span>
              <span className="menu-hint" aria-hidden={item.type === "submenu" ? true : undefined}>
                {item.type === "submenu" ? "›" : (item.shortcut ?? "")}
              </span>
            </button>
            {isOpen && item.items.length > 0 && (
              <MenuPanel
                // Remount when the preview turns into an open with focus.
                key={childFocus ? "focus" : "preview"}
                items={item.items}
                label={item.label}
                autoFocus={childFocus}
                onDone={onDone}
                onBack={() => {
                  setOpenIndex(null);
                  setChildFocus(false);
                  buttons.current.get(index)?.focus();
                }}
              />
            )}
          </li>
        );
      })}
    </ul>
  );
}
